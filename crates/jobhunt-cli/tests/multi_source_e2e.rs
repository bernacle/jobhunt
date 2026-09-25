//! Multi-source discovery end to end: every adapter family served from one
//! local mock server (saved real responses), one discovery pipeline, one
//! SQLite file. Later runs mutate the sources to prove the lifecycle rules
//! (NEW / UNCHANGED / UPDATED / CLOSED / REOPENED), the closing safeguards,
//! conditional fetches, and cross-source grouping.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use jobhunt_core::SourceKey;
use jobhunt_jobs::{
    Discovery, DiscoveryReport, JobEventKind, JobId, JobQuery, JobRepository, JobSource, JobStatus,
    ScanKind,
};
use jobhunt_sources::yc::parse_listing;
use jobhunt_sources::{
    AshbyBoard, AshbySource, GreenhouseBoard, GreenhouseSource, HttpClient, HttpSettings,
    LeverRegion, LeverSite, LeverSource, YcCompany, YcSource,
};
use jobhunt_storage::SqliteJobStore;
use wiremock::matchers::{header, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(family: &str, name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/../jobhunt-sources/tests/fixtures/{family}/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// What the mock sources serve in one run.
#[derive(Clone)]
struct World {
    ashby_linear: Option<String>,
    greenhouse_figma: String,
    greenhouse_mirror: String,
    lever_spotify: String,
    lever_etag: Option<&'static str>,
    yc_failing_details: Vec<&'static str>,
}

impl World {
    fn initial() -> Self {
        Self {
            ashby_linear: Some(fixture("ashby", "linear.json")),
            greenhouse_figma: fixture("greenhouse", "figma.json"),
            greenhouse_mirror: fixture("greenhouse", "figma.json"),
            lever_spotify: fixture("lever", "spotify.json"),
            lever_etag: Some("W/\"spotify-v1\""),
            yc_failing_details: Vec::new(),
        }
    }

    async fn serve(&self, server: &MockServer) {
        server.reset().await;
        let ok = |body: &str| ResponseTemplate::new(200).set_body_string(body.to_owned());

        let ashby = match &self.ashby_linear {
            Some(body) => ok(body),
            None => ResponseTemplate::new(503),
        };
        Mock::given(path("/posting-api/job-board/linear"))
            .respond_with(ashby)
            .mount(server)
            .await;
        Mock::given(path("/v1/boards/figma/jobs"))
            .respond_with(ok(&self.greenhouse_figma))
            .mount(server)
            .await;
        Mock::given(path("/v1/boards/figma-mirror/jobs"))
            .respond_with(ok(&self.greenhouse_mirror))
            .mount(server)
            .await;

        // Lever supports conditional requests: an unchanged listing is a 304.
        if let Some(etag) = self.lever_etag {
            Mock::given(path("/v0/postings/spotify"))
                .and(header("if-none-match", etag))
                .respond_with(ResponseTemplate::new(304))
                .with_priority(1)
                .mount(server)
                .await;
        }
        let mut lever = ok(&self.lever_spotify);
        if let Some(etag) = self.lever_etag {
            lever = lever.insert_header("etag", etag);
        }
        Mock::given(path("/v0/postings/spotify"))
            .respond_with(lever)
            .mount(server)
            .await;

        let listing = fixture("yc", "pine-park-health_jobs.html");
        let company = YcCompany {
            slug: "pine-park-health".into(),
            company: None,
        };
        for stub in parse_listing(listing.as_bytes(), &company).unwrap().jobs {
            let response = if self.yc_failing_details.contains(&stub.id.as_str()) {
                ResponseTemplate::new(500)
            } else {
                ok(&fixture(
                    "yc",
                    &format!("pine-park-health_job_{}.html", stub.id),
                ))
            };
            Mock::given(path(stub.path))
                .respond_with(response)
                .mount(server)
                .await;
        }
        Mock::given(path("/companies/pine-park-health/jobs"))
            .respond_with(ok(&listing))
            .mount(server)
            .await;
    }
}

fn sources(server: &MockServer) -> Vec<Box<JobSource>> {
    let http = HttpClient::new(HttpSettings {
        max_retries: 0,
        timeout: Duration::from_secs(5),
        ..HttpSettings::default()
    })
    .unwrap();
    let key = |s: &str| s.parse::<SourceKey>().unwrap();
    let greenhouse = |board: &str| -> Box<JobSource> {
        Box::new(
            GreenhouseSource::with_api_base(
                key(&format!("greenhouse:{board}")),
                GreenhouseBoard {
                    board: board.into(),
                    company: None,
                },
                http.clone(),
                &server.uri(),
            )
            .unwrap(),
        )
    };
    vec![
        Box::new(
            AshbySource::with_api_base(
                key("ashby:linear"),
                AshbyBoard {
                    board: "linear".into(),
                    company: Some("Linear".into()),
                },
                http.clone(),
                &server.uri(),
            )
            .unwrap(),
        ),
        greenhouse("figma"),
        greenhouse("figma-mirror"),
        Box::new(
            LeverSource::with_api_base(
                key("lever:spotify"),
                LeverSite {
                    site: "spotify".into(),
                    company: Some("Spotify".into()),
                    region: LeverRegion::Global,
                },
                http.clone(),
                &server.uri(),
            )
            .unwrap(),
        ),
        Box::new(
            YcSource::with_base(
                key("yc:pine-park-health"),
                YcCompany {
                    slug: "pine-park-health".into(),
                    company: None,
                },
                http,
                &server.uri(),
            )
            .unwrap(),
        ),
    ]
}

fn lifecycle(report: &DiscoveryReport, source: &str) -> (usize, usize, usize, usize, usize) {
    let stats = report
        .sources
        .iter()
        .find(|s| s.source.to_string() == source)
        .unwrap()
        .result
        .as_ref()
        .unwrap_or_else(|e| panic!("{source} failed: {e}"));
    let c = stats.counts;
    (c.inserted, c.updated, c.unchanged, c.reopened, c.closed)
}

fn kind(report: &DiscoveryReport, source: &str) -> ScanKind {
    report
        .sources
        .iter()
        .find(|s| s.source.to_string() == source)
        .unwrap()
        .result
        .as_ref()
        .unwrap()
        .kind
}

fn id(source: &str, record: &str) -> JobId {
    JobId::derive_from_record_id(&source.parse().unwrap(), record)
}

async fn status(store: &SqliteJobStore, job: JobId) -> JobStatus {
    store.get(job).await.unwrap().unwrap().status
}

const SPOTIFY_REMOVED: &str = "03437e2a-2d5e-4593-9e97-11271014932e";
const FIGMA_EDITED: &str = "5426468004";

#[tokio::test]
async fn multi_source_discovery_tracks_lifecycle_and_duplicates() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteJobStore::open(&dir.path().join("jobhunt.db"))
        .await
        .unwrap();
    let sources = sources(&server);
    let run = || async {
        Discovery::new(&store)
            .with_concurrency(3)
            .run(&sources)
            .await
            .unwrap()
    };

    // Run 1: everything is NEW. The same three Figma jobs are visible
    // through two Greenhouse boards and are grouped, not merged.
    let world = World::initial();
    world.serve(&server).await;
    let first = run().await;
    assert_eq!(first.succeeded(), 5);
    assert_eq!(lifecycle(&first, "ashby:linear"), (3, 0, 0, 0, 0));
    assert_eq!(lifecycle(&first, "greenhouse:figma"), (3, 0, 0, 0, 0));
    assert_eq!(
        lifecycle(&first, "greenhouse:figma-mirror"),
        (3, 0, 0, 0, 0)
    );
    assert_eq!(lifecycle(&first, "lever:spotify"), (3, 0, 0, 0, 0));
    assert_eq!(lifecycle(&first, "yc:pine-park-health"), (2, 0, 0, 0, 0));
    assert_eq!(first.dedupe.multi_source_opportunities, 3);
    assert_eq!(store.count(&JobQuery::default()).await.unwrap(), 14);
    let distinct = JobQuery {
        distinct_opportunities: true,
        status: Some(JobStatus::Open),
        ..JobQuery::default()
    };
    assert_eq!(store.count(&distinct).await.unwrap(), 11);

    // Source records stay individually inspectable after grouping.
    let figma = store
        .get(id("greenhouse:figma", FIGMA_EDITED))
        .await
        .unwrap()
        .unwrap();
    let group = store
        .opportunity_records(figma.opportunity_id)
        .await
        .unwrap();
    let group_sources: Vec<String> = group
        .iter()
        .map(|r| r.posting.provenance.source.to_string())
        .collect();
    assert_eq!(group.len(), 2);
    assert!(group_sources.contains(&"greenhouse:figma".to_owned()));
    assert!(group_sources.contains(&"greenhouse:figma-mirror".to_owned()));
    // Same company and similar titles alone never group: Figma has three
    // "Account Executive, Enterprise …" jobs, each its own opportunity.
    let opportunities: std::collections::HashSet<_> = store
        .search(&JobQuery::default().with_text("figma"))
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.opportunity_id)
        .collect();
    assert_eq!(opportunities.len(), 3);

    // Run 2: nothing changed. Lever confirms with a 304; the rest re-read.
    let second = run().await;
    for source in [
        "ashby:linear",
        "greenhouse:figma",
        "greenhouse:figma-mirror",
        "yc:pine-park-health",
    ] {
        let (new, updated, _, reopened, closed) = lifecycle(&second, source);
        assert_eq!((new, updated, reopened, closed), (0, 0, 0, 0), "{source}");
    }
    assert_eq!(kind(&second, "lever:spotify"), ScanKind::NotModified);
    assert_eq!(lifecycle(&second, "lever:spotify"), (0, 0, 3, 0, 0));
    assert_eq!(second.totals().unchanged, 14);
    assert_eq!(second.dedupe.reassigned, 0);

    // Run 3: every kind of change at once.
    let mut changed = world.clone();
    // Ashby is down: its jobs must stay open.
    changed.ashby_linear = None;
    // A Figma job is retitled on one board.
    changed.greenhouse_figma = world.greenhouse_figma.replace(
        "\"Account Executive, Enterprise\"",
        "\"Senior Account Executive, Enterprise\"",
    );
    // The mirror board returns a truncated listing (1 of its 3 jobs, while
    // reporting 3): partial, so nothing may close there.
    let mut mirror: serde_json::Value = serde_json::from_str(&world.greenhouse_mirror).unwrap();
    mirror["jobs"].as_array_mut().unwrap().truncate(1);
    changed.greenhouse_mirror = mirror.to_string();
    // Spotify takes a posting down (a new ETag, so a full listing).
    let mut spotify: Vec<serde_json::Value> = serde_json::from_str(&world.lever_spotify).unwrap();
    spotify.retain(|p| p["id"] != SPOTIFY_REMOVED);
    changed.lever_spotify = serde_json::to_string(&spotify).unwrap();
    changed.lever_etag = Some("W/\"spotify-v2\"");
    // One YC detail page fails: that job is unreadable, not gone.
    changed.yc_failing_details = vec!["93347"];
    changed.serve(&server).await;

    let third = run().await;
    let failures: Vec<String> = third.failures().map(|(k, _)| k.to_string()).collect();
    assert_eq!(failures, ["ashby:linear"]);
    assert_eq!(lifecycle(&third, "greenhouse:figma"), (0, 1, 2, 0, 0));
    assert_eq!(kind(&third, "greenhouse:figma-mirror"), ScanKind::Partial);
    assert_eq!(
        lifecycle(&third, "greenhouse:figma-mirror"),
        (0, 0, 1, 0, 0)
    );
    assert_eq!(lifecycle(&third, "lever:spotify"), (0, 0, 2, 0, 1));
    assert_eq!(lifecycle(&third, "yc:pine-park-health"), (0, 0, 1, 0, 0));

    let linear = id("ashby:linear", "d3bc1ced-3ce4-4086-a050-555055dbb1ff");
    assert_eq!(
        status(&store, linear).await,
        JobStatus::Open,
        "failed scans close nothing"
    );
    let removed = id("lever:spotify", SPOTIFY_REMOVED);
    assert_eq!(status(&store, removed).await, JobStatus::Closed);
    let unreadable = id("yc:pine-park-health", "93347");
    assert_eq!(
        status(&store, unreadable).await,
        JobStatus::Open,
        "unreadable is not missing"
    );
    for mirrored in ["5579204004", "5426468004", "5783812004"] {
        assert_eq!(
            status(&store, id("greenhouse:figma-mirror", mirrored)).await,
            JobStatus::Open,
            "partial listings close nothing"
        );
    }

    let edited = id("greenhouse:figma", FIGMA_EDITED);
    let history = store.history(edited).await.unwrap();
    assert_eq!(history.last().unwrap().kind, JobEventKind::Updated);
    assert_eq!(history.last().unwrap().changed_fields, ["title"]);
    assert_eq!(
        history.last().unwrap().previous.as_ref().unwrap().title,
        "Account Executive, Enterprise"
    );
    // The retitled record still matches its mirror copy (the title only
    // gained a word), so the pair stays one opportunity.
    let record = store.get(edited).await.unwrap().unwrap();
    assert_eq!(
        store
            .opportunity_records(record.opportunity_id)
            .await
            .unwrap()
            .len(),
        2
    );

    // Run 4: the Spotify posting comes back and Ashby recovers.
    let mut recovered = changed.clone();
    recovered.ashby_linear = world.ashby_linear.clone();
    recovered.lever_spotify = world.lever_spotify.clone();
    recovered.lever_etag = Some("W/\"spotify-v3\"");
    recovered.yc_failing_details.clear();
    recovered.serve(&server).await;

    let fourth = run().await;
    assert_eq!(fourth.succeeded(), 5);
    assert_eq!(lifecycle(&fourth, "lever:spotify"), (0, 0, 2, 1, 0));
    assert_eq!(lifecycle(&fourth, "ashby:linear"), (0, 0, 3, 0, 0));
    assert_eq!(status(&store, removed).await, JobStatus::Open);
    let kinds: Vec<JobEventKind> = store
        .history(removed)
        .await
        .unwrap()
        .iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            JobEventKind::New,
            JobEventKind::Closed,
            JobEventKind::Reopened
        ]
    );
    // Nothing was ever duplicated or deleted.
    assert_eq!(store.count(&JobQuery::default()).await.unwrap(), 14);
    store.close().await;
}
