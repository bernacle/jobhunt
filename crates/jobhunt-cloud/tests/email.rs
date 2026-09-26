//! The Resend sender against a local mock of Resend's API: what is sent
//! (auth, idempotency key, body), and how answers are classified. No test
//! sends real email.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use jobhunt_cloud::email::{EmailMessage, EmailSender, ResendSender, SendError};
use serde_json::json;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn message() -> EmailMessage {
    EmailMessage {
        from: "JobHunt <notify@jobhunt.test>".into(),
        to: "ana@example.com".into(),
        subject: "A strong new match: Staff Engineer at Acme".into(),
        text: "One new opportunity looks worth your time.".into(),
        html: "<p>One new opportunity looks worth your time.</p>".into(),
        headers: vec![(
            "List-Unsubscribe".into(),
            "<https://app.jobhunt.test/settings>".into(),
        )],
    }
}

#[tokio::test]
async fn sends_with_the_key_and_the_idempotency_key() {
    let resend = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/emails"))
        .and(header("authorization", "Bearer re_test_key"))
        .and(header("idempotency-key", "ntf_0123"))
        .and(body_partial_json(json!({
            "from": "JobHunt <notify@jobhunt.test>",
            "to": ["ana@example.com"],
            "subject": "A strong new match: Staff Engineer at Acme",
            "headers": {"List-Unsubscribe": "<https://app.jobhunt.test/settings>"},
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "email_1"})))
        .expect(1)
        .mount(&resend)
        .await;
    let sender = ResendSender::new("re_test_key".into(), resend.uri()).unwrap();
    let receipt = sender.send(&message(), "ntf_0123").await.unwrap();
    assert_eq!(receipt.message_id.as_deref(), Some("email_1"));
}

#[tokio::test]
async fn answers_are_classified_retryable_or_permanent() {
    let cases = [
        (429, json!({"name": "rate_limit_exceeded"}), true),
        (500, json!({"name": "internal_server_error"}), true),
        (409, json!({"name": "concurrent_idempotent_requests"}), true),
        (409, json!({"name": "invalid_idempotent_request"}), false),
        (422, json!({"name": "validation_error"}), false),
        (403, json!({"name": "invalid_from_address"}), false),
    ];
    for (status, body, retryable) in cases {
        let resend = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/emails"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body.clone()))
            .mount(&resend)
            .await;
        let sender = ResendSender::new("re_test_key".into(), resend.uri()).unwrap();
        let error = sender.send(&message(), "ntf_1").await.unwrap_err();
        assert_eq!(
            matches!(error, SendError::Retryable(_)),
            retryable,
            "{status} {body}: {error}"
        );
        // The recipient never appears in the error (it is logged).
        assert!(!error.to_string().contains("ana@example.com"));
    }
    // Unreachable: retryable.
    let sender = ResendSender::new("re_test_key".into(), "http://127.0.0.1:9".into()).unwrap();
    assert!(matches!(
        sender.send(&message(), "ntf_2").await,
        Err(SendError::Retryable(_))
    ));
}
