//! Checks the Greenhouse adapter against real boards.
//!
//! Ignored by default so `cargo test` never depends on the network. Run with:
//!
//! ```bash
//! cargo test -p jobhunt-sources --test greenhouse_live -- --ignored --nocapture
//! JOBHUNT_LIVE_GREENHOUSE_BOARDS=discord,gitlab cargo test -p jobhunt-sources --test greenhouse_live -- --ignored --nocapture
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod live_common;

#[tokio::test]
#[ignore = "hits the live Greenhouse API"]
async fn live_boards_convert_cleanly() {
    let boards = live_common::names(
        "JOBHUNT_LIVE_GREENHOUSE_BOARDS",
        &["anthropic", "stripe", "figma", "airbnb", "discord"],
    );
    live_common::check("greenhouse", &boards, "https://").await;
}
