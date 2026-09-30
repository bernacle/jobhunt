//! What a benchmark judgment says: fit and practicality apart, the label
//! they make together, the structured reasons behind them, and what Today
//! is expected to do with the job.
//!
//! Fit answers "would this candidate want this company and role?";
//! practicality answers "can they realistically pursue it?". They are never
//! collapsed: an excellent fit can be impossible, a weak fit can be well
//! paid, and pay is a practicality, never a reason for fit.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Would a reasonable candidate with this profile want this company and
/// role?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// Exactly the kind of role and company they would seriously consider.
    StrongYes,
    /// Plausible enough to inspect, but something meaningful is missing.
    Maybe,
    /// They would dismiss it: the role or company is meaningfully wrong.
    No,
}

/// Can the candidate realistically pursue it?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Practicality {
    /// Nothing practical stands in the way, as far as the posting says.
    Valid,
    /// Something practical is not stated (pay, where remote work is
    /// allowed): worth checking, not a reason to hide the job.
    Unknown,
    /// A known practical downside that doesn't rule it out (pay below the
    /// candidate's floor, under the current constraint semantics).
    Concern,
    /// A concrete condition makes it unactionable (US-only for someone not
    /// authorized there, mandatory relocation, an incompatible time zone).
    Impossible,
}

/// The benchmark's four-way judgment of one candidate and one job: fit,
/// unless practicality makes it impossible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Label {
    StrongYes,
    Maybe,
    No,
    Impossible,
}

impl Label {
    pub fn of(fit: Fit, practicality: Practicality) -> Self {
        match (practicality, fit) {
            (Practicality::Impossible, _) => Self::Impossible,
            (_, Fit::StrongYes) => Self::StrongYes,
            (_, Fit::Maybe) => Self::Maybe,
            (_, Fit::No) => Self::No,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::StrongYes => "Strong yes",
            Self::Maybe => "Maybe",
            Self::No => "No",
            Self::Impossible => "Impossible",
        }
    }

    /// Would the candidate dismiss it outright (the obvious false
    /// positives)?
    pub fn is_obvious_mismatch(self) -> bool {
        matches!(self, Self::No | Self::Impossible)
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What Today should do with a job, from its judgment.
///
/// | Judgment | Today |
/// | --- | --- |
/// | Strong yes, practicality valid or unknown | surface it |
/// | Strong yes, with a known practical concern | either is acceptable |
/// | Maybe | hold it back: a Maybe doesn't earn a "worth your attention" slot |
/// | No, Impossible | never |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodayExpectation {
    Surface,
    Either,
    Hold,
    Never,
}

impl TodayExpectation {
    pub fn of(fit: Fit, practicality: Practicality) -> Self {
        match Label::of(fit, practicality) {
            Label::StrongYes if practicality == Practicality::Concern => Self::Either,
            Label::StrongYes => Self::Surface,
            Label::Maybe => Self::Hold,
            Label::No | Label::Impossible => Self::Never,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Surface => "surface",
            Self::Either => "either",
            Self::Hold => "hold back",
            Self::Never => "never",
        }
    }
}

/// Whether a reason argues for or against, and about which question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReasonKind {
    /// Affirmative evidence of fit.
    FitFor,
    /// A fit contradiction.
    FitAgainst,
    /// Fit evidence is thin: one weak signal, or none.
    FitThin,
    /// A practical condition known to be met.
    PracticalValid,
    /// A practical condition the posting leaves unknown.
    PracticalUnknown,
    /// A known practical downside that doesn't rule the job out.
    PracticalConcern,
    /// A practical condition that rules the job out.
    PracticalInvalid,
}

impl ReasonKind {
    pub fn is_fit(self) -> bool {
        matches!(self, Self::FitFor | Self::FitAgainst | Self::FitThin)
    }
}

/// The structured reasons behind a judgment. Deliberately small.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    // Fit: affirmative evidence.
    SeniorityMatch,
    EngineeringWorkMatch,
    SpecializationMatch,
    CompanyShapeMatch,
    TeamShapeMatch,
    OwnershipMatch,
    DomainMatch,
    TechnicalDepthMatch,
    StartupMatch,
    // Fit: contradictions.
    SeniorityMismatch,
    EngineeringWorkMismatch,
    SpecializationTooDeep,
    CompanyShapeMismatch,
    TeamShapeMismatch,
    OwnershipMismatch,
    DomainMismatch,
    TechnicalDepthMismatch,
    LargeCompanyMismatch,
    // Fit: thin evidence.
    WeakPositiveEvidence,
    InsufficientFitEvidence,
    // Practicality.
    GeographyValid,
    GeographyInvalid,
    GeographyUnclear,
    WorkAuthorizationInvalid,
    RemoteScopeInvalid,
    RelocationInvalid,
    TimezoneInvalid,
    CompensationBelowFloor,
    /// Pay meets the candidate's floor but not their goal: a preference
    /// missed, still practical.
    CompensationBelowTarget,
    CompensationUnknown,
    CompensationGood,
    CompensationRangeNotApplicable,
    TravelOrOfficeUnclear,
}

impl Reason {
    pub const ALL: [Reason; 33] = [
        Self::SeniorityMatch,
        Self::EngineeringWorkMatch,
        Self::SpecializationMatch,
        Self::CompanyShapeMatch,
        Self::TeamShapeMatch,
        Self::OwnershipMatch,
        Self::DomainMatch,
        Self::TechnicalDepthMatch,
        Self::StartupMatch,
        Self::SeniorityMismatch,
        Self::EngineeringWorkMismatch,
        Self::SpecializationTooDeep,
        Self::CompanyShapeMismatch,
        Self::TeamShapeMismatch,
        Self::OwnershipMismatch,
        Self::DomainMismatch,
        Self::TechnicalDepthMismatch,
        Self::LargeCompanyMismatch,
        Self::WeakPositiveEvidence,
        Self::InsufficientFitEvidence,
        Self::GeographyValid,
        Self::GeographyInvalid,
        Self::GeographyUnclear,
        Self::WorkAuthorizationInvalid,
        Self::RemoteScopeInvalid,
        Self::RelocationInvalid,
        Self::TimezoneInvalid,
        Self::CompensationBelowFloor,
        Self::CompensationBelowTarget,
        Self::CompensationUnknown,
        Self::CompensationGood,
        Self::CompensationRangeNotApplicable,
        Self::TravelOrOfficeUnclear,
    ];

    pub fn kind(self) -> ReasonKind {
        use ReasonKind::*;
        match self {
            Self::SeniorityMatch
            | Self::EngineeringWorkMatch
            | Self::SpecializationMatch
            | Self::CompanyShapeMatch
            | Self::TeamShapeMatch
            | Self::OwnershipMatch
            | Self::DomainMatch
            | Self::TechnicalDepthMatch
            | Self::StartupMatch => FitFor,
            Self::SeniorityMismatch
            | Self::EngineeringWorkMismatch
            | Self::SpecializationTooDeep
            | Self::CompanyShapeMismatch
            | Self::TeamShapeMismatch
            | Self::OwnershipMismatch
            | Self::DomainMismatch
            | Self::TechnicalDepthMismatch
            | Self::LargeCompanyMismatch => FitAgainst,
            Self::WeakPositiveEvidence | Self::InsufficientFitEvidence => FitThin,
            Self::GeographyValid | Self::CompensationGood | Self::CompensationBelowTarget => {
                PracticalValid
            }
            Self::GeographyUnclear
            | Self::CompensationUnknown
            | Self::CompensationRangeNotApplicable
            | Self::TravelOrOfficeUnclear => PracticalUnknown,
            Self::CompensationBelowFloor => PracticalConcern,
            Self::GeographyInvalid
            | Self::WorkAuthorizationInvalid
            | Self::RemoteScopeInvalid
            | Self::RelocationInvalid
            | Self::TimezoneInvalid => PracticalInvalid,
        }
    }

    /// The contradiction category it belongs to, if it is one.
    pub fn contradiction(self) -> Option<Contradiction> {
        match self {
            Self::SeniorityMismatch => Some(Contradiction::Seniority),
            Self::SpecializationTooDeep
            | Self::TechnicalDepthMismatch
            | Self::EngineeringWorkMismatch => Some(Contradiction::RoleDepth),
            Self::GeographyInvalid
            | Self::WorkAuthorizationInvalid
            | Self::RemoteScopeInvalid
            | Self::RelocationInvalid
            | Self::TimezoneInvalid => Some(Contradiction::Eligibility),
            Self::CompanyShapeMismatch | Self::LargeCompanyMismatch | Self::TeamShapeMismatch => {
                Some(Contradiction::CompanyShape)
            }
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SeniorityMatch => "seniority_match",
            Self::EngineeringWorkMatch => "engineering_work_match",
            Self::SpecializationMatch => "specialization_match",
            Self::CompanyShapeMatch => "company_shape_match",
            Self::TeamShapeMatch => "team_shape_match",
            Self::OwnershipMatch => "ownership_match",
            Self::DomainMatch => "domain_match",
            Self::TechnicalDepthMatch => "technical_depth_match",
            Self::StartupMatch => "startup_match",
            Self::SeniorityMismatch => "seniority_mismatch",
            Self::EngineeringWorkMismatch => "engineering_work_mismatch",
            Self::SpecializationTooDeep => "specialization_too_deep",
            Self::CompanyShapeMismatch => "company_shape_mismatch",
            Self::TeamShapeMismatch => "team_shape_mismatch",
            Self::OwnershipMismatch => "ownership_mismatch",
            Self::DomainMismatch => "domain_mismatch",
            Self::TechnicalDepthMismatch => "technical_depth_mismatch",
            Self::LargeCompanyMismatch => "large_company_mismatch",
            Self::WeakPositiveEvidence => "weak_positive_evidence",
            Self::InsufficientFitEvidence => "insufficient_fit_evidence",
            Self::GeographyValid => "geography_valid",
            Self::GeographyInvalid => "geography_invalid",
            Self::GeographyUnclear => "geography_unclear",
            Self::WorkAuthorizationInvalid => "work_authorization_invalid",
            Self::RemoteScopeInvalid => "remote_scope_invalid",
            Self::RelocationInvalid => "relocation_invalid",
            Self::TimezoneInvalid => "timezone_invalid",
            Self::CompensationBelowFloor => "compensation_below_floor",
            Self::CompensationBelowTarget => "compensation_below_target",
            Self::CompensationUnknown => "compensation_unknown",
            Self::CompensationGood => "compensation_good",
            Self::CompensationRangeNotApplicable => "compensation_range_not_applicable",
            Self::TravelOrOfficeUnclear => "travel_or_office_unclear",
        }
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A contradiction that should keep a job out of Today however many
/// generic matches it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Contradiction {
    /// Early-career for a senior, or the other way round.
    Seniority,
    /// The work is materially deeper or of another kind than what the
    /// candidate has done (database internals for someone who used
    /// PostgreSQL; model training for someone who called LLM APIs).
    RoleDepth,
    /// Geography, work authorization, relocation or time zone.
    Eligibility,
    /// The company or team is the kind the candidate avoids.
    CompanyShape,
}

impl Contradiction {
    pub const ALL: [Contradiction; 4] = [
        Self::Seniority,
        Self::RoleDepth,
        Self::Eligibility,
        Self::CompanyShape,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Seniority => "seniority",
            Self::RoleDepth => "role depth",
            Self::Eligibility => "eligibility",
            Self::CompanyShape => "company shape",
        }
    }
}

impl fmt::Display for Contradiction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn practicality_decides_impossible_and_fit_the_rest() {
        assert_eq!(
            Label::of(Fit::StrongYes, Practicality::Impossible),
            Label::Impossible
        );
        assert_eq!(
            Label::of(Fit::No, Practicality::Impossible),
            Label::Impossible
        );
        assert_eq!(
            Label::of(Fit::StrongYes, Practicality::Unknown),
            Label::StrongYes
        );
        assert_eq!(
            Label::of(Fit::StrongYes, Practicality::Concern),
            Label::StrongYes
        );
        assert_eq!(Label::of(Fit::Maybe, Practicality::Valid), Label::Maybe);
        assert_eq!(Label::of(Fit::No, Practicality::Valid), Label::No);
    }

    #[test]
    fn today_expectations_follow_the_label() {
        use TodayExpectation::*;
        assert_eq!(
            TodayExpectation::of(Fit::StrongYes, Practicality::Valid),
            Surface
        );
        assert_eq!(
            TodayExpectation::of(Fit::StrongYes, Practicality::Unknown),
            Surface,
            "unknown pay or scope doesn't hold back a strong fit"
        );
        assert_eq!(
            TodayExpectation::of(Fit::StrongYes, Practicality::Concern),
            Either
        );
        assert_eq!(TodayExpectation::of(Fit::Maybe, Practicality::Valid), Hold);
        assert_eq!(TodayExpectation::of(Fit::No, Practicality::Valid), Never);
        assert_eq!(
            TodayExpectation::of(Fit::StrongYes, Practicality::Impossible),
            Never
        );
    }

    #[test]
    fn every_reason_has_one_kind_and_a_stable_name() {
        for reason in Reason::ALL {
            let name = reason.as_str();
            let parsed: Reason = toml::Value::String(name.to_owned())
                .try_into()
                .expect("the name parses back");
            assert_eq!(parsed, reason);
        }
        let names: std::collections::BTreeSet<&str> =
            Reason::ALL.iter().map(|r| r.as_str()).collect();
        assert_eq!(names.len(), Reason::ALL.len(), "names are unique");
    }

    #[test]
    fn pay_is_never_a_fit_reason() {
        for reason in [
            Reason::CompensationGood,
            Reason::CompensationBelowFloor,
            Reason::CompensationUnknown,
            Reason::CompensationRangeNotApplicable,
        ] {
            assert!(!reason.kind().is_fit(), "{reason}");
        }
    }

    #[test]
    fn contradictions_are_categorized() {
        assert_eq!(
            Reason::SeniorityMismatch.contradiction(),
            Some(Contradiction::Seniority)
        );
        assert_eq!(
            Reason::SpecializationTooDeep.contradiction(),
            Some(Contradiction::RoleDepth)
        );
        assert_eq!(
            Reason::EngineeringWorkMismatch.contradiction(),
            Some(Contradiction::RoleDepth)
        );
        assert_eq!(
            Reason::WorkAuthorizationInvalid.contradiction(),
            Some(Contradiction::Eligibility)
        );
        assert_eq!(
            Reason::LargeCompanyMismatch.contradiction(),
            Some(Contradiction::CompanyShape)
        );
        assert_eq!(Reason::CompensationBelowFloor.contradiction(), None);
        assert_eq!(Reason::InsufficientFitEvidence.contradiction(), None);
    }
}
