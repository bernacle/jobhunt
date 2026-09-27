//! The shortlist: the few opportunities worth the person's time right now.
//!
//! This is what `jobhunt find` and the MCP `search_jobs` tool answer:
//!
//! 1. refresh discovery when stored jobs are stale (or when asked);
//! 2. rank every open opportunity against the profile, preferences and
//!    learned taste (eligibility and verification gate first);
//! 3. verify the best candidates whose listings aren't verified recently
//!    enough, so the shortlist rests on what employers publish now;
//! 4. rank again, and keep the top few, with the funnel that led there.
//!
//! Ranking, gating, tiers and briefs are [`jobhunt_ranking::RankingService`]'s; verification
//! is the verification service's. Nothing here re-decides them.

use chrono::{DateTime, Utc};
use futures::StreamExt;
use jobhunt_eligibility::evaluate::trust;
use jobhunt_jobs::verification::{Standing, VerifyMode, cached};
use jobhunt_ranking::{Gate, RankQuery, RankReport, Ranking, SignalGroup, Tier};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::discover::{Refresh, RefreshMode, RefreshReason};
use crate::error::AppError;
use crate::resolve::{Opportunity, short_id};
use crate::verify::Checked;
use crate::views::{
    CompensationView, EligibilityBrief, FitTier, Recommendation, VerificationBrief, best_record,
    gate_note, locations,
};
use crate::{LocalApp, Progress};

/// The most results a shortlist shows.
pub const MAX_LIMIT: usize = 25;

/// Candidates verified at most per search (the network cost of one
/// `find`).
const MAX_VERIFIED: usize = 12;

/// What to look for.
#[derive(Debug, Clone)]
pub struct FindRequest {
    /// Words every job must match (title, company, location, department,
    /// team). Empty: everything.
    pub text: String,
    /// How many results to show (1 to [`MAX_LIMIT`]).
    pub limit: usize,
    /// Also show jobs that rank as maybe or low priority.
    pub all_tiers: bool,
    pub refresh: RefreshMode,
    /// Verify the best candidates whose listings aren't verified recently
    /// enough (reusing attempts from the last few minutes). Working offline
    /// means `refresh: Never` and `verify: false`.
    pub verify: bool,
}

impl Default for FindRequest {
    fn default() -> Self {
        Self {
            text: String::new(),
            limit: 5,
            all_tiers: false,
            refresh: RefreshMode::Auto,
            verify: true,
        }
    }
}

/// One shortlisted opportunity.
#[derive(Debug, Clone)]
pub struct Entry {
    pub ranking: Ranking,
    pub checked: Checked,
}

/// The answer to a [`FindRequest`].
#[derive(Debug)]
pub struct Found {
    pub refresh: Refresh,
    pub report: RankReport,
    pub shown: Vec<Entry>,
    /// Candidates verified during this search.
    pub verified_now: usize,
    pub request: FindRequest,
}

impl Found {
    /// Rankings that pass the tier filter (not excluded ones: those are
    /// never among `report.rankings`).
    pub fn eligible_for_display(&self) -> impl Iterator<Item = &Ranking> {
        let all = self.request.all_tiers;
        self.report
            .rankings
            .iter()
            .filter(move |r| all || r.tier >= Tier::WorthReviewing)
    }
}

impl LocalApp {
    /// The shortlist (see the module docs). Fails with
    /// [`AppError::NoProfile`] (before touching the network) without a
    /// profile, and with [`AppError::NoJobs`] when nothing was ever
    /// discovered.
    pub async fn find(
        &self,
        request: &FindRequest,
        progress: &dyn Progress,
        now: DateTime<Utc>,
    ) -> Result<Found, AppError> {
        if request.limit == 0 || request.limit > MAX_LIMIT {
            return Err(AppError::InvalidArguments(format!(
                "limit must be between 1 and {MAX_LIMIT}"
            )));
        }
        // Without a profile there is nothing to rank against: say so before
        // spending a refresh on it.
        if self.profile_facts().await?.is_none() {
            return Err(AppError::NoProfile);
        }
        let refresh = self
            .refresh_for_search(request.refresh, progress, now)
            .await?;
        let query = RankQuery {
            text: request.text.clone(),
            store_top: request.limit,
            all: request.all_tiers,
        };
        let all = request.all_tiers;
        let (report, verified_now) = self
            .rank_verified(
                &query,
                request.verify,
                request.limit * 2,
                &|r: &Ranking| all || r.tier >= Tier::WorthReviewing,
                progress,
                now,
            )
            .await?;
        if report.considered == 0 && request.text.trim().is_empty() {
            return Err(AppError::NoJobs);
        }

        let mut shown = Vec::new();
        for r in report
            .rankings
            .iter()
            .filter(|r| all || r.tier >= Tier::WorthReviewing)
            .take(request.limit)
        {
            let records = self.store().opportunity_records(r.opportunity).await?;
            if records.is_empty() {
                continue;
            }
            let opportunity = Opportunity {
                id: r.opportunity,
                records,
            };
            let checked = self.check(&opportunity, now).await?;
            shown.push(Entry {
                ranking: r.clone(),
                checked,
            });
        }
        // Per-person search state ("new since you last looked",
        // notifications): best effort, it never fails a search.
        let shown_now: Vec<jobhunt_storage::Shown> = shown
            .iter()
            .map(|e| jobhunt_storage::Shown {
                opportunity: e.ranking.opportunity,
                tier: e.ranking.tier,
            })
            .collect();
        if let Err(error) = self.store().record_shown(&shown_now, now).await {
            tracing::warn!(%error, "could not record the shortlist");
        }
        Ok(Found {
            refresh,
            report,
            shown,
            verified_now,
            request: request.clone(),
        })
    }
}

impl LocalApp {
    /// Refreshes discovery for a search as `mode` says. A refresh nobody
    /// asked for (`Auto`) must not stand between the person and the jobs
    /// already stored (a laptop offline, a board down): it then reports a
    /// warning instead of failing.
    pub(crate) async fn refresh_for_search(
        &self,
        mode: RefreshMode,
        progress: &dyn Progress,
        now: DateTime<Utc>,
    ) -> Result<Refresh, AppError> {
        match self.refresh(mode, &[], progress, now).await {
            Err(AppError::SourceUnavailable { failed, detail })
                if mode == RefreshMode::Auto && self.has_open_jobs().await? =>
            {
                Ok(Refresh {
                    reason: self.freshness(now).await?,
                    report: None,
                    warnings: vec![format!(
                        "could not reach any source ({failed} failed: {detail}); \
                         using stored jobs"
                    )],
                })
            }
            other => other,
        }
    }

    /// Ranks every open opportunity matching `query`; with `verify`, then
    /// verifies the best candidates among the first `pool` that `wanted`
    /// keeps whose listings aren't trusted yet (at most
    /// [`MAX_VERIFIED`]), and ranks again. Returns the report and how many
    /// candidates were verified now.
    pub(crate) async fn rank_verified(
        &self,
        query: &RankQuery,
        verify: bool,
        pool: usize,
        wanted: &(dyn Fn(&Ranking) -> bool + Sync),
        progress: &dyn Progress,
        now: DateTime<Utc>,
    ) -> Result<(RankReport, usize), AppError> {
        let ranking = self.ranking();
        let mut report = ranking.rank(query, now).await?;
        let mut verified_now = 0;
        if verify && report.considered > 0 {
            let candidates: Vec<&Ranking> = report
                .rankings
                .iter()
                .filter(|r| wanted(r))
                .take(pool)
                .collect();
            verified_now = self
                .verify_candidates(&candidates, pool.clamp(1, MAX_VERIFIED), progress, now)
                .await?;
            if verified_now > 0 {
                report = ranking.rank(query, now).await?;
            }
        }
        Ok((report, verified_now))
    }

    /// Verifies, among `candidates` (best first), up to `budget` whose
    /// listings aren't trusted yet: ones to verify first, and ones whose
    /// eligibility is unclear (their listing still deserves checking
    /// before they are shown). Returns how many were verified now (not
    /// reusing a recent attempt).
    pub(crate) async fn verify_candidates(
        &self,
        candidates: &[&Ranking],
        budget: usize,
        progress: &dyn Progress,
        now: DateTime<Utc>,
    ) -> Result<usize, AppError> {
        let budget = budget.min(MAX_VERIFIED);
        let mut due = Vec::new();
        for r in candidates {
            if due.len() >= budget {
                break;
            }
            let records = match &r.gate {
                Gate::VerifyFirst { .. } => self.store().opportunity_records(r.opportunity).await?,
                Gate::EligibilityUnclear { .. } => {
                    let records = self.store().opportunity_records(r.opportunity).await?;
                    let verified = cached(self.store(), &records).await?;
                    if trust(&verified, &self.policy(), now).standing == Standing::Trusted {
                        continue;
                    }
                    records
                }
                _ => continue,
            };
            due.push(records);
        }
        let mut verified_now = 0;
        if !due.is_empty() {
            let results: Vec<Result<_, AppError>> = futures::stream::iter(&due)
                .map(|r| self.verify_records(r, VerifyMode::IfDue, progress, now))
                .buffer_unordered(4)
                .collect()
                .await;
            for result in results {
                if result?.iter().any(|v| !v.reused) {
                    verified_now += 1;
                }
            }
        }
        Ok(verified_now)
    }
}

/// How discovery was (or wasn't) refreshed for a search.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RefreshSummary {
    /// Sources were read during this search.
    pub performed: bool,
    /// Why ("sources last read 2 days ago", "every source read 3 hours ago").
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources_read: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources_failed: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_jobs: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_jobs: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_jobs: Option<usize>,
    /// Problems that did not stop the refresh.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl RefreshSummary {
    pub fn of(refresh: &Refresh, now: DateTime<Utc>) -> Self {
        let mut out = Self {
            performed: refresh.report.is_some(),
            reason: refresh.reason.describe(now),
            sources_read: None,
            sources_failed: None,
            new_jobs: None,
            updated_jobs: None,
            closed_jobs: None,
            warnings: refresh.warnings.clone(),
        };
        if let Some(report) = &refresh.report {
            let totals = report.totals();
            out.sources_read = Some(report.succeeded());
            out.sources_failed = Some(report.sources.len() - report.succeeded());
            out.new_jobs = Some(totals.inserted);
            out.updated_jobs = Some(totals.updated + totals.reopened);
            out.closed_jobs = Some(totals.closed);
            for (source, error) in report.failures() {
                out.warnings.push(format!(
                    "skipped {source}: {}",
                    jobhunt_core::ErrorChain(error)
                ));
            }
        }
        out
    }
}

/// From every open opportunity to the few shown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Funnel {
    /// Open opportunities checked (matching the query, if any).
    pub checked: usize,
    /// Left after eligibility, closed listings, required pay minimums and
    /// what the person already acted on (rejected, applied).
    pub passed_eligibility: usize,
    /// Of those, the ones ranked maybe or better.
    pub plausible: usize,
    /// Of those, the ones worth reviewing or a strong fit.
    pub worth_reviewing: usize,
    /// Shown now.
    pub shown: usize,
}

/// Opportunities left out, by why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NotShown {
    pub ineligible: usize,
    pub below_pay_minimum: usize,
    /// The posting contradicts something the person requires: a company
    /// or team kind, their work setup, relocation, a remote geography.
    #[serde(default)]
    pub unmet_requirement: usize,
    /// Pay not published (or not comparable), and the person hides such
    /// jobs.
    #[serde(default)]
    pub pay_unknown: usize,
    /// Eligibility not confirmed, and the person hides such jobs.
    #[serde(default)]
    pub eligibility_unconfirmed: usize,
    pub closed: usize,
    pub rejected: usize,
    pub in_pipeline: usize,
    /// Ranked maybe or low priority (`all_tiers` shows them).
    pub lower_tiers: usize,
    /// Beyond the limit.
    pub beyond_limit: usize,
}

/// One shortlisted opportunity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ShortlistItem {
    /// `opp_…`: the logical opportunity (one job, however many sources
    /// list it). Use it with every other tool.
    pub id: String,
    /// Shown by the CLI (`opp_1a2b3c4d`); accepted back while unique.
    pub short_id: String,
    pub title: String,
    pub company: String,
    pub tier: FitTier,
    pub recommendation: Recommendation,
    /// What to check first, when it applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation_note: Option<String>,
    /// Role · level · stack · domain · pay · where.
    pub summary: String,
    pub verification: VerificationBrief,
    pub eligibility: EligibilityBrief,
    /// Published pay: verified facts when there are any, else what the
    /// listing said when discovered.
    pub compensation: CompensationView,
    /// Every location the listing names, primary first.
    #[serde(default)]
    pub locations: Vec<String>,
    /// `remote`, `hybrid` or `onsite`, as the listing says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workplace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub department: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    /// Why it may be worth the person's time (up to 4; verification and
    /// eligibility are in their own fields).
    pub why: Vec<String>,
    /// What to consider: caveats, then unknowns (up to 3).
    pub consider: Vec<String>,
    /// The lines of `consider` about what the posting doesn't say
    /// (unknowns, not cautions); the others are caveats.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknowns: Vec<String>,
    /// How many sources list it.
    pub sources: usize,
    /// The suggested next step, as a CLI command.
    pub next_step: String,
}

impl ShortlistItem {
    pub fn of(entry: &Entry) -> Self {
        let r = &entry.ranking;
        let id = r.opportunity;
        // Verification and eligibility have their own fields; their brief
        // lines would only repeat them.
        let status_lines: Vec<&str> = r
            .signals
            .iter()
            .filter(|s| {
                matches!(
                    s.group,
                    SignalGroup::Eligibility | SignalGroup::Verification
                )
            })
            .map(|s| s.summary.as_str())
            .collect();
        let fresh = |line: &&String| !status_lines.contains(&line.as_str());
        let caveats = r.brief.caveats.iter().filter(fresh).take(2);
        let unknowns: Vec<String> = r
            .brief
            .unknowns
            .iter()
            .filter(fresh)
            .take(1)
            .cloned()
            .collect();
        let consider: Vec<String> = caveats.cloned().chain(unknowns.iter().cloned()).collect();
        let next_step = match &r.gate {
            Gate::VerifyFirst { .. } => format!("jobhunt verify {}", short_id(&id)),
            Gate::EligibilityUnclear { .. } => format!("jobhunt check {}", short_id(&id)),
            _ => format!("jobhunt why {}", short_id(&id)),
        };
        let record = best_record(&entry.checked.verified, &entry.checked.trust, None);
        let posting = record.map(|r| &r.posting);
        Self {
            id: id.to_string(),
            short_id: short_id(&id),
            title: r.title.clone(),
            company: r.company.clone(),
            tier: r.tier.into(),
            recommendation: (&r.gate).into(),
            recommendation_note: gate_note(&r.gate),
            summary: r.brief.summary.clone(),
            verification: VerificationBrief::of(&entry.checked.trust),
            eligibility: EligibilityBrief::of(
                entry.checked.assessment.as_ref().map(|a| &a.decision),
            ),
            compensation: match record {
                Some(record) => CompensationView::best(record, &entry.checked.trust),
                None => CompensationView::of(
                    &jobhunt_jobs::verification::CompensationCheck::observe(None, None),
                    None,
                ),
            },
            locations: record.map(locations).unwrap_or_default(),
            workplace: posting
                .and_then(|p| p.workplace_type.as_ref())
                .map(|w| w.as_str().to_owned()),
            remote: posting.and_then(|p| p.is_remote),
            department: posting.and_then(|p| p.department.clone()),
            team: posting.and_then(|p| p.team.clone()),
            why: r
                .brief
                .worth
                .iter()
                .filter(fresh)
                .take(4)
                .cloned()
                .collect(),
            consider,
            unknowns,
            sources: entry.checked.verified.len(),
            next_step,
        }
    }
}

/// What feedback has taught so far.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Learning {
    /// Pieces of feedback (not counting "looked at").
    pub feedback: usize,
    /// Patterns learned from it that affect ranking now.
    pub active_patterns: usize,
    /// The person has stated preferences (they always outrank learned
    /// taste).
    pub has_preferences: bool,
}

/// The answer of `search_jobs` (and `jobhunt find --json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SearchResults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub refresh: RefreshSummary,
    pub funnel: Funnel,
    /// Best first. Small on purpose.
    pub results: Vec<ShortlistItem>,
    pub not_shown: NotShown,
    pub learning: Learning,
    /// Listings verified during this search.
    pub verified_now: usize,
    /// Things worth knowing about these results.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl SearchResults {
    pub fn of(found: &Found, now: DateTime<Utc>) -> Self {
        let report = &found.report;
        let e = &report.excluded;
        let passed = report.rankings.len();
        let plausible = report
            .rankings
            .iter()
            .filter(|r| r.tier >= Tier::Maybe)
            .count();
        let worth = report
            .rankings
            .iter()
            .filter(|r| r.tier >= Tier::WorthReviewing)
            .count();
        let displayable = found.eligible_for_display().count();
        let mut notes = Vec::new();
        if !report.person.has_preferences() {
            notes.push(
                "Ranking improves with preferences and feedback: say what you want \
                 (jobhunt preferences add \"…\" / update_preferences) and save or reject jobs \
                 with a reason."
                    .to_owned(),
            );
        }
        if found.shown.is_empty() && passed > 0 && !found.request.all_tiers {
            notes.push(format!(
                "Nothing stands out yet: {plausible} opportunities rank as maybe or low priority \
                 (all_tiers / --all shows them)."
            ));
        }
        if matches!(found.refresh.reason, RefreshReason::Disabled) {
            notes.push("Working offline: stored jobs were not refreshed or verified.".into());
        }
        if matches!(
            found.refresh.reason,
            RefreshReason::Background { oldest: None }
        ) {
            notes.push(
                "Narrow has not read any job board yet; its scheduled discovery will.".into(),
            );
        }
        Self {
            query: Some(found.request.text.trim().to_owned()).filter(|q| !q.is_empty()),
            refresh: RefreshSummary::of(&found.refresh, now),
            funnel: Funnel {
                checked: report.considered,
                passed_eligibility: passed,
                plausible,
                worth_reviewing: worth,
                shown: found.shown.len(),
            },
            results: found.shown.iter().map(ShortlistItem::of).collect(),
            not_shown: NotShown {
                ineligible: e.ineligible,
                below_pay_minimum: e.below_minimum,
                unmet_requirement: e.unmet_requirement,
                pay_unknown: e.pay_unknown,
                eligibility_unconfirmed: e.eligibility_unconfirmed,
                closed: e.closed,
                rejected: e.rejected,
                in_pipeline: e.in_pipeline,
                lower_tiers: if found.request.all_tiers {
                    0
                } else {
                    passed - worth
                },
                beyond_limit: displayable.saturating_sub(found.shown.len()),
            },
            learning: Learning {
                feedback: report.taste.events,
                active_patterns: report.taste.active().count(),
                has_preferences: report.person.has_preferences(),
            },
            verified_now: found.verified_now,
            notes,
        }
    }
}
