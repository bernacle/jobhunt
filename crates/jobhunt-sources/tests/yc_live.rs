//! Checks the YC adapter against real company pages.
//!
//! Ignored by default so `cargo test` never depends on the network. Run with:
//!
//! ```bash
//! cargo test -p jobhunt-sources --test yc_live -- --ignored --nocapture
//! JOBHUNT_LIVE_YC_COMPANIES=posthog cargo test -p jobhunt-sources --test yc_live -- --ignored --nocapture
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod live_common;

#[tokio::test]
#[ignore = "hits www.ycombinator.com"]
async fn live_companies_convert_cleanly() {
    let companies = live_common::names(
        "JOBHUNT_LIVE_YC_COMPANIES",
        &["posthog", "doordash", "pine-park-health"],
    );
    live_common::check("yc", &companies, "https://www.ycombinator.com/companies/").await;
}
