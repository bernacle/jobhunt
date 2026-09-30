//! Email notifications about strong new recommendations.
//!
//! An email should mean "this is probably worth interrupting you for", so
//! precision beats recall:
//!
//! * the candidates are the product's own ranking
//!   ([`jobhunt_app::App::notification_candidates`]): strong fits only,
//!   recommended outright (verified recently at an authoritative source,
//!   eligible or conditionally eligible), never acted on, never already
//!   shown in the app or put aside, never notified before;
//! * nothing found means no email (no "nothing new" messages, no digests
//!   of maybes);
//! * at most [`crate::config::NotifySettings::max_items`] per email, and at
//!   most one email per account every few hours (`immediate`) or a day
//!   (`daily`).
//!
//! Delivery is an outbox in Postgres (`jobhunt_storage::postgres::notify`):
//! the rendered message and the opportunities it covers are committed
//! first, then sent with the delivery id as the provider's idempotency key,
//! then marked sent (moving the account's notification cursor). A provider
//! failure leaves it pending, retried with backoff by later runs; a retry
//! after a crash sends the same stored message with the same key, which the
//! provider deduplicates. A delivery still unsent after
//! [`STALE_AFTER_HOURS`] is abandoned rather than sent late (and possibly
//! twice, once the provider has forgotten its key).
//!
//! Workers coordinate through leases on accounts and deliveries (`FOR
//! UPDATE SKIP LOCKED`), like discovery and verification: two workers never
//! compose for the same account at once, and an opportunity can be put in a
//! person's notification only once (a primary key), whatever happens.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use jobhunt_app::feed::NotificationCandidate;
use jobhunt_app::{App, AppError, DiscoveryMode};
use jobhunt_storage::postgres::{
    Cadence, Delivery, DeliveryKind, NewDelivery, NotifiedItem, NotifyAccount, PgStore, WorkerKind,
    new_delivery_id,
};
use serde::Serialize;
use tracing::Instrument;
use url::Url;

use crate::config::CloudConfig;
use crate::email::{EmailMessage, EmailSender, SendError};

/// A pending delivery older than this is abandoned, not sent: the news is
/// stale, and the provider (Resend keeps idempotency keys 24 hours) may no
/// longer recognize a repeat.
pub const STALE_AFTER_HOURS: i64 = 20;
/// Attempts before a delivery is given up as failed.
pub const MAX_ATTEMPTS: u32 = 6;
const LEASE: Duration = Duration::from_secs(10 * 60);
const ABANDON_RUNS_AFTER: Duration = Duration::from_secs(3600);

/// What a notification run did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct NotifySummary {
    /// Accounts with notifications on that were examined.
    pub accounts: usize,
    /// Of those, skipped because they were emailed recently.
    pub recently_notified: usize,
    /// Of those, with nothing worth an email.
    pub nothing_new: usize,
    /// Emails composed (written to the outbox).
    pub composed: usize,
    /// Opportunities in them.
    pub opportunities: usize,
    /// Emails the provider accepted.
    pub sent: usize,
    /// Attempts that failed and will be retried.
    pub retrying: usize,
    /// Emails given up after repeated or permanent failures.
    pub failed: usize,
    /// Emails too old to send safely.
    pub abandoned: usize,
}

/// What happened to one delivery attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Sent,
    Retrying,
    Failed,
    Abandoned,
    /// Another worker finished it first.
    Lost,
}

fn backoff(attempts: u32) -> chrono::Duration {
    // 5 minutes, 10, 20, 40, 80, capped at 3 hours.
    let minutes = 5i64.saturating_mul(1i64 << attempts.min(6));
    chrono::Duration::minutes(minutes.min(180))
}

/// Sends one claimed delivery and records the outcome.
pub async fn deliver(
    store: &PgStore,
    sender: &dyn EmailSender,
    owner: &str,
    delivery: &Delivery,
    now: DateTime<Utc>,
) -> Result<Outcome, AppError> {
    if now - delivery.created_at > chrono::Duration::hours(STALE_AFTER_HOURS) {
        store.delivery_abandoned(owner, &delivery.id).await?;
        tracing::warn!(delivery = %delivery.id, "notification abandoned: too old to send safely");
        return Ok(Outcome::Abandoned);
    }
    let message: EmailMessage = match serde_json::from_slice(&delivery.message) {
        Ok(m) => m,
        Err(e) => {
            store
                .delivery_failed(
                    owner,
                    &delivery.id,
                    &format!("unreadable message: {e}"),
                    None,
                )
                .await?;
            return Ok(Outcome::Failed);
        }
    };
    match sender.send(&message, &delivery.id).await {
        Ok(receipt) => {
            let recorded = store
                .delivery_sent(
                    owner,
                    &delivery.id,
                    sender.name(),
                    receipt.message_id.as_deref(),
                    Utc::now(),
                )
                .await?;
            tracing::info!(delivery = %delivery.id, kind = delivery.kind.as_str(), "notification sent");
            Ok(if recorded {
                Outcome::Sent
            } else {
                Outcome::Lost
            })
        }
        Err(SendError::Retryable(error)) if delivery.attempts + 1 < MAX_ATTEMPTS => {
            let retry_at = now + backoff(delivery.attempts);
            store
                .delivery_failed(owner, &delivery.id, &error, Some(retry_at))
                .await?;
            tracing::warn!(delivery = %delivery.id, %error, %retry_at, "notification not sent; will retry");
            Ok(Outcome::Retrying)
        }
        Err(e) => {
            store
                .delivery_failed(owner, &delivery.id, &e.to_string(), None)
                .await?;
            tracing::warn!(delivery = %delivery.id, error = %e, "notification failed");
            Ok(Outcome::Failed)
        }
    }
}

/// Sends every due delivery (of one account, or all).
pub async fn deliver_due(
    store: &PgStore,
    sender: &dyn EmailSender,
    owner: &str,
    only: Option<&jobhunt_storage::postgres::UserId>,
    summary: &mut NotifySummary,
) -> Result<(), AppError> {
    loop {
        let due = store
            .claim_deliveries(owner, 20, LEASE, Utc::now(), only)
            .await?;
        if due.is_empty() {
            return Ok(());
        }
        for d in &due {
            match deliver(store, sender, owner, d, Utc::now()).await? {
                Outcome::Sent => summary.sent += 1,
                Outcome::Retrying => summary.retrying += 1,
                Outcome::Failed => summary.failed += 1,
                Outcome::Abandoned => summary.abandoned += 1,
                Outcome::Lost => {}
            }
        }
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn link(web: &Url, path: &str) -> String {
    format!("{}{path}", web.as_str().trim_end_matches('/'))
}

/// Where and how: "Remote (Americas)", "Lisbon, Portugal · hybrid".
fn place(c: &NotificationCandidate) -> Option<String> {
    let item = &c.item.item;
    let mut parts: Vec<String> = item.locations.iter().take(2).cloned().collect();
    if let Some(w) = &item.workplace
        && !parts.iter().any(|p| p.to_lowercase().contains(w.as_str()))
    {
        parts.push(w.clone());
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// The one caveat worth reading first: eligibility conditions, then what
/// the brief says to consider.
fn caveat(c: &NotificationCandidate) -> Option<String> {
    use jobhunt_app::views::EligibilityStatus;
    let item = &c.item.item;
    match item.eligibility.status {
        EligibilityStatus::Conditional | EligibilityStatus::Uncertain => {
            Some(item.eligibility.headline.clone())
        }
        _ => item.consider.first().cloned(),
    }
}

/// The email about new strong recommendations.
pub fn render_recommendations(
    from: &str,
    to: &str,
    web: &Url,
    candidates: &[NotificationCandidate],
) -> EmailMessage {
    let subject = match candidates {
        [one] => format!(
            "A strong new match: {} at {}",
            one.item.item.title, one.item.item.company
        ),
        many => format!("{} new jobs worth your time", many.len()),
    };
    let settings = link(web, "/settings");
    let intro = match candidates.len() {
        1 => "One new opportunity looks worth your time.".to_owned(),
        n => format!("{n} new opportunities look worth your time."),
    };
    let mut text = format!("{intro}\n");
    let mut html = format!(
        "<div style=\"font-family:system-ui,sans-serif;max-width:560px;color:#1c1b19;line-height:1.5\">\
         <p style=\"font-size:16px\">{}</p>",
        escape(&intro)
    );
    for c in candidates {
        let item = &c.item.item;
        let url = link(web, &format!("/opportunities/{}", item.id));
        let pay = (item.compensation.status == "published")
            .then(|| item.compensation.ranges.first().cloned())
            .flatten();
        let facts: Vec<String> = place(c).into_iter().chain(pay).collect();
        let why: Vec<String> = item.why.iter().take(2).cloned().collect();
        text.push_str(&format!("\n{} — {}\n", item.title, item.company));
        if !facts.is_empty() {
            text.push_str(&format!("{}\n", facts.join(" · ")));
        }
        if !why.is_empty() {
            text.push_str(&format!("Why: {}\n", why.join("; ")));
        }
        if let Some(caveat) = caveat(c) {
            text.push_str(&format!("Consider: {caveat}\n"));
        }
        text.push_str(&format!("{url}\n"));

        html.push_str(&format!(
            "<div style=\"border-top:1px solid #e4e0d8;padding:14px 0\">\
             <p style=\"margin:0;font-size:17px\"><a href=\"{}\" style=\"color:#1c1b19\">{}</a></p>\
             <p style=\"margin:2px 0 8px;color:#5f5a52;font-family:system-ui,sans-serif;font-size:14px\">{}</p>",
            escape(&url),
            escape(&format!("{} — {}", item.title, item.company)),
            escape(&facts.join(" · ")),
        ));
        if !why.is_empty() {
            html.push_str(&format!(
                "<p style=\"margin:0 0 4px;font-family:system-ui,sans-serif;font-size:14px\">\
                 <strong>Why:</strong> {}</p>",
                escape(&why.join("; "))
            ));
        }
        if let Some(caveat) = caveat(c) {
            html.push_str(&format!(
                "<p style=\"margin:0;font-family:system-ui,sans-serif;font-size:14px;color:#5f5a52\">\
                 <strong>Consider:</strong> {}</p>",
                escape(&caveat)
            ));
        }
        html.push_str("</div>");
    }
    let footer = format!(
        "You get this because email notifications are on in Narrow. Turn them off or change \
         how often: {settings}"
    );
    text.push_str(&format!("\n—\n{footer}\n"));
    html.push_str(&format!(
        "<p style=\"border-top:1px solid #e4e0d8;padding-top:12px;color:#5f5a52;\
         font-family:system-ui,sans-serif;font-size:12px\">You get this because email \
         notifications are on in Narrow. <a href=\"{}\">Turn them off or change how often</a>.</p>\
         </div>",
        escape(&settings)
    ));
    EmailMessage {
        from: from.to_owned(),
        to: to.to_owned(),
        subject,
        text,
        html,
        headers: vec![("List-Unsubscribe".into(), format!("<{settings}>"))],
    }
}

/// The email that confirms an address.
pub fn render_confirmation(from: &str, to: &str, web: &Url, token: &str) -> EmailMessage {
    let url = link(web, &format!("/settings/confirm?token={token}"));
    let text = format!(
        "Confirm this address to get Narrow notifications about strong new job matches:\n\n\
         {url}\n\nThe link works for 48 hours, while you are signed in. If you did not ask for \
         this, ignore it: nothing will be sent.\n"
    );
    let html = format!(
        "<div style=\"font-family:system-ui,sans-serif;max-width:520px;color:#1c1b19;line-height:1.5\">\
         <p>Confirm this address to get Narrow notifications about strong new job matches.</p>\
         <p><a href=\"{0}\">Confirm the address</a></p>\
         <p style=\"color:#5f5a52;font-size:13px\">The link works for 48 hours, while you are \
         signed in. If you did not ask for this, ignore it: nothing will be sent.</p></div>",
        escape(&url)
    );
    EmailMessage {
        from: from.to_owned(),
        to: to.to_owned(),
        subject: "Confirm your email for Narrow notifications".into(),
        text,
        html,
        headers: Vec::new(),
    }
}

/// Composes (at most) one recommendations email for an account and sends
/// it. Returns how many opportunities it covered (0: nothing sent).
async fn compose(
    store: &PgStore,
    config: &CloudConfig,
    sender: &dyn EmailSender,
    owner: &str,
    account: &NotifyAccount,
    summary: &mut NotifySummary,
) -> Result<usize, AppError> {
    let now = Utc::now();
    let interval = match account.cadence {
        Cadence::Immediate => config.notify.min_interval,
        Cadence::Daily => Duration::from_secs(24 * 3600),
    };
    if let Some(last) = account.last_sent_at
        && (now - last).to_std().unwrap_or_default() < interval
    {
        summary.recently_notified += 1;
        return Ok(0);
    }
    let (Some(web), Some(email)) = (&config.web_url, &config.email) else {
        return Err(AppError::Config(
            "notifications need JOBHUNT_WEB_URL and an email provider".into(),
        ));
    };
    // The shortlist is reviewed here too: after discovery, in the
    // background, so Today's requests find the reviews stored.
    let app = App::from_parts(
        Arc::clone(&config.app),
        Arc::new(store.for_user(account.user.clone())),
        DiscoveryMode::Background,
    )
    .with_fit_review(crate::config::fit_review(config));
    let already = store.notified_opportunities_all(&account.user).await?;
    let candidates = app
        .notification_candidates(&already, config.notify.max_items, now)
        .await?;
    if candidates.is_empty() {
        summary.nothing_new += 1;
        return Ok(0);
    }
    let message = render_recommendations(
        crate::email::from_address(email),
        &account.email,
        web,
        &candidates,
    );
    let delivery = NewDelivery {
        id: new_delivery_id(),
        user: account.user.clone(),
        kind: DeliveryKind::Recommendations,
        message: serde_json::to_vec(&message)
            .map_err(|e| AppError::Config(format!("encoding an email: {e}")))?,
        items: candidates
            .iter()
            .map(|c| NotifiedItem {
                opportunity: c.item.item.id.clone(),
                tier: serde_json::to_value(c.item.item.tier)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "strong_fit".into()),
                job: c.job.clone(),
                content_version: c.content_version.clone(),
                ranking_version: c.ranking_version.clone(),
            })
            .collect(),
    };
    if !store.enqueue_delivery(&delivery, now).await? {
        // Another worker notified about one of them meanwhile.
        return Ok(0);
    }
    summary.composed += 1;
    summary.opportunities += candidates.len();
    deliver_due(store, sender, owner, Some(&account.user), summary).await?;
    Ok(candidates.len())
}

/// `narrow worker notify`: sends due retries, then composes and sends at
/// most one email per account with notifications on.
pub async fn notify(
    store: &PgStore,
    config: &CloudConfig,
    sender: &dyn EmailSender,
) -> Result<NotifySummary, AppError> {
    let owner = format!("notification:{}", config.instance);
    let run = store
        .start_worker_run(
            WorkerKind::Notification,
            &owner,
            ABANDON_RUNS_AFTER,
            Utc::now(),
        )
        .await?;
    let span = tracing::info_span!("worker", kind = "notification", run = run.id, worker = %owner);
    let result = notify_inner(store, config, sender, &owner)
        .instrument(span)
        .await;
    let (summary_json, error) = match &result {
        Ok(summary) => (serde_json::to_value(summary).unwrap_or_default(), None),
        Err(e) => (serde_json::json!({}), Some(e.public_message())),
    };
    if let Err(e) = store
        .finish_worker_run(run, &summary_json, error.as_deref(), Utc::now())
        .await
    {
        tracing::warn!(error = %e, "could not record the end of the run");
    }
    result
}

async fn notify_inner(
    store: &PgStore,
    config: &CloudConfig,
    sender: &dyn EmailSender,
    owner: &str,
) -> Result<NotifySummary, AppError> {
    let mut summary = NotifySummary::default();
    // Retries first: an email that failed earlier goes before new ones.
    deliver_due(store, sender, owner, None, &mut summary).await?;
    let mut claimed = Vec::new();
    let result = async {
        loop {
            let accounts = store
                .claim_notification_accounts(owner, config.notify.batch, LEASE, Utc::now())
                .await?;
            if accounts.is_empty() {
                break;
            }
            for account in accounts {
                summary.accounts += 1;
                // One account's failure does not stop the others.
                if let Err(e) = compose(store, config, sender, owner, &account, &mut summary).await
                {
                    tracing::warn!(user = %account.user, error = %e.public_message(), "could not notify an account");
                }
                claimed.push(account.user);
            }
        }
        Ok::<(), AppError>(())
    }
    .await;
    // Leases are held for the whole run so an account is looked at once.
    for user in &claimed {
        if let Err(e) = store.release_notification_account(owner, user).await {
            tracing::warn!(error = %e, "could not release an account");
        }
    }
    result?;
    tracing::info!(
        accounts = summary.accounts,
        composed = summary.composed,
        sent = summary.sent,
        retrying = summary.retrying,
        failed = summary.failed,
        "notification run finished"
    );
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_is_capped() {
        assert_eq!(backoff(0), chrono::Duration::minutes(5));
        assert_eq!(backoff(1), chrono::Duration::minutes(10));
        assert_eq!(backoff(10), chrono::Duration::minutes(180));
    }

    #[test]
    fn html_is_escaped() {
        assert_eq!(
            escape("<a href=\"x\">&'"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
        );
    }

    #[test]
    fn confirmation_links_to_the_web_app() {
        let web = Url::parse("https://app.jobhunt.test/").unwrap();
        let m = render_confirmation("J <n@j.test>", "ana@example.com", &web, "abc");
        assert!(
            m.text
                .contains("https://app.jobhunt.test/settings/confirm?token=abc")
        );
        assert_eq!(m.to, "ana@example.com");
    }
}
