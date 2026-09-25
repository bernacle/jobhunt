//! First-party verification over HTTP against a local mock server serving
//! the saved real responses: active and closed listings, working and
//! broken application paths, timeouts, 5xx and unreadable answers, for
//! every family.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use chrono::{TimeZone, Utc};
use jobhunt_core::{CanonicalUrl, SourceKey};
use jobhunt_jobs::verification::{
    ApplicationBasis, ApplicationStatus, LinkKind, ListingVerifier, ObserveError, ObservedListing,
    SourceObservation, VerificationMethod,
};
use jobhunt_jobs::{JobPosting, JobRecord, JobStatus, OpportunityId};
use jobhunt_sources::yc::{CompanyContext, page_data};
use jobhunt_sources::{
    HttpClient, HttpSettings, HttpVerifier, VerifierHosts, ashby, greenhouse, lever,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(family: &str, name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{family}/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn http(timeout: Duration) -> HttpClient {
    HttpClient::new(HttpSettings {
        timeout,
        connect_timeout: Duration::from_secs(5),
        max_retries: 1,
        retry_base_delay: Duration::from_millis(1),
        max_per_host: 4,
    })
    .unwrap()
}

fn verifier(server: &MockServer) -> HttpVerifier {
    HttpVerifier::with_hosts(
        http(Duration::from_secs(5)),
        VerifierHosts::all_at(&server.uri()),
    )
}

fn record(posting: JobPosting) -> JobRecord {
    let at = Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap();
    JobRecord {
        id: posting.id(),
        opportunity_id: OpportunityId::founded_by(posting.id()),
        posting,
        first_seen_at: at,
        last_seen_at: at,
        content_updated_at: at,
        status: JobStatus::Open,
        closed_at: None,
    }
}

fn greenhouse_job() -> (serde_json::Value, JobRecord) {
    let board: serde_json::Value =
        serde_json::from_slice(&fixture("greenhouse", "stripe.json")).unwrap();
    let job = board["jobs"][0].clone();
    let context = greenhouse::BoardContext {
        source: SourceKey::new("greenhouse", "stripe").unwrap(),
        board: "stripe".into(),
        company: None,
        fetched_from: None,
    };
    let posting =
        greenhouse::to_posting(serde_json::from_value(job.clone()).unwrap(), &context).unwrap();
    (job, record(posting))
}

fn lever_job() -> (serde_json::Value, JobRecord) {
    let site: serde_json::Value =
        serde_json::from_slice(&fixture("lever", "spotify.json")).unwrap();
    let job = site[0].clone();
    let context = lever::SiteContext {
        source: SourceKey::new("lever", "spotify").unwrap(),
        company: "Spotify".into(),
        fetched_from: None,
    };
    let posting =
        lever::to_posting(serde_json::from_value(job.clone()).unwrap(), &context).unwrap();
    (job, record(posting))
}

fn ashby_job() -> JobRecord {
    let context = ashby::BoardContext {
        source: SourceKey::new("ashby", "linear").unwrap(),
        company: "Linear".into(),
        fetched_from: None,
    };
    let batch = ashby::parse_board(&fixture("ashby", "linear.json"), &context).unwrap();
    record(batch.records.into_iter().next().unwrap())
}

fn yc_job() -> JobRecord {
    let context = CompanyContext {
        source: SourceKey::new("yc", "posthog").unwrap(),
        company: "PostHog".into(),
        fetched_from: CanonicalUrl::parse("https://www.ycombinator.com/companies/posthog/jobs")
            .ok(),
    };
    let page = page_data(&fixture("yc", "posthog_job_104329.html")).unwrap();
    record(jobhunt_sources::yc::to_posting(page.props["job"].clone(), &context).unwrap())
}

fn found(o: &SourceObservation) -> &JobPosting {
    match &o.listing {
        ObservedListing::Found(p) => p,
        other => panic!("expected a live listing, got {other:?}"),
    }
}

async fn serve(server: &MockServer, at: &str, response: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path(at.to_owned()))
        .respond_with(response)
        .mount(server)
        .await;
}

#[tokio::test]
async fn greenhouse_active_listing_with_a_working_application_page() {
    let server = MockServer::start().await;
    let (job, record) = greenhouse_job();
    let id = record.posting.provenance.source_record_id.clone().unwrap();
    serve(
        &server,
        &format!("/v1/boards/stripe/jobs/{id}"),
        ResponseTemplate::new(200).set_body_json(&job),
    )
    .await;
    serve(
        &server,
        &format!("/stripe/jobs/{id}"),
        ResponseTemplate::new(200).set_body_string("<form>apply</form>"),
    )
    .await;

    let o = verifier(&server).observe(&record).await.unwrap();
    assert_eq!(o.method, VerificationMethod::GreenhouseJobApi);
    assert!(o.checked_url.ends_with(&format!(
        "/v1/boards/stripe/jobs/{id}?pay_transparency=true"
    )));
    let posting = found(&o);
    assert_eq!(
        posting.id(),
        record.id,
        "the same job, by the same conversion"
    );
    assert_eq!(posting.snapshot(), record.posting.snapshot());
    assert_eq!(o.application.status, ApplicationStatus::Active);
    assert_eq!(o.application.basis, ApplicationBasis::Probed);
    assert_eq!(o.application.http_status, Some(200));
    let kinds: Vec<LinkKind> = o.chain.iter().map(|l| l.kind).collect();
    assert_eq!(kinds, [LinkKind::AtsApi, LinkKind::ApplicationPage]);
    assert!(o.chain.iter().all(|l| l.checked));
}

#[tokio::test]
async fn greenhouse_deleted_listing_is_not_found() {
    let server = MockServer::start().await;
    let (_, record) = greenhouse_job();
    let id = record.posting.provenance.source_record_id.clone().unwrap();
    serve(
        &server,
        &format!("/v1/boards/stripe/jobs/{id}"),
        ResponseTemplate::new(404).set_body_string(r#"{"status":404,"error":"Job not found"}"#),
    )
    .await;
    let o = verifier(&server).observe(&record).await.unwrap();
    assert!(matches!(o.listing, ObservedListing::NotFound { .. }));
    assert_eq!(o.application.status, ApplicationStatus::Unknown);
}

#[tokio::test]
async fn greenhouse_broken_application_path_is_reported_separately() {
    let server = MockServer::start().await;
    let (job, record) = greenhouse_job();
    let id = record.posting.provenance.source_record_id.clone().unwrap();
    serve(
        &server,
        &format!("/v1/boards/stripe/jobs/{id}"),
        ResponseTemplate::new(200).set_body_json(&job),
    )
    .await;
    // A closed job's hosted page redirects to the board with error=true.
    serve(
        &server,
        &format!("/stripe/jobs/{id}"),
        ResponseTemplate::new(302)
            .insert_header("location", format!("{}/stripe?error=true", server.uri())),
    )
    .await;
    serve(
        &server,
        "/stripe",
        ResponseTemplate::new(200).set_body_string("board"),
    )
    .await;
    let o = verifier(&server).observe(&record).await.unwrap();
    found(&o);
    assert_eq!(o.application.status, ApplicationStatus::Closed);
    assert!(
        o.application
            .detail
            .as_deref()
            .unwrap()
            .contains("error=true")
    );
}

#[tokio::test]
async fn greenhouse_server_errors_and_garbage_are_errors_not_closures() {
    let server = MockServer::start().await;
    let (_, record) = greenhouse_job();
    let id = record.posting.provenance.source_record_id.clone().unwrap();
    serve(
        &server,
        &format!("/v1/boards/stripe/jobs/{id}"),
        ResponseTemplate::new(503),
    )
    .await;
    let error = verifier(&server).observe(&record).await.unwrap_err();
    assert!(
        matches!(
            error,
            ObserveError::Unavailable {
                status: Some(503),
                ..
            }
        ),
        "{error:?}"
    );

    let server = MockServer::start().await;
    serve(
        &server,
        &format!("/v1/boards/stripe/jobs/{id}"),
        ResponseTemplate::new(200).set_body_string("<html>maintenance</html>"),
    )
    .await;
    let error = verifier(&server).observe(&record).await.unwrap_err();
    assert!(matches!(error, ObserveError::Malformed { .. }), "{error:?}");
}

#[tokio::test]
async fn timeouts_are_typed() {
    let server = MockServer::start().await;
    let (job, record) = greenhouse_job();
    let id = record.posting.provenance.source_record_id.clone().unwrap();
    serve(
        &server,
        &format!("/v1/boards/stripe/jobs/{id}"),
        ResponseTemplate::new(200)
            .set_body_json(&job)
            .set_delay(Duration::from_secs(3)),
    )
    .await;
    let slow = HttpVerifier::with_hosts(
        http(Duration::from_millis(200)),
        VerifierHosts::all_at(&server.uri()),
    );
    let error = slow.observe(&record).await.unwrap_err();
    assert!(matches!(error, ObserveError::Timeout { .. }), "{error:?}");
}

#[tokio::test]
async fn lever_active_closed_and_broken_application() {
    let server = MockServer::start().await;
    let (job, record) = lever_job();
    let id = record.posting.provenance.source_record_id.clone().unwrap();
    serve(
        &server,
        &format!("/v0/postings/spotify/{id}"),
        ResponseTemplate::new(200).set_body_json(&job),
    )
    .await;
    serve(
        &server,
        &format!("/spotify/{id}/apply"),
        ResponseTemplate::new(200).set_body_string("<form></form>"),
    )
    .await;
    let o = verifier(&server).observe(&record).await.unwrap();
    assert_eq!(o.method, VerificationMethod::LeverPostingApi);
    assert_eq!(found(&o).snapshot(), record.posting.snapshot());
    assert_eq!(o.application.status, ApplicationStatus::Active);
    assert!(o.application.url.as_deref().unwrap().ends_with("/apply"));

    // The posting is live but its application page is gone.
    let server = MockServer::start().await;
    serve(
        &server,
        &format!("/v0/postings/spotify/{id}"),
        ResponseTemplate::new(200).set_body_json(&job),
    )
    .await;
    serve(
        &server,
        &format!("/spotify/{id}/apply"),
        ResponseTemplate::new(404),
    )
    .await;
    let o = verifier(&server).observe(&record).await.unwrap();
    found(&o);
    assert_eq!(o.application.status, ApplicationStatus::Closed);

    let server = MockServer::start().await;
    serve(
        &server,
        &format!("/v0/postings/spotify/{id}"),
        ResponseTemplate::new(404).set_body_string(r#"{"ok":false,"error":"Document not found"}"#),
    )
    .await;
    let o = verifier(&server).observe(&record).await.unwrap();
    assert!(matches!(o.listing, ObservedListing::NotFound { .. }));
}

#[tokio::test]
async fn ashby_reads_the_board_once_and_trusts_its_published_application_url() {
    let server = MockServer::start().await;
    let record = ashby_job();
    serve(
        &server,
        "/posting-api/job-board/linear",
        ResponseTemplate::new(200).set_body_bytes(fixture("ashby", "linear.json")),
    )
    .await;
    let o = verifier(&server).observe(&record).await.unwrap();
    assert_eq!(o.method, VerificationMethod::AshbyBoardApi);
    assert_eq!(found(&o).snapshot(), record.posting.snapshot());
    assert_eq!(o.application.status, ApplicationStatus::Active);
    assert_eq!(o.application.basis, ApplicationBasis::Published);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);

    // Gone from the complete board: closed.
    let server = MockServer::start().await;
    serve(
        &server,
        "/posting-api/job-board/linear",
        ResponseTemplate::new(200).set_body_string(r#"{"jobs":[]}"#),
    )
    .await;
    let o = verifier(&server).observe(&record).await.unwrap();
    assert!(matches!(
        o.listing,
        ObservedListing::MissingFromCompleteListing { .. }
    ));
}

#[tokio::test]
async fn yc_job_page_live_removed_and_redirected() {
    let server = MockServer::start().await;
    let record = yc_job();
    let page_path = url::Url::parse(record.posting.url.as_str())
        .unwrap()
        .path()
        .to_owned();
    serve(
        &server,
        &page_path,
        ResponseTemplate::new(200).set_body_bytes(fixture("yc", "posthog_job_104329.html")),
    )
    .await;
    serve(
        &server,
        "/application",
        ResponseTemplate::new(200).set_body_string("sign up"),
    )
    .await;
    let o = verifier(&server).observe(&record).await.unwrap();
    assert_eq!(o.method, VerificationMethod::YcJobPage);
    let posting = found(&o);
    assert_eq!(posting.id(), record.id);
    assert_eq!(
        posting.work_authorization,
        record.posting.work_authorization
    );
    assert_eq!(o.application.status, ApplicationStatus::Active);
    assert_eq!(o.chain[0].kind, LinkKind::PlatformPage);

    let server = MockServer::start().await;
    serve(&server, &page_path, ResponseTemplate::new(404)).await;
    let o = verifier(&server).observe(&record).await.unwrap();
    assert!(matches!(o.listing, ObservedListing::NotFound { .. }));

    // A removed job's URL showing the company's job list.
    let server = MockServer::start().await;
    serve(
        &server,
        &page_path,
        ResponseTemplate::new(200).set_body_bytes(fixture("yc", "posthog_jobs.html")),
    )
    .await;
    let o = verifier(&server).observe(&record).await.unwrap();
    assert!(matches!(o.listing, ObservedListing::NotFound { .. }));
}

#[tokio::test]
async fn unsupported_sources_say_so() {
    let server = MockServer::start().await;
    let (_, mut record) = greenhouse_job();
    record.posting.provenance.source = SourceKey::new("workday", "acme").unwrap();
    let error = verifier(&server).observe(&record).await.unwrap_err();
    assert!(matches!(error, ObserveError::NotSupported { ref kind } if kind == "workday"));
    assert!(server.received_requests().await.unwrap().is_empty());
}
