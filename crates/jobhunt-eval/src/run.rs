//! Running the current ranker on the fixtures, and what it did with each
//! candidate's pool.
//!
//! Every job is ranked exactly as `RankingService::rank` ranks it
//! ([`jobhunt_ranking::rank()`] on the eligibility assessment, with the
//! person read by [`Person::from_profile`]); the only differences are that
//! nothing is stored and learned taste is empty (a person with no
//! feedback yet).
//!
//! Today is then selected as `jobhunt-app`'s feed selects it (`feed.rs`):
//! a job **qualifies** for Today ("worth your attention") when its gate is
//! not an exclusion and its tier is at least worth reviewing; the **feed**
//! is the qualifying jobs in rank order, one per company, at most
//! [`FEED_LIMIT`]. Every benchmark job is new to the candidate, so the
//! feed's "new" rule keeps all of them. If the feed's selection changes,
//! [`qualifies`] and [`feed`] must change with it.

use jobhunt_core::text::search_key;
use jobhunt_eligibility::{Eligibility, ProfileFacts};
use jobhunt_ranking::rank::order;
use jobhunt_ranking::{
    Candidate, Context, Exclusion, Gate, OpportunityState, Person, RULE_READER_REVISION, Ranking,
    SignalGroup, SignalKind, TasteModel, Tier, rank,
};

use crate::build::{self, BuildError};
use crate::fixture::{CandidateFixture, Fixtures, Group, JobFixture, Judgment};
use crate::taxonomy::{Contradiction, Label, TodayExpectation};

/// The feed's default size (`jobhunt_app::feed::DEFAULT_FEED_LIMIT`).
pub const FEED_LIMIT: usize = 5;

/// Whether a ranking earns "worth your attention": what the feed
/// considers (`tier >= WorthReviewing` among the rankings that are not
/// excluded).
pub fn qualifies(r: &Ranking) -> bool {
    !r.gate.is_excluded() && r.tier >= Tier::WorthReviewing
}

/// The Today feed from a pool: qualifying rankings in rank order, the
/// best of each company, at most `limit`.
pub fn feed(rankings: &[Ranking], limit: usize) -> Vec<&Ranking> {
    let mut ordered: Vec<Ranking> = rankings.iter().filter(|r| qualifies(r)).cloned().collect();
    order(&mut ordered);
    let mut companies: Vec<String> = Vec::new();
    let mut out: Vec<&Ranking> = Vec::new();
    for r in &ordered {
        let company = search_key(&r.facets.company);
        if companies.contains(&company) {
            continue;
        }
        if out.len() == limit {
            break;
        }
        companies.push(company);
        if let Some(original) = rankings.iter().find(|o| o.opportunity == r.opportunity) {
            out.push(original);
        }
    }
    out
}

/// The gate, in a few words.
pub fn gate_label(gate: &Gate) -> String {
    match gate {
        Gate::Recommended => "recommended".into(),
        Gate::VerifyFirst { .. } => "verify first".into(),
        Gate::EligibilityUnclear { .. } => "eligibility unclear".into(),
        Gate::Excluded { exclusion } => format!(
            "excluded ({})",
            match exclusion {
                Exclusion::Rejected => "rejected",
                Exclusion::InPipeline { .. } => "in pipeline",
                Exclusion::Closed { .. } => "closed",
                Exclusion::Ineligible { .. } => "ineligible",
                Exclusion::BelowMinimum { .. } => "below minimum pay",
                Exclusion::UnmetRequirement { .. } => "unmet requirement",
                Exclusion::PayUnknown { .. } => "pay unknown",
                Exclusion::EligibilityUnconfirmed { .. } => "eligibility unconfirmed",
            }
        ),
    }
}

/// The contradictions the current ranker flagged on its own: a negative
/// signal (or a gate) of the kind that would carry each one.
pub fn detected(r: &Ranking) -> Vec<Contradiction> {
    let negative = |groups: &[SignalGroup]| {
        r.signals
            .iter()
            .any(|s| groups.contains(&s.group) && (s.weight < 0.0 || s.kind == SignalKind::Blocker))
    };
    let mut out = Vec::new();
    if negative(&[SignalGroup::Seniority]) {
        out.push(Contradiction::Seniority);
    }
    if negative(&[SignalGroup::Role, SignalGroup::Stack]) {
        out.push(Contradiction::RoleDepth);
    }
    let ruled_out = matches!(
        &r.gate,
        Gate::Excluded {
            exclusion: Exclusion::Ineligible { .. } | Exclusion::UnmetRequirement { .. }
        }
    );
    let blocked = r.signals.iter().any(|s| {
        matches!(s.group, SignalGroup::Eligibility | SignalGroup::WorkMode)
            && s.kind == SignalKind::Blocker
    });
    if ruled_out || blocked {
        out.push(Contradiction::Eligibility);
    }
    if negative(&[SignalGroup::Company]) {
        out.push(Contradiction::CompanyShape);
    }
    out
}

/// What the current ranker did with one job for one candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct Observed {
    pub gate: String,
    /// Why the gate is what it is (the exclusion, or what is unclear).
    pub gate_why: Option<String>,
    pub eligibility: Eligibility,
    pub tier: Tier,
    pub score: f64,
    /// Earns "worth your attention" (see [`qualifies`]).
    pub qualifies: bool,
    /// Among the few the feed shows.
    pub in_feed: bool,
    /// The brief's reasons, as the person sees them.
    pub worth: Vec<String>,
    pub caveats: Vec<String>,
    pub unknowns: Vec<String>,
    pub detected: Vec<Contradiction>,
    /// It qualifies only because of pay: ranked again without the
    /// candidate's pay preferences, it doesn't.
    pub pay_carried: bool,
    /// The ranker read the published pay as meeting what the candidate
    /// wants (a positive pay signal).
    pub pay_read_as_met: bool,
    /// Its gate is "eligibility unclear": nothing confirms the candidate
    /// can take it.
    pub eligibility_unconfirmed: bool,
}

/// How a case came out against its judgment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Verdict {
    Pass,
    /// A No or Impossible labeled worth the candidate's attention.
    FalsePositive,
    /// A Maybe labeled worth the candidate's attention.
    MaybeInToday,
    /// A practical Strong yes not surfaced.
    Missed,
}

impl Verdict {
    pub fn of(expected: TodayExpectation, surfaced: bool) -> Self {
        match (expected, surfaced) {
            (TodayExpectation::Never, true) => Self::FalsePositive,
            (TodayExpectation::Hold, true) => Self::MaybeInToday,
            (TodayExpectation::Surface, false) => Self::Missed,
            _ => Self::Pass,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::FalsePositive => "FAIL: false positive",
            Self::MaybeInToday => "FAIL: maybe in Today",
            Self::Missed => "FAIL: missed",
        }
    }

    pub fn failed(self) -> bool {
        self != Self::Pass
    }
}

/// One candidate and one job: the judgment, and what happened.
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    pub job: String,
    pub title: String,
    pub company: String,
    pub group: Group,
    pub pair: Option<String>,
    /// For golden jobs: whether this is the candidate the production
    /// failure was observed on.
    pub as_observed: bool,
    pub judgment: Judgment,
    pub observed: Observed,
}

impl Case {
    pub fn label(&self) -> Label {
        self.judgment.label()
    }

    pub fn surfaced(&self) -> bool {
        self.observed.qualifies
    }

    pub fn verdict(&self) -> Verdict {
        Verdict::of(self.judgment.today(), self.surfaced())
    }

    /// The contradictions the judgment names.
    pub fn contradictions(&self) -> Vec<Contradiction> {
        let mut out: Vec<Contradiction> = self
            .judgment
            .reasons
            .iter()
            .filter_map(|r| r.contradiction())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Named contradictions the job surfaced despite.
    pub fn missed(&self) -> Vec<Contradiction> {
        if self.surfaced() {
            self.contradictions()
        } else {
            Vec::new()
        }
    }

    /// Named contradictions the ranker has no signal for.
    pub fn undetected(&self) -> Vec<Contradiction> {
        self.contradictions()
            .into_iter()
            .filter(|c| !self.observed.detected.contains(c))
            .collect()
    }
}

/// What the ranker read about a candidate, for the report.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub location: Option<String>,
    pub level: Option<String>,
    pub roles: Vec<String>,
    pub technologies: Vec<String>,
    pub stated: Vec<String>,
}

impl Reading {
    fn of(person: &Person, facts: &ProfileFacts) -> Self {
        Self {
            location: facts.location.as_ref().map(|l| l.raw.clone()),
            level: person
                .level
                .as_ref()
                .map(|(l, title)| format!("{} ({title})", l.as_str())),
            roles: person.roles.iter().map(|r| r.topic.clone()).collect(),
            technologies: person
                .technologies
                .iter()
                .map(|(name, strength)| format!("{name} ({strength:?})").to_lowercase())
                .collect(),
            stated: person.stated.iter().map(|p| p.text.clone()).collect(),
        }
    }
}

/// One candidate's whole pool.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateRun {
    pub id: String,
    pub summary: String,
    pub unexpressed: Vec<String>,
    pub reading: Reading,
    /// In the order the candidate's judgments are written.
    pub cases: Vec<Case>,
    /// Job ids on the simulated feed, best first.
    pub feed: Vec<String>,
}

/// Every candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub candidates: Vec<CandidateRun>,
}

fn rank_one(
    job: &JobFixture,
    record: &jobhunt_jobs::JobRecord,
    person: &Person,
    facts: &ProfileFacts,
    taste: &TasteModel,
) -> Result<(Ranking, Eligibility), BuildError> {
    let assessment = build::assessment(job, record, facts);
    let records = [record.clone()];
    let state = OpportunityState::default();
    let candidate = Candidate {
        records: &records,
        assessment: &assessment,
        state: &state,
    };
    let ctx = Context {
        person,
        taste,
        now: build::now(),
    };
    let ranking = rank(&candidate, &ctx).ok_or_else(|| BuildError::Job {
        job: job.id.clone(),
        message: "the ranker returned nothing".into(),
    })?;
    Ok((ranking, assessment.decision.status))
}

/// Runs one candidate's pool through the current ranker.
pub fn run_candidate(
    fixtures: &Fixtures,
    candidate: &CandidateFixture,
) -> Result<CandidateRun, BuildError> {
    let data = build::profile(candidate)?;
    let person = Person::from_profile(&data);
    let facts = ProfileFacts::from_profile(&data);
    let taste = TasteModel::empty(RULE_READER_REVISION);
    let without_pay = Person {
        pay: Vec::new(),
        ..person.clone()
    };
    let mut ranked: Vec<(&Judgment, &JobFixture, Ranking, Eligibility, bool)> = Vec::new();
    for judgment in &candidate.judgment {
        let Some(job) = fixtures.jobs.get(&judgment.job) else {
            return Err(BuildError::Job {
                job: judgment.job.clone(),
                message: "no such job".into(),
            });
        };
        let record = build::record(job)?;
        let (ranking, eligibility) = rank_one(job, &record, &person, &facts, &taste)?;
        let pay_carried = qualifies(&ranking)
            && !qualifies(&rank_one(job, &record, &without_pay, &facts, &taste)?.0);
        ranked.push((judgment, job, ranking, eligibility, pay_carried));
    }
    let rankings: Vec<Ranking> = ranked.iter().map(|(_, _, r, ..)| r.clone()).collect();
    let on_feed: Vec<_> = feed(&rankings, FEED_LIMIT)
        .into_iter()
        .map(|r| r.opportunity)
        .collect();
    let mut feed_jobs = Vec::new();
    for id in &on_feed {
        if let Some((_, job, ..)) = ranked.iter().find(|(_, _, r, ..)| r.opportunity == *id) {
            feed_jobs.push(job.id.clone());
        }
    }
    let cases = ranked
        .into_iter()
        .map(|(judgment, job, r, eligibility, pay_carried)| Case {
            job: job.id.clone(),
            title: job.title.clone(),
            company: job.company.clone(),
            group: job.group,
            pair: job.pair.clone(),
            as_observed: job.observed_for.as_deref() == Some(candidate.id.as_str()),
            judgment: judgment.clone(),
            observed: Observed {
                gate: gate_label(&r.gate),
                gate_why: match &r.gate {
                    Gate::Excluded { exclusion } => Some(exclusion.label()),
                    Gate::EligibilityUnclear { why } | Gate::VerifyFirst { why } => {
                        Some(why.clone())
                    }
                    Gate::Recommended => None,
                },
                eligibility,
                tier: r.tier,
                score: r.score,
                qualifies: qualifies(&r),
                in_feed: on_feed.contains(&r.opportunity),
                worth: r.brief.worth.clone(),
                caveats: r.brief.caveats.clone(),
                unknowns: r.brief.unknowns.clone(),
                detected: detected(&r),
                pay_carried,
                pay_read_as_met: r
                    .signals
                    .iter()
                    .any(|s| s.group == SignalGroup::Compensation && s.weight > 0.0),
                eligibility_unconfirmed: matches!(r.gate, Gate::EligibilityUnclear { .. }),
            },
        })
        .collect();
    Ok(CandidateRun {
        id: candidate.id.clone(),
        summary: candidate.summary.clone(),
        unexpressed: candidate.unexpressed.clone(),
        reading: Reading::of(&person, &facts),
        cases,
        feed: feed_jobs,
    })
}

/// Runs every candidate.
pub fn run(fixtures: &Fixtures) -> Result<Run, BuildError> {
    let candidates = fixtures
        .candidates
        .iter()
        .map(|c| run_candidate(fixtures, c))
        .collect::<Result<_, _>>()?;
    Ok(Run { candidates })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdicts_compare_today_with_the_expectation() {
        use TodayExpectation::*;
        let table = [
            (Surface, true, Verdict::Pass),
            (Surface, false, Verdict::Missed),
            (Either, true, Verdict::Pass),
            (Either, false, Verdict::Pass),
            (Hold, true, Verdict::MaybeInToday),
            (Hold, false, Verdict::Pass),
            (Never, true, Verdict::FalsePositive),
            (Never, false, Verdict::Pass),
        ];
        for (expected, surfaced, verdict) in table {
            assert_eq!(
                Verdict::of(expected, surfaced),
                verdict,
                "{expected:?}, surfaced {surfaced}"
            );
            assert_eq!(verdict.failed(), verdict != Verdict::Pass);
        }
    }

    #[test]
    fn gates_read_in_a_few_words() {
        assert_eq!(gate_label(&Gate::Recommended), "recommended");
        assert_eq!(
            gate_label(&Gate::EligibilityUnclear { why: "x".into() }),
            "eligibility unclear"
        );
        assert_eq!(
            gate_label(&Gate::Excluded {
                exclusion: Exclusion::BelowMinimum { why: "x".into() }
            }),
            "excluded (below minimum pay)"
        );
    }
}
