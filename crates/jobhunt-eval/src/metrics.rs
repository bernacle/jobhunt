//! The benchmark's measurements, each reported on its own. There is no
//! aggregate score: one number would hide whether obvious mismatches are
//! gone, whether good jobs still surface, and how precise Today is.
//!
//! "Surfaced" means labeled worth the candidate's attention (it qualifies
//! for Today, see [`crate::run::qualifies`]).
//!
//! * **Strict precision**: surfaced jobs judged Strong yes.
//! * **Relaxed precision**: surfaced jobs judged Strong yes or Maybe.
//! * **Actionable precision**: surfaced jobs that fit enough (Strong yes or
//!   Maybe) *and* have no known practical problem (practicality valid or
//!   merely unknown).
//! * **Obvious false positives**: surfaced jobs judged No or Impossible.
//!   Should be zero.
//! * **Strong yes surfaced**: of the practical Strong yeses, how many
//!   surfaced. Guards against a Today that is precise because it is empty.
//! * **Contradiction misses**, per kind: jobs whose judgment names the
//!   contradiction but surfaced anyway; and how many of them the ranker has
//!   no signal for at all.
//! * **Density**: how many jobs surfaced, and how many the feed shows.
//! * **Surfaced on pay**: surfaced jobs that would not be without the
//!   candidate's pay preferences.
//! * **Foreign pay ranges read as met**: of the jobs whose published range
//!   is for another location, how many the ranker read as meeting the
//!   candidate's pay.
//! * **Surfaced with eligibility unconfirmed**: surfaced jobs nothing
//!   confirms the candidate can take (descriptive, not a failure).

use std::fmt;

use crate::run::Case;
use crate::taxonomy::{Contradiction, Fit, Label, Practicality, Reason, TodayExpectation};

/// `hits` of `of`, shown with its percentage; nothing to measure is said
/// so, never shown as 0% or 100%.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ratio {
    pub hits: usize,
    pub of: usize,
}

impl Ratio {
    pub fn new(hits: usize, of: usize) -> Self {
        Self { hits, of }
    }

    /// `None` when there is nothing to measure.
    pub fn value(self) -> Option<f64> {
        (self.of > 0).then(|| self.hits as f64 / self.of as f64)
    }
}

impl fmt::Display for Ratio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.value() {
            Some(v) => write!(f, "{}/{} ({:.0}%)", self.hits, self.of, v * 100.0),
            None => write!(f, "n/a (0/0)"),
        }
    }
}

/// One kind of contradiction across cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContradictionMetric {
    pub kind: Contradiction,
    /// Cases whose judgment names it.
    pub cases: usize,
    /// Of those, surfaced anyway: the miss rate is `surfaced / cases`.
    pub surfaced: usize,
    /// Of those, the ranker has no negative signal for it.
    pub undetected: usize,
}

impl ContradictionMetric {
    pub fn miss_rate(&self) -> Ratio {
        Ratio::new(self.surfaced, self.cases)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    pub judged: usize,
    pub surfaced: usize,
    /// On the simulated feed (one per company, at most five).
    pub on_feed: usize,
    pub strict_precision: Ratio,
    pub relaxed_precision: Ratio,
    pub actionable_precision: Ratio,
    pub obvious_false_positives: Ratio,
    pub maybes_surfaced: usize,
    pub strong_yes_surfaced: Ratio,
    pub feed_strict_precision: Ratio,
    pub feed_relaxed_precision: Ratio,
    pub contradictions: Vec<ContradictionMetric>,
    pub surfaced_on_pay: usize,
    pub foreign_range_read_as_met: Ratio,
    pub surfaced_unconfirmed: usize,
    /// Cases whose verdict is a failure.
    pub failures: usize,
}

impl Metrics {
    pub fn of<'a>(cases: impl IntoIterator<Item = &'a Case>) -> Self {
        let cases: Vec<&Case> = cases.into_iter().collect();
        let surfaced: Vec<&Case> = cases.iter().copied().filter(|c| c.surfaced()).collect();
        let fed: Vec<&Case> = cases
            .iter()
            .copied()
            .filter(|c| c.observed.in_feed)
            .collect();
        let count = |set: &[&Case], f: &dyn Fn(&Case) -> bool| set.iter().filter(|c| f(c)).count();
        let strong = |c: &Case| c.label() == Label::StrongYes;
        let acceptable = |c: &Case| matches!(c.label(), Label::StrongYes | Label::Maybe);
        let actionable = |c: &Case| {
            matches!(c.judgment.fit, Fit::StrongYes | Fit::Maybe)
                && matches!(
                    c.judgment.practicality,
                    Practicality::Valid | Practicality::Unknown
                )
        };
        let expected: Vec<&Case> = cases
            .iter()
            .copied()
            .filter(|c| c.judgment.today() == TodayExpectation::Surface)
            .collect();
        let contradictions = Contradiction::ALL
            .iter()
            .map(|kind| {
                let named: Vec<&Case> = cases
                    .iter()
                    .copied()
                    .filter(|c| c.contradictions().contains(kind))
                    .collect();
                ContradictionMetric {
                    kind: *kind,
                    cases: named.len(),
                    surfaced: count(&named, &|c| c.surfaced()),
                    undetected: count(&named, &|c| c.undetected().contains(kind)),
                }
            })
            .collect();
        Self {
            judged: cases.len(),
            surfaced: surfaced.len(),
            on_feed: fed.len(),
            strict_precision: Ratio::new(count(&surfaced, &strong), surfaced.len()),
            relaxed_precision: Ratio::new(count(&surfaced, &acceptable), surfaced.len()),
            actionable_precision: Ratio::new(count(&surfaced, &actionable), surfaced.len()),
            obvious_false_positives: Ratio::new(
                count(&surfaced, &|c| c.label().is_obvious_mismatch()),
                surfaced.len(),
            ),
            maybes_surfaced: count(&surfaced, &|c| c.label() == Label::Maybe),
            strong_yes_surfaced: Ratio::new(count(&expected, &|c| c.surfaced()), expected.len()),
            feed_strict_precision: Ratio::new(count(&fed, &strong), fed.len()),
            feed_relaxed_precision: Ratio::new(count(&fed, &acceptable), fed.len()),
            contradictions,
            surfaced_on_pay: count(&surfaced, &|c| c.observed.pay_carried),
            foreign_range_read_as_met: {
                let foreign: Vec<&Case> = cases
                    .iter()
                    .copied()
                    .filter(|c| {
                        c.judgment
                            .reasons
                            .contains(&Reason::CompensationRangeNotApplicable)
                    })
                    .collect();
                Ratio::new(
                    count(&foreign, &|c| c.observed.pay_read_as_met),
                    foreign.len(),
                )
            },
            surfaced_unconfirmed: count(&surfaced, &|c| c.observed.eligibility_unconfirmed),
            failures: count(&cases, &|c| c.verdict().failed()),
        }
    }

    pub fn contradiction(&self, kind: Contradiction) -> Option<&ContradictionMetric> {
        self.contradictions.iter().find(|m| m.kind == kind)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::fixture::{Group, Judgment};
    use crate::run::{Observed, Verdict};
    use jobhunt_eligibility::Eligibility;
    use jobhunt_ranking::Tier;

    /// A case with a judgment and whether it surfaced.
    pub(crate) fn case(
        fit: Fit,
        practicality: Practicality,
        reasons: &[Reason],
        surfaced: bool,
    ) -> Case {
        Case {
            job: "job".into(),
            title: "Engineer".into(),
            company: "Acme".into(),
            group: Group::Positive,
            pair: None,
            as_observed: false,
            judgment: Judgment {
                job: "job".into(),
                fit,
                practicality,
                reasons: reasons.to_vec(),
                why: "test".into(),
            },
            observed: Observed {
                gate: "recommended".into(),
                gate_why: None,
                eligibility: Eligibility::Eligible,
                tier: if surfaced {
                    Tier::StrongFit
                } else {
                    Tier::Maybe
                },
                score: 0.0,
                fit: String::new(),
                against: Vec::new(),
                practical: String::new(),
                qualifies: surfaced,
                in_feed: surfaced,
                worth: Vec::new(),
                caveats: Vec::new(),
                unknowns: Vec::new(),
                detected: Vec::new(),
                pay_carried: false,
                pay_read_as_met: false,
                eligibility_unconfirmed: false,
            },
        }
    }

    fn strong(surfaced: bool) -> Case {
        case(
            Fit::StrongYes,
            Practicality::Valid,
            &[Reason::OwnershipMatch, Reason::GeographyValid],
            surfaced,
        )
    }

    #[test]
    fn precision_counts_only_what_surfaced() {
        let cases = [
            strong(true),
            strong(true),
            strong(false),
            case(
                Fit::Maybe,
                Practicality::Valid,
                &[Reason::InsufficientFitEvidence, Reason::GeographyValid],
                true,
            ),
            case(
                Fit::No,
                Practicality::Valid,
                &[Reason::SeniorityMismatch, Reason::GeographyValid],
                true,
            ),
            case(
                Fit::No,
                Practicality::Valid,
                &[Reason::SeniorityMismatch, Reason::GeographyValid],
                false,
            ),
        ];
        let m = Metrics::of(&cases);
        assert_eq!(m.judged, 6);
        assert_eq!(m.surfaced, 4);
        assert_eq!(m.strict_precision, Ratio::new(2, 4));
        assert_eq!(m.relaxed_precision, Ratio::new(3, 4));
        assert_eq!(m.obvious_false_positives, Ratio::new(1, 4));
        assert_eq!(m.maybes_surfaced, 1);
        assert_eq!(m.strong_yes_surfaced, Ratio::new(2, 3));
        assert_eq!(m.failures, 3, "one false positive, one maybe, one miss");
    }

    #[test]
    fn an_empty_today_is_not_perfect_precision() {
        let cases = [strong(false), strong(false)];
        let m = Metrics::of(&cases);
        assert_eq!(m.surfaced, 0);
        assert_eq!(m.strict_precision.value(), None);
        assert_eq!(m.strict_precision.to_string(), "n/a (0/0)");
        assert_eq!(
            m.strong_yes_surfaced,
            Ratio::new(0, 2),
            "the empty feed shows up as missed strong yeses"
        );
    }

    #[test]
    fn actionable_precision_leaves_out_impossible_and_known_concerns() {
        let impossible = case(
            Fit::StrongYes,
            Practicality::Impossible,
            &[
                Reason::EngineeringWorkMatch,
                Reason::WorkAuthorizationInvalid,
            ],
            true,
        );
        let concern = case(
            Fit::StrongYes,
            Practicality::Concern,
            &[Reason::EngineeringWorkMatch, Reason::CompensationBelowFloor],
            true,
        );
        let unknown_pay = case(
            Fit::StrongYes,
            Practicality::Unknown,
            &[Reason::EngineeringWorkMatch, Reason::CompensationUnknown],
            true,
        );
        let m = Metrics::of([&impossible, &concern, &unknown_pay]);
        assert_eq!(m.actionable_precision, Ratio::new(1, 3));
        assert_eq!(m.relaxed_precision, Ratio::new(2, 3));
        assert_eq!(m.obvious_false_positives, Ratio::new(1, 3));
        assert_eq!(impossible.verdict(), Verdict::FalsePositive);
        assert_eq!(concern.verdict(), Verdict::Pass, "either is acceptable");
        assert_eq!(unknown_pay.verdict(), Verdict::Pass);
    }

    #[test]
    fn contradictions_are_counted_by_kind_with_what_the_ranker_saw() {
        let mut seniority = case(
            Fit::No,
            Practicality::Valid,
            &[Reason::SeniorityMismatch, Reason::GeographyValid],
            true,
        );
        seniority.observed.detected = vec![Contradiction::Seniority];
        let depth = case(
            Fit::No,
            Practicality::Valid,
            &[
                Reason::SpecializationTooDeep,
                Reason::TechnicalDepthMismatch,
                Reason::GeographyValid,
            ],
            true,
        );
        let geography = case(
            Fit::Maybe,
            Practicality::Impossible,
            &[Reason::InsufficientFitEvidence, Reason::RemoteScopeInvalid],
            false,
        );
        let m = Metrics::of([&seniority, &depth, &geography]);
        let s = m.contradiction(Contradiction::Seniority).unwrap();
        assert_eq!((s.cases, s.surfaced, s.undetected), (1, 1, 0));
        let d = m.contradiction(Contradiction::RoleDepth).unwrap();
        assert_eq!(
            (d.cases, d.surfaced, d.undetected),
            (1, 1, 1),
            "two role-depth reasons are one contradiction"
        );
        let e = m.contradiction(Contradiction::Eligibility).unwrap();
        assert_eq!((e.cases, e.surfaced), (1, 0));
        assert_eq!(e.miss_rate(), Ratio::new(0, 1));
        assert_eq!(depth.missed(), [Contradiction::RoleDepth]);
        assert!(geography.missed().is_empty());
    }

    #[test]
    fn pay_carried_and_feed_are_counted_apart() {
        let mut paid = case(
            Fit::Maybe,
            Practicality::Valid,
            &[Reason::WeakPositiveEvidence, Reason::CompensationGood],
            true,
        );
        paid.observed.pay_carried = true;
        paid.observed.in_feed = false;
        let mut foreign = case(
            Fit::StrongYes,
            Practicality::Unknown,
            &[
                Reason::OwnershipMatch,
                Reason::CompensationRangeNotApplicable,
            ],
            true,
        );
        foreign.observed.pay_read_as_met = true;
        foreign.observed.eligibility_unconfirmed = true;
        let m = Metrics::of([&paid, &strong(true)]);
        assert_eq!(m.surfaced_on_pay, 1);
        assert_eq!(m.foreign_range_read_as_met, Ratio::new(0, 0));
        let with_foreign = Metrics::of([&paid, &foreign]);
        assert_eq!(with_foreign.foreign_range_read_as_met, Ratio::new(1, 1));
        assert_eq!(with_foreign.surfaced_unconfirmed, 1);
        assert_eq!(m.on_feed, 1);
        assert_eq!(m.feed_strict_precision, Ratio::new(1, 1));
        assert_eq!(m.strict_precision, Ratio::new(1, 2));
    }
}
