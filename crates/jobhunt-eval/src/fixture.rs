//! The benchmark's fixtures, as written in the repository, and the checks
//! that keep them honest.
//!
//! ```text
//! fixtures/recommendation/
//!   jobs/*.toml                    [[job]] postings, in a compact form of
//!                                  the canonical JobPosting
//!   candidates/<id>/candidate.toml the candidate: preferences (the
//!                                  canonical PreferenceValue), what the
//!                                  current model can't express, their
//!                                  taste profile ([taste], the candidate
//!                                  taste model of BRU-321), and one
//!                                  [[judgment]] per job in their pool
//!   candidates/<id>/resume.md      their resume, read by the production
//!                                  resume parser
//! ```
//!
//! Everything is synthetic or paraphrased from public postings: no real
//! person's data. A candidate is judged only on the jobs they have a
//! judgment for (their pool), so the benchmark measures candidate-job fit,
//! never a job's worth in general.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use jobhunt_jobs::Compensation;
use jobhunt_profile::taste::{Polarity, TasteDimension, normalize_value};
use jobhunt_profile::{PreferenceValue, Stance};
use serde::Deserialize;

use crate::taxonomy::{Fit, Label, Practicality, Reason, ReasonKind, TodayExpectation};

/// Why the fixtures can't be used.
#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    #[error("could not read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse {}: {message}", path.display())]
    Parse { path: PathBuf, message: String },
    #[error("the fixtures are inconsistent:\n  - {}", .0.join("\n  - "))]
    Invalid(Vec<String>),
}

/// Which part of the benchmark a job belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    /// The production failures the benchmark exists for.
    Golden,
    /// Jobs Narrow should surface.
    Positive,
    /// Pairs that differ in one meaningful way.
    Contrastive,
    /// Pay behaving as a practicality, never as fit.
    Compensation,
    /// Geography, relocation and time zones.
    Practicality,
}

impl Group {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Golden => "golden",
            Self::Positive => "positive",
            Self::Contrastive => "contrastive",
            Self::Compensation => "compensation",
            Self::Practicality => "practicality",
        }
    }
}

/// One posting, in a compact form of [`jobhunt_jobs::JobPosting`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobFixture {
    /// Stable, readable id (`airbnb-early-career`).
    pub id: String,
    pub group: Group,
    /// For contrastive jobs: the pair it belongs to (`postgres`).
    #[serde(default)]
    pub pair: Option<String>,
    /// For golden jobs: the candidate the production failure was observed
    /// on (their synthetic stand-in).
    #[serde(default)]
    pub observed_for: Option<String>,
    /// A source key (`greenhouse:airbnb`).
    pub source: String,
    pub company: String,
    pub title: String,
    #[serde(default)]
    pub department: Option<String>,
    #[serde(default)]
    pub team: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    /// `remote`, `hybrid` or `on_site`.
    #[serde(default)]
    pub workplace: Option<String>,
    #[serde(default)]
    pub remote: Option<bool>,
    /// `full_time`, `contract`, …
    #[serde(default)]
    pub employment: Option<String>,
    #[serde(default)]
    pub work_authorization: Option<String>,
    /// The canonical compensation, as a source adapter would store it.
    #[serde(default)]
    pub compensation: Option<Compensation>,
    pub description: String,
    /// Whether the listing was verified active before Today showed it
    /// (Today verifies its candidates first; default true).
    #[serde(default = "yes")]
    pub verified: bool,
    /// Where the posting comes from ("paraphrased from a public posting",
    /// "synthetic").
    pub provenance: String,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobFile {
    #[serde(default)]
    job: Vec<JobFixture>,
}

/// A preference as the candidate set it: the canonical value and stance.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreferenceFixture {
    pub stance: Stance,
    pub value: PreferenceValue,
}

/// One statement of a candidate's taste profile, as they would confirm it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasteStatementFixture {
    pub dimension: TasteDimension,
    /// A canonical value (`small_team`, `database_internals`).
    pub value: String,
    pub polarity: Polarity,
    /// As shown; the vocabulary's phrase when absent.
    #[serde(default)]
    pub text: Option<String>,
}

/// A candidate's taste profile: what they said they're looking for, and
/// the statements they confirmed. Loaded into the profile the ranker is
/// given; the current ranker doesn't read it (BRU-322 will), so it leaves
/// the baseline unchanged.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TasteFixture {
    /// Their answer to "What kind of job are you looking for?".
    pub looking_for: String,
    #[serde(default)]
    pub statement: Vec<TasteStatementFixture>,
}

/// The benchmark's expectation for one candidate and one job.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Judgment {
    pub job: String,
    pub fit: Fit,
    pub practicality: Practicality,
    pub reasons: Vec<Reason>,
    /// One or two sentences a reviewer can check the judgment against.
    pub why: String,
}

impl Judgment {
    pub fn label(&self) -> Label {
        Label::of(self.fit, self.practicality)
    }

    pub fn today(&self) -> TodayExpectation {
        TodayExpectation::of(self.fit, self.practicality)
    }

    /// Problems with the judgment itself: every judgment says why in
    /// structured reasons, and the reasons agree with the verdicts.
    pub fn problems(&self) -> Vec<String> {
        let kinds: Vec<ReasonKind> = self.reasons.iter().map(|r| r.kind()).collect();
        let has = |k: ReasonKind| kinds.contains(&k);
        let mut out = Vec::new();
        let mut problem = |text: &str| out.push(format!("{}: {text}", self.job));
        if !kinds.iter().any(|k| k.is_fit()) {
            problem("needs at least one fit reason");
        }
        if kinds.iter().all(|k| k.is_fit()) {
            problem("needs at least one practicality reason");
        }
        let unique: BTreeSet<&Reason> = self.reasons.iter().collect();
        if unique.len() != self.reasons.len() {
            problem("repeats a reason");
        }
        match self.fit {
            Fit::StrongYes => {
                if !has(ReasonKind::FitFor) {
                    problem("a strong yes needs affirmative fit evidence");
                }
                if has(ReasonKind::FitAgainst) || has(ReasonKind::FitThin) {
                    problem("a strong yes can't carry a fit contradiction or thin evidence");
                }
            }
            Fit::Maybe => {
                if !has(ReasonKind::FitAgainst) && !has(ReasonKind::FitThin) {
                    problem("a maybe needs to say what is missing or against it");
                }
            }
            Fit::No => {
                if !has(ReasonKind::FitAgainst) {
                    problem("a no needs a fit contradiction");
                }
            }
        }
        let practical = |k: &ReasonKind| !k.is_fit();
        match self.practicality {
            Practicality::Valid => {
                if kinds
                    .iter()
                    .filter(|k| practical(k))
                    .any(|k| *k != ReasonKind::PracticalValid)
                {
                    problem("valid practicality can only carry valid practical reasons");
                }
            }
            Practicality::Unknown => {
                if !has(ReasonKind::PracticalUnknown)
                    || has(ReasonKind::PracticalInvalid)
                    || has(ReasonKind::PracticalConcern)
                {
                    problem("unknown practicality needs an unknown, and nothing worse");
                }
            }
            Practicality::Concern => {
                if !has(ReasonKind::PracticalConcern) || has(ReasonKind::PracticalInvalid) {
                    problem("a practical concern needs a concern, and nothing invalid");
                }
            }
            Practicality::Impossible => {
                if !has(ReasonKind::PracticalInvalid) {
                    problem("impossible needs the condition that makes it so");
                }
            }
        }
        if self.why.trim().is_empty() {
            problem("needs a why");
        }
        out
    }
}

/// One synthetic candidate archetype.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateFixture {
    /// Also the directory name.
    pub id: String,
    /// Who they are, in a sentence.
    pub summary: String,
    /// What this candidate wants that Narrow's current preference model
    /// can't express (ownership depth, specialization, engineering shape):
    /// recorded for the preference redesign, not read by the ranker.
    #[serde(default)]
    pub unexpressed: Vec<String>,
    #[serde(default)]
    pub preference: Vec<PreferenceFixture>,
    /// The candidate taste profile, when the fixture has one.
    #[serde(default)]
    pub taste: Option<TasteFixture>,
    #[serde(default)]
    pub judgment: Vec<Judgment>,
    /// The resume, as the candidate would upload it.
    #[serde(skip)]
    pub resume: String,
}

/// Every fixture, loaded and checked.
#[derive(Debug, Clone)]
pub struct Fixtures {
    /// By id.
    pub jobs: BTreeMap<String, JobFixture>,
    /// In id order.
    pub candidates: Vec<CandidateFixture>,
}

fn read(path: &Path) -> Result<String, FixtureError> {
    std::fs::read_to_string(path).map_err(|source| FixtureError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn parse<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, FixtureError> {
    toml::from_str(&read(path)?).map_err(|e| FixtureError::Parse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

/// Entries of a directory, sorted, so loading never depends on the file
/// system's order.
fn entries(dir: &Path) -> Result<Vec<PathBuf>, FixtureError> {
    let io = |source| FixtureError::Io {
        path: dir.to_path_buf(),
        source,
    };
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(io)?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()
        .map_err(io)?;
    out.sort();
    Ok(out)
}

/// The fixtures shipped with the crate.
pub fn default_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/recommendation")
}

impl Fixtures {
    /// Reads and checks every fixture under `dir`.
    pub fn load(dir: &Path) -> Result<Self, FixtureError> {
        let mut jobs = BTreeMap::new();
        let mut problems = Vec::new();
        for path in entries(&dir.join("jobs"))? {
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let file: JobFile = parse(&path)?;
            for job in file.job {
                if let Some(earlier) = jobs.insert(job.id.clone(), job) {
                    problems.push(format!("job {} is defined twice", earlier.id));
                }
            }
        }
        let mut candidates = Vec::new();
        for path in entries(&dir.join("candidates"))? {
            if !path.is_dir() {
                continue;
            }
            let mut candidate: CandidateFixture = parse(&path.join("candidate.toml"))?;
            candidate.resume = read(&path.join("resume.md"))?;
            let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if candidate.id != dir_name {
                problems.push(format!(
                    "candidate {} lives in a directory named {dir_name}",
                    candidate.id
                ));
            }
            candidates.push(candidate);
        }
        let fixtures = Self { jobs, candidates };
        problems.extend(fixtures.problems());
        if problems.is_empty() {
            Ok(fixtures)
        } else {
            Err(FixtureError::Invalid(problems))
        }
    }

    /// Everything that makes the fixtures unusable as a benchmark.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.candidates.is_empty() {
            out.push("no candidates".to_owned());
        }
        for job in self.jobs.values() {
            out.extend(
                job_problems(job)
                    .into_iter()
                    .map(|p| format!("job {}: {p}", job.id)),
            );
        }
        let mut judged: BTreeSet<&str> = BTreeSet::new();
        for c in &self.candidates {
            let who = format!("candidate {}", c.id);
            if c.resume.trim().is_empty() {
                out.push(format!("{who}: empty resume"));
            }
            if c.judgment.is_empty() {
                out.push(format!("{who}: judges no job"));
            }
            let mut seen: BTreeSet<&str> = BTreeSet::new();
            for j in &c.judgment {
                if !self.jobs.contains_key(&j.job) {
                    out.push(format!("{who}: judges unknown job {}", j.job));
                }
                if !seen.insert(&j.job) {
                    out.push(format!("{who}: judges {} twice", j.job));
                }
                judged.insert(&j.job);
                out.extend(j.problems().into_iter().map(|p| format!("{who}: {p}")));
            }
            // A pool with nothing to find or nothing to avoid measures
            // only one side.
            if !c.judgment.iter().any(|j| j.label() == Label::StrongYes) {
                out.push(format!("{who}: no strong yes to find"));
            }
            if !c.judgment.iter().any(|j| j.label().is_obvious_mismatch()) {
                out.push(format!("{who}: no mismatch to avoid"));
            }
            if let Some(taste) = &c.taste {
                if taste.looking_for.trim().is_empty() {
                    out.push(format!("{who}: taste has no looking_for"));
                }
                let mut seen: BTreeSet<String> = BTreeSet::new();
                for s in &taste.statement {
                    if normalize_value(&s.value) != s.value {
                        out.push(format!(
                            "{who}: taste value {:?} is not a token ({:?})",
                            s.value,
                            normalize_value(&s.value)
                        ));
                    }
                    if !seen.insert(format!("{}:{}", s.dimension, s.value)) {
                        out.push(format!(
                            "{who}: taste {}:{} is stated twice",
                            s.dimension, s.value
                        ));
                    }
                }
            }
            let mut keys: BTreeSet<String> = BTreeSet::new();
            for p in &c.preference {
                // Two preferences with one key: the later would replace the
                // earlier, silently.
                if !keys.insert(p.value.key()) {
                    out.push(format!("{who}: preference {} is set twice", p.value.key()));
                }
            }
        }
        for id in self.jobs.keys() {
            if !judged.contains(id.as_str()) {
                out.push(format!("job {id}: no candidate judges it"));
            }
        }
        for job in self.jobs.values() {
            match (&job.observed_for, job.group) {
                (None, Group::Golden) => out.push(format!(
                    "job {}: a golden job names the candidate it was observed on",
                    job.id
                )),
                (Some(_), group) if group != Group::Golden => out.push(format!(
                    "job {}: only golden jobs are observed on a candidate",
                    job.id
                )),
                (Some(who), _)
                    if !self
                        .candidate(who)
                        .is_some_and(|c| c.judgment.iter().any(|j| j.job == job.id)) =>
                {
                    out.push(format!(
                        "job {}: observed on {who}, who doesn't judge it",
                        job.id
                    ));
                }
                _ => {}
            }
        }
        let mut pairs: BTreeMap<&str, usize> = BTreeMap::new();
        for job in self.jobs.values() {
            if let Some(pair) = &job.pair {
                *pairs.entry(pair).or_default() += 1;
            }
        }
        for (pair, n) in pairs {
            if n < 2 {
                out.push(format!("pair {pair} has only one side"));
            }
        }
        out
    }

    pub fn candidate(&self, id: &str) -> Option<&CandidateFixture> {
        self.candidates.iter().find(|c| c.id == id)
    }
}

fn job_problems(job: &JobFixture) -> Vec<String> {
    let mut out = Vec::new();
    if job.source.parse::<jobhunt_core::SourceKey>().is_err() {
        out.push(format!("source {:?} is not a source key", job.source));
    }
    if let Some(w) = &job.workplace
        && !matches!(w.as_str(), "remote" | "hybrid" | "on_site")
    {
        out.push(format!("workplace {w:?} is not remote, hybrid or on_site"));
    }
    if job.group == Group::Contrastive && job.pair.is_none() {
        out.push("a contrastive job names its pair".to_owned());
    }
    if job.description.trim().is_empty() {
        out.push("empty description".to_owned());
    }
    if job.provenance.trim().is_empty() {
        out.push("says nothing about where it comes from".to_owned());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn judgment(fit: Fit, practicality: Practicality, reasons: &[Reason]) -> Judgment {
        Judgment {
            job: "j".into(),
            fit,
            practicality,
            reasons: reasons.to_vec(),
            why: "because".into(),
        }
    }

    #[test]
    fn a_strong_yes_needs_affirmative_evidence_and_no_contradiction() {
        let ok = judgment(
            Fit::StrongYes,
            Practicality::Valid,
            &[Reason::SeniorityMatch, Reason::GeographyValid],
        );
        assert!(ok.problems().is_empty(), "{:?}", ok.problems());
        let thin = judgment(
            Fit::StrongYes,
            Practicality::Valid,
            &[Reason::WeakPositiveEvidence, Reason::GeographyValid],
        );
        assert!(!thin.problems().is_empty());
        let contradicted = judgment(
            Fit::StrongYes,
            Practicality::Valid,
            &[
                Reason::SeniorityMatch,
                Reason::SpecializationTooDeep,
                Reason::GeographyValid,
            ],
        );
        assert!(!contradicted.problems().is_empty());
        // Pay is no fit evidence.
        let paid = judgment(
            Fit::StrongYes,
            Practicality::Valid,
            &[Reason::CompensationGood, Reason::GeographyValid],
        );
        assert!(
            paid.problems()
                .iter()
                .any(|p| p.contains("affirmative fit evidence")),
            "{:?}",
            paid.problems()
        );
    }

    #[test]
    fn verdicts_need_matching_reasons() {
        let no_without_contradiction = judgment(
            Fit::No,
            Practicality::Valid,
            &[Reason::InsufficientFitEvidence, Reason::GeographyValid],
        );
        assert!(!no_without_contradiction.problems().is_empty());
        let impossible_without_condition = judgment(
            Fit::Maybe,
            Practicality::Impossible,
            &[Reason::InsufficientFitEvidence, Reason::GeographyUnclear],
        );
        assert!(!impossible_without_condition.problems().is_empty());
        let impossible = judgment(
            Fit::Maybe,
            Practicality::Impossible,
            &[
                Reason::LargeCompanyMismatch,
                Reason::WorkAuthorizationInvalid,
            ],
        );
        assert!(
            impossible.problems().is_empty(),
            "{:?}",
            impossible.problems()
        );
        let valid_with_unknown = judgment(
            Fit::StrongYes,
            Practicality::Valid,
            &[Reason::OwnershipMatch, Reason::CompensationUnknown],
        );
        assert!(!valid_with_unknown.problems().is_empty());
        let unknown = judgment(
            Fit::StrongYes,
            Practicality::Unknown,
            &[
                Reason::OwnershipMatch,
                Reason::GeographyValid,
                Reason::CompensationUnknown,
            ],
        );
        assert!(unknown.problems().is_empty(), "{:?}", unknown.problems());
        let no_practicality = judgment(Fit::No, Practicality::Valid, &[Reason::SeniorityMismatch]);
        assert!(
            no_practicality
                .problems()
                .iter()
                .any(|p| p.contains("practicality reason"))
        );
    }

    #[test]
    fn judgments_parse_from_toml() {
        let parsed: Judgment = toml::from_str(
            r#"
            job = "airbnb-early-career"
            fit = "no"
            practicality = "valid"
            reasons = ["seniority_mismatch", "geography_valid"]
            why = "An early-career role for a senior engineer."
            "#,
        )
        .unwrap();
        assert_eq!(parsed.label(), Label::No);
        assert_eq!(parsed.today(), TodayExpectation::Never);
        assert!(parsed.problems().is_empty());
        let unknown_reason = toml::from_str::<Judgment>(
            r#"
            job = "x"
            fit = "no"
            practicality = "valid"
            reasons = ["vibes"]
            why = "."
            "#,
        );
        assert!(unknown_reason.is_err(), "reasons come from the taxonomy");
    }

    #[test]
    fn preferences_are_the_canonical_values() {
        let p: PreferenceFixture = toml::from_str(
            r#"
            stance = "required"
            value = { type = "work_mode", mode = "remote" }
            "#,
        )
        .unwrap();
        assert_eq!(p.stance, Stance::Required);
        assert_eq!(
            p.value,
            PreferenceValue::WorkMode {
                mode: jobhunt_profile::WorkMode::Remote
            }
        );
        let pay: PreferenceFixture = toml::from_str(
            r#"
            stance = "wanted"
            value = { type = "compensation", bound = "target", amount = 180000, currency = "USD", period = "year" }
            "#,
        )
        .unwrap();
        assert!(matches!(
            pay.value,
            PreferenceValue::Compensation {
                amount: 180_000,
                ..
            }
        ));
    }
}
