//! Checks the adapter against the real Ashby API.
//!
//! Ignored by default so `cargo test` never depends on the network. Run with:
//!
//! ```bash
//! cargo test -p jobhunt-sources --test ashby_live -- --ignored
//! ```
//!
//! Optionally set `JOBHUNT_LIVE_ASHBY_BOARD` to check a different board.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use jobhunt_core::{FetchRequest, Fetched, Source, SourceKey};
use jobhunt_sources::{AshbyBoard, AshbySource, HttpClient, HttpSettings};

#[tokio::test]
#[ignore = "hits the live Ashby API"]
async fn live_board_converts_cleanly() {
    let board = std::env::var("JOBHUNT_LIVE_ASHBY_BOARD").unwrap_or_else(|_| "linear".into());
    let source = AshbySource::new(
        SourceKey::new("ashby", &board).unwrap(),
        AshbyBoard {
            board: board.clone(),
            company: None,
        },
        HttpClient::new(HttpSettings::default()).unwrap(),
    )
    .unwrap();

    let Fetched::Batch(batch) = source.fetch(&FetchRequest::default()).await.unwrap() else {
        panic!("unconditional fetch answered not modified");
    };
    assert!(batch.complete);
    println!(
        "ashby:{board}: {} received, {} converted, {} rejected, {} skipped",
        batch.received(),
        batch.records.len(),
        batch.rejected.len(),
        batch.skipped
    );
    for error in &batch.rejected {
        println!("  rejected: {error}");
    }

    assert!(!batch.records.is_empty(), "board {board} returned no jobs");
    assert!(
        batch.rejected.len() * 20 <= batch.received(),
        "more than 5% of postings were rejected; the payload format may have changed"
    );
    for posting in &batch.records {
        posting.validate().unwrap();
        assert!(
            posting
                .url
                .as_str()
                .starts_with("https://jobs.ashbyhq.com/")
        );
        assert!(posting.provenance.source_record_id.is_some());
    }
}
