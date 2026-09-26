//! The use cases front-ends call: record feedback, see an opportunity's
//! state, derive taste, rank opportunities, explain one ranking, list the
//! pipeline.
//!
//! Everything goes through the domain's repository traits (jobs,
//! verification, eligibility, profile, feedback, rankings); nothing here
//! knows which database is used.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};
use jobhunt_eligibility::{
    Assessment, EligibilityRepository, ProfileFacts, cached_assess, cached_assess_many,
};
use jobhunt_jobs::verification::{FreshnessPolicy, VerificationRepository, cached, cached_many};
use jobhunt_jobs::{JobId, JobQuery, JobRecord, JobRepository, JobStatus, OpportunityId};
use jobhunt_profile::{ProfileData, ProfileError, ProfileRepository, ProfileService};

use crate::cache::{FeedbackRepository, RankKey, RankingRepository, cached_rank};
use crate::facets::facets;
use crate::feedback::{FeedbackAction, FeedbackEvent, OpportunityState, Stage};
use crate::person::Person;
use crate::rank::{Candidate, Context, Exclusion, Gate, Ranking, Tier, order, rank};
use crate::reason::{ReasonReader, ReasonReading};
use crate::taste::{FeedbackOpportunity, TasteModel, derive};

/// Something the use case needs is missing, or storage failed.
#[derive(Debug, thiserror::Error)]
pub enum RankingError {
    #[error(transparent)]
    Storage(#[from] jobhunt_jobs::StorageError),
    #[error(transparent)]
    Profile(#[from] ProfileError),
    #[error(
        "ranking needs a career profile: run `jobhunt init <resume>` or \
         `jobhunt preferences set location <place>`"
    )]
    NoProfile,
    #[error("no source records to act on")]
    NoRecords,
}

/// What recording feedback did.
#[derive(Debug, Clone)]
pub struct Recorded {
    /// The event stored now; or, when nothing was stored (`recorded` is
    /// false), the earlier event it repeats (or the one it would have been).
    pub event: FeedbackEvent,
    pub before: OpportunityState,
    pub after: OpportunityState,
    /// How the reason was read (`None` without a reason).
    pub reading: Option<ReasonReading>,
    /// Whether a new event was stored. An action that changes nothing
    /// (the state already reflects it, and its reason, if any, was
    /// already given for the same action) is a repeat, typically a retry,
    /// and is not stored again.
    pub recorded: bool,
}

/// One ranking with what it was computed from.
#[derive(Debug, Clone)]
pub struct Explained {
    pub ranking: Ranking,
    pub assessment: Assessment,
    pub state: OpportunityState,
    pub taste: TasteModel,
    pub person: Person,
    /// It was read from the store rather than computed now.
    pub reused: bool,
}

/// What to rank.
#[derive(Debug, Clone, Default)]
pub struct RankQuery {
    /// Words every job must match (as `find`).
    pub text: String,
    /// How many of the best rankings to store as shown.
    pub store_top: usize,
    /// Maybe and low-priority jobs are shown too (by default only strong
    /// fits and jobs worth reviewing are).
    pub all: bool,
}

/// Opportunities left out of the recommendations, by why.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Excluded {
    pub rejected: usize,
    pub in_pipeline: usize,
    pub closed: usize,
    pub ineligible: usize,
    pub below_minimum: usize,
    /// The posting contradicts a required company or team kind.
    pub unmet_requirement: usize,
}

impl Excluded {
    fn count(&mut self, exclusion: &Exclusion) {
        match exclusion {
            Exclusion::Rejected => self.rejected += 1,
            Exclusion::InPipeline { .. } => self.in_pipeline += 1,
            Exclusion::Closed { .. } => self.closed += 1,
            Exclusion::Ineligible { .. } => self.ineligible += 1,
            Exclusion::BelowMinimum { .. } => self.below_minimum += 1,
            Exclusion::UnmetRequirement { .. } => self.unmet_requirement += 1,
        }
    }

    pub fn total(&self) -> usize {
        self.rejected
            + self.in_pipeline
            + self.closed
            + self.ineligible
            + self.below_minimum
            + self.unmet_requirement
    }
}

/// Every open opportunity, ranked.
#[derive(Debug, Clone)]
pub struct RankReport {
    /// Not excluded, best first.
    pub rankings: Vec<Ranking>,
    pub excluded: Excluded,
    /// Open opportunities looked at.
    pub considered: usize,
    pub taste: TasteModel,
    pub person: Person,
    /// Where the time went, for operations.
    pub timings: RankTimings,
}

/// How long each part of a ranking took, in milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RankTimings {
    /// The person: profile, preferences, learned taste, feedback.
    pub person_ms: u64,
    /// The open opportunities, their records and verification state.
    pub load_ms: u64,
    pub eligibility_ms: u64,
    pub ranking_ms: u64,
}

fn ms_since(started: std::time::Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// An opportunity in the person's pipeline.
#[derive(Debug, Clone)]
pub struct PipelineEntry {
    pub opportunity: OpportunityId,
    /// The record the latest action was about.
    pub job: JobId,
    pub title: String,
    pub company: String,
    pub state: OpportunityState,
    /// Every record of the opportunity is closed.
    pub closed: bool,
}

pub struct RankingService<'a, R: ?Sized> {
    repo: &'a R,
    reader: &'a dyn ReasonReader,
    policy: FreshnessPolicy,
}

impl<'a, R> RankingService<'a, R>
where
    R: JobRepository
        + VerificationRepository
        + EligibilityRepository
        + FeedbackRepository
        + RankingRepository
        + ProfileRepository
        + ?Sized,
{
    pub fn new(repo: &'a R, reader: &'a dyn ReasonReader) -> Self {
        Self {
            repo,
            reader,
            policy: FreshnessPolicy::default(),
        }
    }

    pub fn with_policy(mut self, policy: FreshnessPolicy) -> Self {
        self.policy = policy;
        self
    }

    fn profile_id(&self) -> String {
        ProfileService::new(self.repo).profile_id().to_string()
    }

    /// The stored profile, and the person as ranking reads them (empty
    /// without a profile).
    pub async fn person(&self) -> Result<(Option<ProfileData>, Person), RankingError> {
        let data = ProfileService::new(self.repo).load().await?;
        let person = match &data {
            Some(d) => Person::from_profile(d),
            None => Person {
                profile_id: self.profile_id(),
                ..Person::default()
            },
        };
        Ok((data, person))
    }

    /// The person's state on an opportunity, from the feedback on any of
    /// its records.
    pub async fn state(&self, records: &[JobRecord]) -> Result<OpportunityState, RankingError> {
        let ids: Vec<JobId> = records.iter().map(|r| r.id).collect();
        let events = self
            .repo
            .feedback_for_jobs(&self.profile_id(), &ids)
            .await?;
        Ok(OpportunityState::of(events))
    }

    /// Records an action on an opportunity. `records` are its source
    /// records, the one the person was looking at first.
    pub async fn record(
        &self,
        records: &[JobRecord],
        action: FeedbackAction,
        reason: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Recorded, RankingError> {
        let main = records.first().ok_or(RankingError::NoRecords)?;
        let before = self.state(records).await?;
        let event = FeedbackEvent::new(
            &self.profile_id(),
            main.opportunity_id,
            main.id,
            action,
            reason,
            (&main.posting.title, &main.posting.company),
            now,
        );
        let after = OpportunityState::of(before.events.iter().cloned().chain([event.clone()]));
        let reading = event.reason.as_deref().map(|r| self.reader.read(r, action));
        let same_state = after.stage == before.stage
            && after.sentiment == before.sentiment
            && after.furthest == before.furthest;
        let said_before = |e: &FeedbackEvent| e.action == action && e.reason == event.reason;
        let repeat =
            same_state && (event.reason.is_none() || before.events.iter().any(said_before));
        if repeat {
            let earlier = before
                .events
                .iter()
                .rev()
                .find(|e| said_before(e))
                .or_else(|| before.events.iter().rev().find(|e| e.action == action))
                .cloned()
                .unwrap_or(event);
            return Ok(Recorded {
                event: earlier,
                after: before.clone(),
                before,
                reading,
                recorded: false,
            });
        }
        self.repo.record_feedback(&event).await?;
        Ok(Recorded {
            event,
            before,
            after,
            reading,
            recorded: true,
        })
    }

    /// Records that the person looked at an opportunity, once.
    pub async fn mark_seen(
        &self,
        records: &[JobRecord],
        now: DateTime<Utc>,
    ) -> Result<bool, RankingError> {
        if self.state(records).await?.stage != Stage::Unseen {
            return Ok(false);
        }
        self.record(records, FeedbackAction::Seen, None, now)
            .await?;
        Ok(true)
    }

    /// Every feedback event, oldest first.
    pub async fn feedback(&self) -> Result<Vec<FeedbackEvent>, RankingError> {
        Ok(self.repo.feedback(&self.profile_id()).await?)
    }

    /// Events grouped by the opportunity their record belongs to now (so
    /// feedback follows its record when opportunities are merged).
    async fn by_opportunity(
        &self,
        events: Vec<FeedbackEvent>,
    ) -> Result<BTreeMap<OpportunityId, Vec<FeedbackEvent>>, RankingError> {
        let mut current: HashMap<JobId, OpportunityId> = HashMap::new();
        let mut groups: BTreeMap<OpportunityId, Vec<FeedbackEvent>> = BTreeMap::new();
        for event in events {
            let opportunity = match current.get(&event.job) {
                Some(o) => *o,
                None => {
                    let o = self
                        .repo
                        .get(event.job)
                        .await?
                        .map_or(event.opportunity, |r| r.opportunity_id);
                    current.insert(event.job, o);
                    o
                }
            };
            groups.entry(opportunity).or_default().push(event);
        }
        Ok(groups)
    }

    /// Taste learned from every piece of feedback, with its evidence.
    pub async fn taste(&self, person: &Person) -> Result<TasteModel, RankingError> {
        let events = self.feedback().await?;
        let mut opportunities = Vec::new();
        for (opportunity, events) in self.by_opportunity(events).await? {
            let records = self.repo.opportunity_records(opportunity).await?;
            let representative = records
                .iter()
                .find(|r| r.status == JobStatus::Open)
                .or_else(|| records.first());
            let fingerprint = records
                .iter()
                .map(|r| r.posting.content_fingerprint().to_hex())
                .collect::<Vec<_>>()
                .join(",");
            opportunities.push(FeedbackOpportunity {
                opportunity,
                state: OpportunityState::of(events),
                facets: representative.map(facets),
                fingerprint,
            });
        }
        Ok(derive(&opportunities, &person.explicit_keys(), self.reader))
    }

    async fn assess(
        &self,
        records: &[JobRecord],
        facts: &ProfileFacts,
        now: DateTime<Utc>,
    ) -> Result<Assessment, RankingError> {
        let verified = cached(self.repo, records).await?;
        let (assessment, _) = cached_assess(self.repo, &verified, facts, &self.policy, now).await?;
        Ok(assessment)
    }

    /// The ranking of one opportunity, stored as shown.
    pub async fn explain(
        &self,
        records: &[JobRecord],
        now: DateTime<Utc>,
    ) -> Result<Explained, RankingError> {
        let (data, person) = self.person().await?;
        let data = data.ok_or(RankingError::NoProfile)?;
        let facts = ProfileFacts::from_profile(&data);
        let taste = self.taste(&person).await?;
        let state = self.state(records).await?;
        let assessment = self.assess(records, &facts, now).await?;
        let ctx = Context {
            person: &person,
            taste: &taste,
            now,
        };
        let candidate = Candidate {
            records,
            assessment: &assessment,
            state: &state,
        };
        let (ranking, reused) = cached_rank(self.repo, &candidate, &ctx, true)
            .await?
            .ok_or(RankingError::NoRecords)?;
        Ok(Explained {
            ranking,
            assessment,
            state,
            taste,
            person,
            reused,
        })
    }

    /// Ranks every open opportunity matching the query.
    pub async fn rank(
        &self,
        query: &RankQuery,
        now: DateTime<Utc>,
    ) -> Result<RankReport, RankingError> {
        let mut timings = RankTimings::default();
        let started = std::time::Instant::now();
        let (data, person) = self.person().await?;
        let data = data.ok_or(RankingError::NoProfile)?;
        let facts = ProfileFacts::from_profile(&data);
        let taste = self.taste(&person).await?;
        let mut by_job: HashMap<JobId, Vec<FeedbackEvent>> = HashMap::new();
        for event in self.feedback().await? {
            by_job.entry(event.job).or_default().push(event);
        }
        let search = JobQuery {
            status: Some(JobStatus::Open),
            distinct_opportunities: true,
            ..JobQuery::default()
        }
        .with_text(&query.text);
        let ctx = Context {
            person: &person,
            taste: &taste,
            now,
        };
        timings.person_ms = ms_since(started);
        let started = std::time::Instant::now();
        struct Prepared {
            records: Vec<JobRecord>,
            assessment: Assessment,
            state: OpportunityState,
        }
        let mut prepared: Vec<Prepared> = Vec::new();
        let mut excluded = Excluded::default();
        let mut rankings: Vec<(usize, Ranking)> = Vec::new();
        let listed = self.repo.search(&search).await?;
        let considered = listed.len();
        // Gather every candidate first, then assess them together: records
        // and verification state in two repository calls, and one
        // eligibility lookup (and one write), whatever the search's size.
        let ids: Vec<OpportunityId> = listed.iter().map(|r| r.opportunity_id).collect();
        let mut by_opportunity = self.repo.opportunity_records_many(&ids).await?;
        let mut gathered: Vec<(Vec<JobRecord>, OpportunityState)> = Vec::new();
        for record in listed {
            let records = by_opportunity
                .remove(&record.opportunity_id)
                .unwrap_or_default();
            let state = OpportunityState::of(
                records
                    .iter()
                    .flat_map(|r| by_job.get(&r.id).cloned().unwrap_or_default()),
            );
            gathered.push((records, state));
        }
        let all: Vec<&[JobRecord]> = gathered.iter().map(|(r, _)| r.as_slice()).collect();
        let verified = cached_many(self.repo, &all).await?;
        timings.load_ms = ms_since(started);
        let started = std::time::Instant::now();
        let assessments =
            cached_assess_many(self.repo, &verified, &facts, &self.policy, now).await?;
        timings.eligibility_ms = ms_since(started);
        let started = std::time::Instant::now();
        for ((records, state), assessment) in gathered.into_iter().zip(assessments) {
            let candidate = Candidate {
                records: &records,
                assessment: &assessment,
                state: &state,
            };
            let Some(ranking) = rank(&candidate, &ctx) else {
                continue;
            };
            if let Gate::Excluded { exclusion } = &ranking.gate {
                excluded.count(exclusion);
                continue;
            }
            rankings.push((prepared.len(), ranking));
            prepared.push(Prepared {
                records,
                assessment,
                state,
            });
        }
        timings.ranking_ms = ms_since(started);
        let mut ordered: Vec<Ranking> = Vec::with_capacity(rankings.len());
        let mut index: HashMap<OpportunityId, usize> = HashMap::new();
        for (i, r) in rankings {
            index.insert(r.opportunity, i);
            ordered.push(r);
        }
        order(&mut ordered);
        // Keep a record of what was shown, and why.
        let shown = ordered
            .iter()
            .filter(|r| query.all || r.tier >= Tier::WorthReviewing)
            .take(query.store_top);
        for ranking in shown {
            let Some(p) = index.get(&ranking.opportunity).map(|i| &prepared[*i]) else {
                continue;
            };
            let candidate = Candidate {
                records: &p.records,
                assessment: &p.assessment,
                state: &p.state,
            };
            if let Some(key) = RankKey::of(&candidate, &ctx)
                && self.repo.cached_ranking(&key).await?.is_none()
            {
                self.repo.store_ranking(&key, ranking, now).await?;
            }
        }
        Ok(RankReport {
            rankings: ordered,
            excluded,
            considered,
            taste,
            person,
            timings,
        })
    }

    /// Opportunities the person saved or applied to (and, with
    /// `rejected`, the ones they turned down), latest action first.
    pub async fn pipeline(&self, rejected: bool) -> Result<Vec<PipelineEntry>, RankingError> {
        let events = self.feedback().await?;
        let mut out = Vec::new();
        for (opportunity, events) in self.by_opportunity(events).await? {
            let state = OpportunityState::of(events);
            let keep = match state.stage {
                Stage::Saved | Stage::Applied | Stage::Interviewing | Stage::Offer => true,
                Stage::Rejected => rejected,
                Stage::Seen | Stage::Unseen => false,
            };
            let Some(last) = state.events.last() else {
                continue;
            };
            if !keep {
                continue;
            }
            let records = self.repo.opportunity_records(opportunity).await?;
            let closed =
                !records.is_empty() && records.iter().all(|r| r.status == JobStatus::Closed);
            out.push(PipelineEntry {
                opportunity,
                job: last.job,
                title: last.title.clone(),
                company: last.company.clone(),
                closed,
                state,
            });
        }
        let progress = |s: Stage| match s {
            Stage::Offer => 0,
            Stage::Interviewing => 1,
            Stage::Applied => 2,
            Stage::Saved => 3,
            _ => 4,
        };
        out.sort_by(|a, b| {
            progress(a.state.stage)
                .cmp(&progress(b.state.stage))
                .then(b.state.since().cmp(&a.state.since()))
        });
        Ok(out)
    }
}
