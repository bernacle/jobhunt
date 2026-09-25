//! Checks the Lever adapter against real sites.
//!
//! Ignored by default so `cargo test` never depends on the network. Run with:
//!
//! ```bash
//! cargo test -p jobhunt-sources --test lever_live -- --ignored --nocapture
//! JOBHUNT_LIVE_LEVER_SITES=spotify,zoox cargo test -p jobhunt-sources --test lever_live -- --ignored --nocapture
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod live_common;

#[tokio::test]
#[ignore = "hits the live Lever API"]
async fn live_sites_convert_cleanly() {
    let sites = live_common::names(
        "JOBHUNT_LIVE_LEVER_SITES",
        &["spotify", "palantir", "zoox", "outreach", "matchgroup"],
    );
    live_common::check("lever", &sites, "https://jobs.lever.co/").await;
}
