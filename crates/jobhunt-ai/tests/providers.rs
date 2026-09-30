//! The providers against local mock servers: the request each sends, how
//! answers are read and validated, and which failures are retried.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use jobhunt_ai::{ModelConfig, ModelInterpreter};
use jobhunt_profile::taste::reading::{InterpretError, TasteInterpreter, TasteRequest, Words};
use jobhunt_profile::taste::{Polarity, TasteDimension, TasteOrigin};
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const WORDS: &str = "Small technical teams, high ownership. I don't want early-career roles.";

fn request() -> TasteRequest {
    TasteRequest {
        words: vec![Words {
            text: WORDS.into(),
            statement: None,
        }],
        evidence: Vec::new(),
        feedback: Vec::new(),
        settled: Vec::new(),
        focus: None,
    }
}

fn reading() -> Value {
    json!({
        "summary": "Senior work on small, high-ownership teams.",
        "preferences": [
            {"dimension": "team", "value": "small_team", "polarity": "prefer", "confidence": "high",
             "statement": "Small technical teams", "explanation": "Stated.",
             "sources": [{"kind": "words", "ref": "Small technical teams"}]},
            {"dimension": "seniority", "value": "early_career", "polarity": "avoid", "confidence": "high",
             "statement": "Early-career roles", "explanation": "Stated.",
             "sources": [{"kind": "words", "ref": "I don't want early-career roles"}]}
        ],
        "ambiguities": [],
        "constraints_noted": []
    })
}

fn anthropic_answer(text: &str, stop: &str) -> Value {
    json!({
        "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-opus-5-5",
        "content": [{"type": "text", "text": text}],
        "stop_reason": stop,
        "usage": {"input_tokens": 10, "output_tokens": 10}
    })
}

fn config(server: &MockServer, provider: &str, model: Option<&str>) -> ModelConfig {
    let base = if provider == "openai" {
        format!("{}/v1", server.uri())
    } else {
        server.uri()
    };
    let mut c =
        ModelConfig::new(provider, model, Some(&base), Some("sk-test".into()), "KEY").unwrap();
    c.backoff = Duration::from_millis(1);
    c.timeout = Duration::from_secs(5);
    c
}

#[tokio::test]
async fn anthropic_uses_structured_output_and_reads_the_answer() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "sk-test"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(anthropic_answer(&reading().to_string(), "end_turn")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let model = ModelInterpreter::new(config(&server, "anthropic", None)).unwrap();
    assert_eq!(model.name(), "model/anthropic:claude-opus-5-5");
    assert!(model.is_remote());
    let r = model.interpret(&request()).await.unwrap();
    assert_eq!(r.assertions.len(), 2);
    assert_eq!(r.assertions[0].dimension, TasteDimension::Team);
    assert_eq!(r.assertions[1].polarity, Polarity::Avoid);
    assert_eq!(r.assertions[1].origin, TasteOrigin::Interpreted);

    let sent: Value =
        serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
    assert_eq!(sent["model"], "claude-opus-5-5");
    assert_eq!(sent["output_config"]["format"]["type"], "json_schema");
    assert_eq!(sent["output_config"]["effort"], "low");
    assert!(
        sent.get("fallbacks").is_none(),
        "only on the hosted Claude API"
    );
    let prompt = sent["messages"][0]["content"].as_str().unwrap();
    assert!(prompt.contains(WORDS));
    assert!(sent["system"].as_str().unwrap().contains("Taste only"));
}

#[tokio::test]
async fn openai_compatible_servers_use_json_schema_response_format() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer sk-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"index": 0, "finish_reason": "stop",
                         "message": {"role": "assistant", "content": reading().to_string(), "refusal": null}}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let model = ModelInterpreter::new(config(&server, "openai", Some("local-model"))).unwrap();
    let r = model.interpret(&request()).await.unwrap();
    assert_eq!(r.assertions.len(), 2);
    assert_eq!(r.interpreter, "model/openai:local-model");
    let sent: Value =
        serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
    assert_eq!(sent["response_format"]["type"], "json_schema");
    assert_eq!(sent["response_format"]["json_schema"]["strict"], true);
    assert_eq!(sent["messages"][0]["role"], "system");
}

#[tokio::test]
async fn transient_failures_and_garbled_answers_are_retried() {
    let server = MockServer::start().await;
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = calls.clone();
    Mock::given(method("POST"))
        .respond_with(move |_: &Request| {
            match counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
                0 => ResponseTemplate::new(529),
                1 => ResponseTemplate::new(200)
                    .set_body_json(anthropic_answer("Sure! small teams", "end_turn")),
                _ => ResponseTemplate::new(200)
                    .set_body_json(anthropic_answer(&reading().to_string(), "end_turn")),
            }
        })
        .mount(&server)
        .await;
    let mut c = config(&server, "anthropic", None);
    c.max_retries = 2;
    let r = ModelInterpreter::new(c)
        .unwrap()
        .interpret(&request())
        .await
        .unwrap();
    assert_eq!(r.assertions.len(), 2);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 3);
}

#[tokio::test]
async fn malformed_output_fails_safely_after_the_retries() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(anthropic_answer(
            r#"{"preferences": "small teams"}"#,
            "end_turn",
        )))
        .expect(2)
        .mount(&server)
        .await;
    let error = ModelInterpreter::new(config(&server, "anthropic", None))
        .unwrap()
        .interpret(&request())
        .await
        .unwrap_err();
    assert!(matches!(error, InterpretError::Malformed(_)), "{error:?}");
    assert!(
        !error.to_string().contains("small teams"),
        "errors never carry the answer"
    );
}

#[tokio::test]
async fn refusals_truncation_and_client_errors_are_not_retried() {
    for (response, expected) in [
        (
            ResponseTemplate::new(200).set_body_json(anthropic_answer("", "refusal")),
            InterpretError::Refused,
        ),
        (
            ResponseTemplate::new(200).set_body_json(anthropic_answer("{\"summ", "max_tokens")),
            InterpretError::Truncated,
        ),
        (
            ResponseTemplate::new(400).set_body_string("bad request"),
            InterpretError::Status {
                status: 400,
                retryable: false,
            },
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;
        let error = ModelInterpreter::new(config(&server, "anthropic", None))
            .unwrap()
            .interpret(&request())
            .await
            .unwrap_err();
        assert_eq!(error, expected);
    }
}

#[tokio::test]
async fn an_unreachable_model_is_a_transport_error() {
    let mut c = ModelConfig::new(
        "openai",
        Some("m"),
        Some("http://127.0.0.1:9/v1"),
        None,
        "KEY",
    )
    .unwrap();
    c.max_retries = 0;
    c.timeout = Duration::from_secs(2);
    let error = ModelInterpreter::new(c)
        .unwrap()
        .interpret(&request())
        .await
        .unwrap_err();
    assert!(matches!(error, InterpretError::Transport(_)), "{error:?}");
}
