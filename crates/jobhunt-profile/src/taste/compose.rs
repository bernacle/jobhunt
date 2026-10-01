//! The taste profile as it stands: stored statements, the structured
//! preferences set earlier and learned patterns, merged by key with the
//! person's decisions first.
//!
//! Precedence for one `dimension:value` (the first present wins; the rest
//! that agree add their sources, the rest that disagree are kept as
//! `against`):
//!
//! 1. a stored statement the person reviewed or wrote (removed ones hide
//!    the key entirely);
//! 2. a stored reading of the person's words;
//! 3. a structured preference set earlier;
//! 4. a stored inference from the profile or from feedback;
//! 5. a learned pattern (shown in its own section, never as something the
//!    person said).
//!
//! The kind of work is special (BRU-324): once the person has chosen the
//! kinds of work they want ("What kind of role are you looking for?", or
//! a work shape they wrote, confirmed or set), kinds of work that were only
//! read or inferred no longer count as wanted. They stay, with their
//! provenance, as [`TasteProfile::supporting`]: what someone has done is
//! not what they want next. A reading that avoids what they chose ("avoid
//! AI" for someone choosing ML product work) is set aside
//! ([`TasteProfile::set_aside`]) rather than holding their choice back.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::aggregate::ProfileData;
use crate::ids::TasteId;

use super::{
    OriginalReading, Polarity, TasteAssertion, TasteConfidence, TasteDimension, TasteOrigin,
    TasteReview, TasteSource, roles, vocab,
};

/// A pattern learned from feedback, in taste terms (the app layer reads
/// ranking's learned taste into these).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedSignal {
    pub dimension: TasteDimension,
    pub value: String,
    /// `Prefer` or `Avoid`.
    pub polarity: Polarity,
    /// Tentative patterns are `Low`, established `Medium`, strong `High`.
    pub confidence: TasteConfidence,
    /// Ranking's key (`company_trait:startup`).
    pub pattern: String,
    /// "2 saves and 1 reason in your words".
    pub basis: String,
}

/// One statement of the composed profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComposedAssertion {
    /// The stored statement's id, or a stable id for a live one (a
    /// structured preference or a learned pattern) that becomes the stored
    /// id when the person acts on it.
    pub id: TasteId,
    pub dimension: TasteDimension,
    pub value: String,
    pub polarity: Polarity,
    pub text: String,
    pub confidence: TasteConfidence,
    pub origin: TasteOrigin,
    pub review: TasteReview,
    pub sources: Vec<TasteSource>,
    /// Sources that point the other way ("your earlier setting: avoid").
    pub against: Vec<TasteSource>,
    pub explanation: Option<String>,
    pub interpreter: Option<String>,
    pub original: Option<OriginalReading>,
    /// Stored in the profile (false: read live from a preference or from
    /// feedback).
    pub stored: bool,
}

impl ComposedAssertion {
    pub fn key(&self) -> String {
        super::key(self.dimension, &self.value)
    }

    /// The person's own: written, confirmed or corrected by them.
    pub fn is_persons(&self) -> bool {
        self.origin == TasteOrigin::Stated
            || matches!(self.review, TasteReview::Confirmed | TasteReview::Corrected)
    }

    /// Explicitly theirs: written, confirmed or corrected by them, or an
    /// earlier setting they entered themselves (not one the statement
    /// parser read from their words: that is Narrow's reading).
    pub fn is_explicit(&self) -> bool {
        self.is_persons()
            || (self.origin == TasteOrigin::Legacy
                && self.confidence == TasteConfidence::High
                && !self
                    .sources
                    .iter()
                    .any(|s| matches!(s, TasteSource::Words { .. })))
    }

    /// A firm part of the summary: the person's, or read from their words
    /// or settings with at least medium confidence. Learned patterns and
    /// weak inferences are not firm until the person confirms them.
    pub fn is_firm(&self) -> bool {
        self.is_persons()
            || (matches!(
                self.origin,
                TasteOrigin::Interpreted | TasteOrigin::Legacy | TasteOrigin::Profile
            ) && self.confidence >= TasteConfidence::Medium)
    }

    /// The statement, stored.
    pub fn to_stored(&self, now: DateTime<Utc>) -> TasteAssertion {
        TasteAssertion {
            id: self.id,
            dimension: self.dimension,
            value: self.value.clone(),
            polarity: self.polarity,
            text: self.text.clone(),
            confidence: self.confidence,
            origin: self.origin,
            review: self.review,
            sources: self.sources.clone(),
            explanation: self.explanation.clone(),
            interpreter: self.interpreter.clone(),
            original: self.original.clone(),
            superseded_by: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// The taste profile: the interface ranking (BRU-322) reads.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TasteProfile {
    /// Every statement in effect, in dimension order.
    pub assertions: Vec<ComposedAssertion>,
    /// Statements the person removed (keys that never come back).
    pub removed: Vec<ComposedAssertion>,
    /// Kinds of work read or inferred while the person chose the kinds of
    /// work they want: context, not wanted (see the module docs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supporting: Vec<ComposedAssertion>,
    /// Readings that avoid a kind of work the person chose: set aside, for
    /// them to settle, rather than holding their choice back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub set_aside: Vec<ComposedAssertion>,
}

impl TasteProfile {
    /// Statements about one dimension.
    pub fn about(&self, dimension: TasteDimension) -> impl Iterator<Item = &ComposedAssertion> {
        self.assertions
            .iter()
            .filter(move |a| a.dimension == dimension)
    }

    /// Values wanted (or fine) in a dimension, excluding learned-only ones.
    pub fn wanted(&self, dimension: TasteDimension) -> Vec<&str> {
        self.about(dimension)
            .filter(|a| {
                matches!(a.polarity, Polarity::Prefer | Polarity::Open)
                    && a.origin != TasteOrigin::Learned
            })
            .map(|a| a.value.as_str())
            .collect()
    }

    /// Values avoided in a dimension, excluding learned-only ones.
    pub fn avoided(&self, dimension: TasteDimension) -> Vec<&str> {
        self.about(dimension)
            .filter(|a| a.polarity == Polarity::Avoid && a.origin != TasteOrigin::Learned)
            .map(|a| a.value.as_str())
            .collect()
    }

    pub fn find(&self, id: TasteId) -> Option<&ComposedAssertion> {
        self.assertions
            .iter()
            .chain(&self.removed)
            .chain(&self.supporting)
            .chain(&self.set_aside)
            .find(|a| a.id == id)
    }

    /// The kinds of work the person chose: explicit `work_shape`
    /// statements they want. Empty when they haven't said.
    pub fn chosen_roles(&self) -> Vec<&ComposedAssertion> {
        self.about(TasteDimension::WorkShape)
            .filter(|a| a.polarity == Polarity::Prefer && a.is_explicit())
            .collect()
    }

    /// The role title in their words, when they gave one.
    pub fn role_title(&self) -> Option<&ComposedAssertion> {
        self.assertions
            .iter()
            .find(|a| roles::is_title(a.dimension, &a.value))
    }
}

/// The person's choice of the kinds of work they want comes first: what
/// was only read or inferred about the kind of work becomes supporting
/// context, and readings avoiding what they chose are set aside.
fn chosen_work_first(profile: &mut TasteProfile) {
    let chosen: Vec<String> = profile
        .chosen_roles()
        .iter()
        .map(|a| a.value.clone())
        .collect();
    if chosen.is_empty() {
        return;
    }
    let against_choice = |a: &ComposedAssertion| {
        a.polarity == Polarity::Avoid
            && matches!(
                a.dimension,
                TasteDimension::Domain | TasteDimension::Technology | TasteDimension::WorkShape
            )
            && chosen
                .iter()
                .any(|c| roles::contradicting(c).contains(&a.value.as_str()))
    };
    let mut kept = Vec::with_capacity(profile.assertions.len());
    for a in std::mem::take(&mut profile.assertions) {
        if a.is_explicit() {
            kept.push(a);
        } else if a.dimension == TasteDimension::WorkShape
            && matches!(a.polarity, Polarity::Prefer | Polarity::Open)
        {
            profile.supporting.push(a);
        } else if against_choice(&a) {
            profile.set_aside.push(a);
        } else {
            kept.push(a);
        }
    }
    profile.assertions = kept;
}

fn rank(a: &ComposedAssertion) -> u8 {
    if a.stored && (a.review.is_reviewed() || a.origin == TasteOrigin::Stated) {
        0
    } else if a.stored && a.origin == TasteOrigin::Interpreted {
        1
    } else if a.origin == TasteOrigin::Legacy {
        2
    } else if a.stored {
        3
    } else {
        4
    }
}

fn of_stored(a: &TasteAssertion) -> ComposedAssertion {
    ComposedAssertion {
        id: a.id,
        dimension: a.dimension,
        value: a.value.clone(),
        polarity: a.polarity,
        text: a.text.clone(),
        confidence: a.confidence,
        origin: a.origin,
        review: a.review,
        sources: a.sources.clone(),
        against: Vec::new(),
        explanation: a.explanation.clone(),
        interpreter: a.interpreter.clone(),
        original: a.original.clone(),
        stored: true,
    }
}

/// The id a live statement gets (and keeps once stored).
pub fn live_id(data: &ProfileData, origin: TasteOrigin, key: &str) -> TasteId {
    TasteId::derive(&[&data.id().to_string(), origin.as_str(), key])
}

/// Composes the taste profile of `data` with the learned patterns.
pub fn compose(data: &ProfileData, learned: &[LearnedSignal]) -> TasteProfile {
    let mut candidates: Vec<ComposedAssertion> = Vec::new();
    for a in data.taste.iter().filter(|a| a.superseded_by.is_none()) {
        candidates.push(of_stored(a));
    }
    for p in data.preferences.iter().filter(|p| p.active) {
        for read in vocab::from_preference(p) {
            let key = super::key(read.dimension, &read.value);
            candidates.push(ComposedAssertion {
                id: live_id(data, TasteOrigin::Legacy, &key),
                dimension: read.dimension,
                value: read.value,
                polarity: read.polarity,
                text: read.text,
                confidence: read.confidence,
                origin: TasteOrigin::Legacy,
                review: TasteReview::Unreviewed,
                sources: read.sources,
                against: Vec::new(),
                explanation: None,
                interpreter: None,
                original: None,
                stored: false,
            });
        }
    }
    for s in learned {
        let key = super::key(s.dimension, &s.value);
        candidates.push(ComposedAssertion {
            id: live_id(data, TasteOrigin::Learned, &key),
            dimension: s.dimension,
            value: s.value.clone(),
            polarity: s.polarity,
            text: vocab::sentence(s.dimension, &s.value, s.polarity),
            confidence: s.confidence,
            origin: TasteOrigin::Learned,
            review: TasteReview::Unreviewed,
            sources: vec![TasteSource::Feedback {
                pattern: s.pattern.clone(),
                text: s.basis.clone(),
            }],
            against: Vec::new(),
            explanation: None,
            interpreter: None,
            original: None,
            stored: false,
        });
    }
    // Keys a correction replaced: what Narrow had read never comes back.
    let corrected_away: Vec<String> = data
        .taste
        .iter()
        .filter(|a| a.is_active() && a.review == TasteReview::Corrected)
        .filter_map(|a| a.original.as_ref())
        .map(|o| super::key(o.dimension, &o.value))
        .collect();
    candidates.sort_by_key(rank);
    let mut by_key: BTreeMap<String, ComposedAssertion> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for c in candidates {
        let key = c.key();
        match by_key.get_mut(&key) {
            None => {
                if c.origin != TasteOrigin::Stated
                    && !c.review.is_reviewed()
                    && corrected_away.contains(&key)
                {
                    continue;
                }
                order.push(key.clone());
                by_key.insert(key, c);
            }
            Some(winner) => {
                let agrees = winner.polarity == c.polarity;
                for s in c.sources {
                    let list = if agrees {
                        &mut winner.sources
                    } else {
                        &mut winner.against
                    };
                    if !list.contains(&s) {
                        list.push(s);
                    }
                }
            }
        }
    }
    let mut profile = TasteProfile::default();
    for key in order {
        if let Some(a) = by_key.remove(&key) {
            if a.review == TasteReview::Removed {
                profile.removed.push(a);
            } else {
                profile.assertions.push(a);
            }
        }
    }
    let position = |d: TasteDimension| TasteDimension::ALL.iter().position(|x| *x == d);
    profile
        .assertions
        .sort_by_key(|a| (position(a.dimension), a.polarity));
    chosen_work_first(&mut profile);
    profile
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::{
        Certainty, CompanyTrait, Preference, PreferenceId, PreferenceOrigin, PreferenceValue,
        ProfileId, Stance,
    };

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 20, 0, 0, 0).unwrap()
    }

    fn pref(company: CompanyTrait, stance: Stance) -> Preference {
        let value = PreferenceValue::Company { company };
        Preference {
            id: PreferenceId::derive(&[&value.key()]),
            value,
            stance,
            origin: PreferenceOrigin::UserEntered,
            statement: None,
            snippet: None,
            certainty: Certainty::Certain,
            note: None,
            active: true,
            superseded_by: None,
            created_at: at(),
            updated_at: at(),
        }
    }

    fn stored(
        dimension: TasteDimension,
        value: &str,
        polarity: Polarity,
        origin: TasteOrigin,
        review: TasteReview,
    ) -> TasteAssertion {
        TasteAssertion {
            id: TasteId::derive(&[value, origin.as_str()]),
            dimension,
            value: value.into(),
            polarity,
            text: vocab::sentence(dimension, value, polarity),
            confidence: TasteConfidence::High,
            origin,
            review,
            sources: vec![TasteSource::Words {
                quote: value.into(),
                statement: None,
            }],
            explanation: None,
            interpreter: Some("rules/1".into()),
            original: None,
            superseded_by: None,
            created_at: at(),
            updated_at: at(),
        }
    }

    #[test]
    fn legacy_preferences_compose_live_and_merge_with_readings() {
        let mut data = ProfileData::new(ProfileId::local(), at());
        data.preferences
            .push(pref(CompanyTrait::SmallTeam, Stance::Wanted));
        data.preferences
            .push(pref(CompanyTrait::Startup, Stance::Wanted));
        data.taste.push(stored(
            TasteDimension::Team,
            "small_team",
            Polarity::Prefer,
            TasteOrigin::Interpreted,
            TasteReview::Unreviewed,
        ));
        let p = compose(&data, &[]);
        assert_eq!(p.assertions.len(), 2);
        let team = p.about(TasteDimension::Team).next().unwrap();
        assert!(team.stored, "the reading of the words wins");
        assert_eq!(team.sources.len(), 2, "the earlier setting agrees");
        let startup = p.about(TasteDimension::Company).next().unwrap();
        assert_eq!(startup.origin, TasteOrigin::Legacy);
        assert!(!startup.stored);
        assert_eq!(p.wanted(TasteDimension::Company), ["startup"]);
    }

    #[test]
    fn the_persons_decisions_win_and_removals_hide() {
        let mut data = ProfileData::new(ProfileId::local(), at());
        data.preferences
            .push(pref(CompanyTrait::LargeCompany, Stance::Wanted));
        data.taste.push(stored(
            TasteDimension::Company,
            "large_company",
            Polarity::Avoid,
            TasteOrigin::Interpreted,
            TasteReview::Confirmed,
        ));
        data.taste.push(stored(
            TasteDimension::Team,
            "small_team",
            Polarity::Prefer,
            TasteOrigin::Interpreted,
            TasteReview::Removed,
        ));
        let learned = [LearnedSignal {
            dimension: TasteDimension::Team,
            value: "small_team".into(),
            polarity: Polarity::Prefer,
            confidence: TasteConfidence::Medium,
            pattern: "company_trait:small_team".into(),
            basis: "3 saves".into(),
        }];
        let p = compose(&data, &learned);
        let large = p.about(TasteDimension::Company).next().unwrap();
        assert_eq!(large.polarity, Polarity::Avoid);
        assert_eq!(large.against.len(), 1, "the disagreeing setting is kept");
        assert!(p.about(TasteDimension::Team).next().is_none(), "removed");
        assert_eq!(p.removed.len(), 1);
    }

    #[test]
    fn learned_patterns_are_not_firm() {
        let data = ProfileData::new(ProfileId::local(), at());
        let p = compose(
            &data,
            &[LearnedSignal {
                dimension: TasteDimension::Company,
                value: "startup".into(),
                polarity: Polarity::Prefer,
                confidence: TasteConfidence::High,
                pattern: "company_trait:startup".into(),
                basis: "5 applications".into(),
            }],
        );
        let a = &p.assertions[0];
        assert_eq!(a.origin, TasteOrigin::Learned);
        assert!(!a.is_firm());
        assert!(p.wanted(TasteDimension::Company).is_empty());
    }
}
