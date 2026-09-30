//! The fit reviewer against local mock servers: what it sends (and never
//! sends), how answers are validated, the tokens it reports, and which
//! failures are retried. A failure is an error for the ranking to absorb
//! (the rules' assessment stands), never a guess.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use jobhunt_ai::{ModelConfig, ModelFitReviewer};
use jobhunt_ranking::review::{CandidateBrief, FitReviewRequest, JobBrief};
use jobhunt_ranking::{FitLevel, FitReviewer, ReviewError};
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn request() -> FitReviewRequest {
    FitReviewRequest {
        candidate: CandidateBrief {
            taste: vec![
                "prefer · work_shape: backend (Backend engineering) · said".into(),
                "avoid · specialization: deep (Deep specialist roles) · confirmed".into(),
            ],
            level: Some("senior (latest title)".into()),
            specialties: Vec::new(),
            experience: vec![
                "Senior Software Engineer · 2022–present · work: backend · technologies: Go, PostgreSQL"
                    .into(),
            ],
        },
        job: JobBrief {
            title: "OrioleDB Developer".into(),
            company: "Supabase".into(),
            setup: "Remote · remote".into(),
            reading: vec!["specialized: database-engine internals".into()],
            text: vec![
                "Develop the OrioleDB storage engine: page layout, B-tree indexes, the buffer manager."
                    .into(),
                "Write PostgreSQL core patches and upstream them.".into(),
            ],
        },
    }
}

fn answer(fit: &str) -> Value {
    json!({
        "fit": fit,
        "role_fit": "mismatch",
        "company_fit": "unknown",
        "seniority": "unknown",
        "specialization": "mismatch",
        "affirmative": [],
        "contradictions": [
            {"aspect": "specialization",
             "reason": "Building a storage engine is far deeper than the application PostgreSQL work they have done",
             "quote": "Develop the OrioleDB storage engine"}
        ],
        "uncertainties": ["The posting doesn't say how big the team is"]
    })
}

fn anthropic(text: &str, stop: &str) -> Value {
    json!({
        "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-opus-5-5",
        "content": [{"type": "text", "text": text}],
        "stop_reason": stop,
        "usage": {"input_tokens": 2400, "output_tokens": 350}
    })
}

fn config(server: &MockServer) -> ModelConfig {
    let mut c = ModelConfig::new(
        "anthropic",
        None,
        Some(&server.uri()),
        Some("sk-test".into()),
        "KEY",
    )
    .unwrap();
    c.backoff = Duration::from_millis(1);
    c.timeout = Duration::from_secs(5);
    c
}

#[tokio::test]
async fn sends_the_request_as_structured_output_and_reads_the_review() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "sk-test"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(anthropic(&answer("poor").to_string(), "end_turn")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let reviewer = ModelFitReviewer::new(config(&server)).unwrap();
    assert_eq!(reviewer.name(), "model/anthropic:claude-opus-5-5");
    let review = reviewer.review(&request()).await.unwrap();
    assert_eq!(review.fit, FitLevel::Poor);
    assert_eq!(review.contradictions.len(), 1);
    assert_eq!((review.input_tokens, review.output_tokens), (2400, 350));
    assert_eq!(review.reviewer, "model/anthropic:claude-opus-5-5");

    let sent: Value =
        serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
    assert_eq!(sent["model"], "claude-opus-5-5");
    assert_eq!(sent["output_config"]["format"]["type"], "json_schema");
    assert_eq!(
        sent["output_config"]["format"]["schema"]["required"][0],
        "fit"
    );
    assert_eq!(sent["output_config"]["effort"], "low");
    let system = sent["system"].as_str().unwrap();
    assert!(system.contains("Judge FIT only"));
    assert!(system.contains("Pay, visas, relocation"));
    let prompt = sent["messages"][0]["content"].as_str().unwrap();
    assert!(prompt.contains("<taste>"));
    assert!(prompt.contains("Develop the OrioleDB storage engine"));
    for private in ["@", "Rafael", "linkedin", "prof_", "USD"] {
        assert!(!prompt.contains(private), "{private} is never sent");
    }
}

#[tokio::test]
async fn rate_limits_are_retried_then_absorbed() {
    let server = MockServer::start().await;
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = calls.clone();
    Mock::given(method("POST"))
        .respond_with(move |_: &Request| {
            match counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
                0 => ResponseTemplate::new(429),
                _ => ResponseTemplate::new(200)
                    .set_body_json(anthropic(&answer("poor").to_string(), "end_turn")),
            }
        })
        .mount(&server)
        .await;
    let review = ModelFitReviewer::new(config(&server))
        .unwrap()
        .review(&request())
        .await
        .unwrap();
    assert_eq!(review.fit, FitLevel::Poor);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);

    // Still limited after the retries: an error the ranking absorbs.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429))
        .expect(2)
        .mount(&server)
        .await;
    let error = ModelFitReviewer::new(config(&server))
        .unwrap()
        .review(&request())
        .await
        .unwrap_err();
    assert_eq!(error.short(), "rate limited");
}

#[tokio::test]
async fn malformed_refused_and_truncated_answers_fail_safely() {
    for (response, check) in [
        (
            ResponseTemplate::new(200)
                .set_body_json(anthropic(r#"{"fit": "strong", "score": 0.97}"#, "end_turn")),
            (|e: &ReviewError| matches!(e, ReviewError::Malformed(_))) as fn(&ReviewError) -> bool,
        ),
        (
            ResponseTemplate::new(200).set_body_json(anthropic("", "refusal")),
            |e: &ReviewError| *e == ReviewError::Refused,
        ),
        (
            ResponseTemplate::new(200).set_body_json(anthropic("{\"fit", "max_tokens")),
            |e: &ReviewError| *e == ReviewError::Truncated,
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(response)
            .mount(&server)
            .await;
        let error = ModelFitReviewer::new(config(&server))
            .unwrap()
            .review(&request())
            .await
            .unwrap_err();
        assert!(check(&error), "{error:?}");
        assert!(
            !error.to_string().contains("0.97"),
            "errors never carry the answer"
        );
    }
}

#[tokio::test]
async fn a_timeout_is_a_transport_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(anthropic(&answer("poor").to_string(), "end_turn"))
                .set_delay(Duration::from_secs(3)),
        )
        .mount(&server)
        .await;
    let mut c = config(&server);
    c.timeout = Duration::from_millis(200);
    c.max_retries = 0;
    let error = ModelFitReviewer::new(c)
        .unwrap()
        .review(&request())
        .await
        .unwrap_err();
    assert_eq!(error, ReviewError::Transport("timed out".into()));
}

#[tokio::test]
async fn an_invented_quote_cannot_make_a_strong_fit() {
    let invented = json!({
        "fit": "strong",
        "role_fit": "match",
        "company_fit": "match",
        "seniority": "match",
        "specialization": "match",
        "affirmative": [
            {"aspect": "role", "reason": "Backend work", "quote": "own backend services"},
            {"aspect": "company", "reason": "A famous startup", "quote": "Supabase is a startup of 80"}
        ],
        "contradictions": [],
        "uncertainties": []
    });
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(anthropic(&invented.to_string(), "end_turn")),
        )
        .mount(&server)
        .await;
    let review = ModelFitReviewer::new(config(&server))
        .unwrap()
        .review(&request())
        .await
        .unwrap();
    assert_eq!(review.rejected, 2);
    assert!(review.affirmative.is_empty());
    assert_eq!(
        review.fit,
        FitLevel::Plausible,
        "no quoted evidence, no strong fit"
    );
}
