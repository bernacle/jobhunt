//! Changing the taste profile: storing a reading of the person's words,
//! and the person's decisions about it. Pure functions over
//! [`ProfileData`]; [`crate::ProfileService::change_taste`] stores the
//! result.
//!
//! The person's decisions are authoritative:
//!
//! * a new reading replaces only Narrow's earlier unreviewed readings;
//!   it never adds a statement about a key the person confirmed,
//!   corrected, wrote or removed, nor the key a correction replaced;
//! * a correction keeps what Narrow had read ([`TasteAssertion::original`])
//!   and becomes the person's statement;
//! * a removal is kept as a tombstone, so nothing brings the key back;
//! * none of this is touched by resume, LinkedIn or GitHub imports, which
//!   never write taste.

use chrono::{DateTime, Utc};

use crate::aggregate::ProfileData;
use crate::ids::{PreferenceId, StatementId, TasteId};

use super::compose::{ComposedAssertion, TasteProfile};
use super::reading::{TasteReading, settled_keys};
use super::{
    Interpretation, InterpretationOutcome, OriginalReading, Polarity, TasteAssertion, TasteBrief,
    TasteConfidence, TasteDimension, TasteOrigin, TasteReview, TasteSource, normalize_value, vocab,
};

/// Why an edit can't be made.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TasteEditError {
    #[error("no taste statement {0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
}

fn upsert(data: &mut ProfileData, a: TasteAssertion) {
    match data.taste.iter_mut().find(|x| x.id == a.id) {
        Some(existing) => *existing = a,
        None => data.taste.push(a),
    }
}

/// Sets what the person is looking for, in their words. Returns whether it
/// changed. A new text needs a new confirmation.
pub fn set_brief(
    data: &mut ProfileData,
    text: &str,
    statement: Option<StatementId>,
    now: DateTime<Utc>,
) -> bool {
    let id = TasteBrief::id_for(data.id());
    match &mut data.taste_brief {
        Some(brief) if brief.text == text => {
            if brief.statement.is_none() && statement.is_some() {
                brief.statement = statement;
                brief.updated_at = now;
            }
            false
        }
        Some(brief) => {
            brief.text = text.to_owned();
            brief.statement = statement;
            brief.confirmed_at = None;
            brief.updated_at = now;
            true
        }
        None => {
            data.taste_brief = Some(TasteBrief {
                id,
                text: text.to_owned(),
                statement,
                interpretation: None,
                confirmed_at: None,
                created_at: now,
                updated_at: now,
            });
            true
        }
    }
}

/// What storing a reading did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    pub added: usize,
    /// Readings dropped because the person already decided about the key.
    pub kept_persons: usize,
    /// Earlier unreviewed readings replaced.
    pub replaced: usize,
}

/// Stores a reading of the brief: Narrow's earlier unreviewed readings are
/// replaced, the person's decisions are left alone, and the brief records
/// the interpretation.
pub fn apply_reading(
    data: &mut ProfileData,
    reading: &TasteReading,
    outcome: InterpretationOutcome,
    note: Option<String>,
    input_digest: String,
    now: DateTime<Utc>,
) -> Applied {
    let mut applied = Applied::default();
    let before = data.taste.len();
    data.taste.retain(|a| {
        a.origin == TasteOrigin::Stated || a.review.is_reviewed() || a.superseded_by.is_some()
    });
    applied.replaced = before - data.taste.len();
    let settled = settled_keys(&data.taste);
    let profile = data.id().to_string();
    for read in &reading.assertions {
        let key = read.key();
        if settled.contains(&key) {
            applied.kept_persons += 1;
            continue;
        }
        applied.added += 1;
        upsert(
            data,
            TasteAssertion {
                id: TasteId::derive(&[&profile, "read", &key]),
                dimension: read.dimension,
                value: read.value.clone(),
                polarity: read.polarity,
                text: read.text.clone(),
                confidence: read.confidence,
                origin: read.origin,
                review: TasteReview::Unreviewed,
                sources: read.sources.clone(),
                explanation: read.explanation.clone(),
                interpreter: Some(reading.interpreter.clone()),
                original: None,
                superseded_by: None,
                created_at: now,
                updated_at: now,
            },
        );
    }
    if let Some(brief) = &mut data.taste_brief {
        brief.interpretation = Some(Interpretation {
            interpreter: reading.interpreter.clone(),
            outcome,
            input_digest,
            at: now,
            note,
            summary: reading.summary.clone(),
            ambiguities: reading.ambiguities.clone(),
            constraints_noted: reading.constraints_noted.clone(),
            rejected: reading.rejected,
        });
        brief.updated_at = now;
    }
    applied
}

/// Structured preferences behind a statement: a decision about the
/// statement is a decision about them too.
fn preferences_of(a: &ComposedAssertion) -> Vec<PreferenceId> {
    a.sources
        .iter()
        .filter_map(|s| match s {
            TasteSource::Preference { preference, .. } => Some(*preference),
            _ => None,
        })
        .collect()
}

fn active(profile: &TasteProfile, id: TasteId) -> Result<&ComposedAssertion, TasteEditError> {
    profile
        .assertions
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| TasteEditError::NotFound(id.to_string()))
}

fn with_person(mut sources: Vec<TasteSource>) -> Vec<TasteSource> {
    if !sources.contains(&TasteSource::Person) {
        sources.push(TasteSource::Person);
    }
    sources
}

/// Confirms statements: `ids`, or with none every firm statement of the
/// summary Narrow read and the person hasn't reviewed (not learned
/// patterns, which are confirmed one by one). Returns how many changed.
pub fn confirm(
    data: &mut ProfileData,
    profile: &TasteProfile,
    ids: &[TasteId],
    now: DateTime<Utc>,
) -> Result<usize, TasteEditError> {
    let targets: Vec<&ComposedAssertion> = if ids.is_empty() {
        profile
            .assertions
            .iter()
            .filter(|a| !a.is_persons() && a.is_firm())
            .collect()
    } else {
        ids.iter()
            .map(|id| active(profile, *id))
            .collect::<Result<_, _>>()?
    };
    let mut changed = 0;
    for a in targets {
        if a.is_persons() {
            continue;
        }
        let mut stored = match data.taste.iter().find(|x| x.id == a.id) {
            Some(x) => x.clone(),
            None => a.to_stored(now),
        };
        stored.review = TasteReview::Confirmed;
        stored.sources = with_person(a.sources.clone());
        stored.updated_at = now;
        upsert(data, stored);
        changed += 1;
    }
    if ids.is_empty()
        && let Some(brief) = &mut data.taste_brief
    {
        brief.confirmed_at = Some(now);
        brief.updated_at = now;
    }
    Ok(changed)
}

/// What a correction or a removal did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decided {
    /// The statements in effect now (none for a removal).
    pub now: Vec<TasteId>,
    /// Structured preferences behind the old statement, for the caller to
    /// bring in line (removed, or given the new stance).
    pub preferences: Vec<PreferenceId>,
    /// The polarity those preferences should take (`None`: remove them).
    pub polarity: Option<Polarity>,
}

/// Corrects a statement: a new polarity ("I don't care about this" is
/// [`Polarity::Neutral`]), new words, or both. New words are read by the
/// caller (`reading`, possibly empty) and replace the statement.
pub fn correct(
    data: &mut ProfileData,
    profile: &TasteProfile,
    id: TasteId,
    polarity: Option<Polarity>,
    text: Option<&str>,
    reading: Option<&TasteReading>,
    now: DateTime<Utc>,
) -> Result<Decided, TasteEditError> {
    let a = active(profile, id)?.clone();
    let text = text
        .map(str::trim)
        .filter(|t| !t.is_empty() && *t != a.text);
    if text.is_none() && polarity.is_none_or(|p| p == a.polarity) {
        return Err(TasteEditError::Invalid(
            "nothing to change: give new words or another polarity".into(),
        ));
    }
    let original = a.original.clone().or_else(|| {
        (!a.is_persons()).then(|| OriginalReading {
            dimension: a.dimension,
            value: a.value.clone(),
            polarity: a.polarity,
            text: a.text.clone(),
            origin: a.origin,
        })
    });
    let preferences = preferences_of(&a);
    let base = match data.taste.iter().find(|x| x.id == a.id) {
        Some(x) => x.clone(),
        None => a.to_stored(now),
    };
    let Some(text) = text else {
        // Only the polarity changes: the same statement, the person's now.
        let polarity = polarity.unwrap_or(a.polarity);
        let generated = a.text == vocab::sentence(a.dimension, &a.value, a.polarity)
            || a.text
                .starts_with(&vocab::sentence(a.dimension, &a.value, a.polarity));
        let mut stored = base;
        stored.polarity = polarity;
        if generated {
            stored.text = vocab::sentence(a.dimension, &a.value, polarity);
        }
        stored.review = TasteReview::Corrected;
        stored.original = original;
        stored.sources = with_person(a.sources.clone());
        stored.updated_at = now;
        upsert(data, stored);
        return Ok(Decided {
            now: vec![id],
            preferences,
            polarity: Some(polarity).filter(|p| *p != Polarity::Neutral),
        });
    };
    let mut reads: Vec<(TasteDimension, String, Polarity, TasteConfidence, String)> = reading
        .map(|r| {
            r.assertions
                .iter()
                .map(|x| {
                    (
                        x.dimension,
                        x.value.clone(),
                        polarity.unwrap_or(x.polarity),
                        TasteConfidence::High,
                        x.text.clone(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if reads.is_empty() {
        let value = normalize_value(text);
        if value.is_empty() {
            return Err(TasteEditError::Invalid("say it in a few words".into()));
        }
        reads.push((
            a.dimension,
            value,
            polarity.unwrap_or(a.polarity),
            TasteConfidence::High,
            text.to_owned(),
        ));
    }
    let single = reads.len() == 1;
    let profile_id = data.id().to_string();
    let mut ids = Vec::new();
    for (dimension, value, p, confidence, read_text) in reads {
        let key = super::key(dimension, &value);
        let same = key == a.key();
        let new_id = if same {
            a.id
        } else {
            TasteId::derive(&[&profile_id, "stated", &key, &now.to_rfc3339()])
        };
        let mut sources = vec![TasteSource::Words {
            quote: text.to_owned(),
            statement: None,
        }];
        if same {
            sources.extend(a.sources.iter().cloned());
        }
        upsert(
            data,
            TasteAssertion {
                id: new_id,
                dimension,
                value,
                polarity: p,
                text: if single { text.to_owned() } else { read_text },
                confidence,
                origin: TasteOrigin::Stated,
                review: TasteReview::Corrected,
                sources: with_person(sources),
                explanation: None,
                interpreter: reading.map(|r| r.interpreter.clone()),
                original: original.clone(),
                superseded_by: None,
                created_at: now,
                updated_at: now,
            },
        );
        ids.push(new_id);
    }
    if !ids.contains(&a.id) {
        let mut old = base;
        old.review = TasteReview::Removed;
        old.superseded_by = ids.first().copied();
        old.updated_at = now;
        upsert(data, old);
    }
    let still_same = ids.contains(&a.id);
    Ok(Decided {
        now: ids,
        preferences,
        polarity: if still_same {
            polarity.filter(|p| *p != Polarity::Neutral)
        } else {
            None
        },
    })
}

/// Removes a statement; it stays as a tombstone so it never comes back.
pub fn remove(
    data: &mut ProfileData,
    profile: &TasteProfile,
    id: TasteId,
    now: DateTime<Utc>,
) -> Result<Decided, TasteEditError> {
    let a = active(profile, id)?.clone();
    let mut stored = match data.taste.iter().find(|x| x.id == a.id) {
        Some(x) => x.clone(),
        None => a.to_stored(now),
    };
    stored.review = TasteReview::Removed;
    stored.sources = with_person(a.sources.clone());
    stored.updated_at = now;
    upsert(data, stored);
    Ok(Decided {
        now: Vec::new(),
        preferences: preferences_of(&a),
        polarity: None,
    })
}

/// Adds the person's own sentence, read by the caller into `reading`
/// (possibly empty: then it is kept as written). A statement about a key
/// already in the profile replaces it: the person's latest words win.
pub fn add(
    data: &mut ProfileData,
    text: &str,
    reading: &TasteReading,
    now: DateTime<Utc>,
) -> Result<Vec<TasteId>, TasteEditError> {
    let text = text.trim();
    let mut reads: Vec<(TasteDimension, String, Polarity, String)> = reading
        .assertions
        .iter()
        .map(|x| (x.dimension, x.value.clone(), x.polarity, x.text.clone()))
        .collect();
    if reads.is_empty() {
        let value = normalize_value(text);
        if value.is_empty() {
            return Err(TasteEditError::Invalid("say it in a few words".into()));
        }
        reads.push((
            TasteDimension::Other,
            value,
            Polarity::Prefer,
            text.to_owned(),
        ));
    }
    let single = reads.len() == 1;
    let profile_id = data.id().to_string();
    let mut ids = Vec::new();
    for (dimension, value, polarity, read_text) in reads {
        let key = super::key(dimension, &value);
        let existing = data
            .taste
            .iter()
            .find(|a| a.key() == key && a.superseded_by.is_none())
            .cloned();
        let id = existing
            .as_ref()
            .map_or_else(|| TasteId::derive(&[&profile_id, "stated", &key]), |e| e.id);
        let mut sources = vec![TasteSource::Words {
            quote: text.to_owned(),
            statement: None,
        }];
        if let Some(e) = &existing {
            sources.extend(e.sources.iter().cloned());
        }
        upsert(
            data,
            TasteAssertion {
                id,
                dimension,
                value,
                polarity,
                text: if single { text.to_owned() } else { read_text },
                confidence: TasteConfidence::High,
                origin: TasteOrigin::Stated,
                review: TasteReview::Confirmed,
                sources: with_person(sources),
                explanation: None,
                interpreter: Some(reading.interpreter.clone()).filter(|i| !i.is_empty()),
                original: existing.and_then(|e| e.original),
                superseded_by: None,
                created_at: now,
                updated_at: now,
            },
        );
        ids.push(id);
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::super::compose::compose;
    use super::super::reading::ReadAssertion;
    use super::*;
    use crate::{
        Certainty, CompanyTrait, Preference, PreferenceOrigin, PreferenceValue, ProfileId, Stance,
    };

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 20, 12, minute, 0).unwrap()
    }

    fn read(dimension: TasteDimension, value: &str, polarity: Polarity) -> ReadAssertion {
        ReadAssertion {
            dimension,
            value: value.into(),
            polarity,
            confidence: TasteConfidence::High,
            text: vocab::sentence(dimension, value, polarity),
            explanation: None,
            origin: TasteOrigin::Interpreted,
            sources: vec![TasteSource::Words {
                quote: value.into(),
                statement: None,
            }],
        }
    }

    fn reading(items: Vec<ReadAssertion>) -> TasteReading {
        TasteReading {
            interpreter: "model/test".into(),
            assertions: items,
            ..TasteReading::default()
        }
    }

    fn data() -> ProfileData {
        let mut d = ProfileData::new(ProfileId::local(), at(0));
        set_brief(&mut d, "early-stage startups, small teams", None, at(0));
        d
    }

    fn find<'a>(p: &'a TasteProfile, key: &str) -> Option<&'a ComposedAssertion> {
        p.assertions.iter().find(|a| a.key() == key)
    }

    #[test]
    fn a_correction_survives_reinterpretation() {
        let mut d = data();
        let first = reading(vec![
            read(TasteDimension::Company, "early_stage", Polarity::Prefer),
            read(TasteDimension::Team, "small_team", Polarity::Prefer),
        ]);
        apply_reading(
            &mut d,
            &first,
            InterpretationOutcome::Read,
            None,
            "a".into(),
            at(1),
        );
        let p = compose(&d, &[]);
        let early = find(&p, "company:early_stage").unwrap().id;
        // "open to startups or small established companies"
        let words = reading(vec![
            read(TasteDimension::Company, "startup", Polarity::Open),
            read(TasteDimension::Company, "small_company", Polarity::Open),
        ]);
        let decided = correct(
            &mut d,
            &p,
            early,
            None,
            Some("Open to startups or small established companies"),
            Some(&words),
            at(2),
        )
        .unwrap();
        assert_eq!(decided.now.len(), 2);
        // The same words read again, and a profile import in between: the
        // correction stands, the old reading doesn't come back.
        apply_reading(
            &mut d,
            &first,
            InterpretationOutcome::Read,
            None,
            "b".into(),
            at(3),
        );
        let p = compose(&d, &[]);
        assert!(
            find(&p, "company:early_stage").is_none(),
            "{:#?}",
            p.assertions
        );
        let startup = find(&p, "company:startup").unwrap();
        assert_eq!(startup.origin, TasteOrigin::Stated);
        assert_eq!(startup.review, TasteReview::Corrected);
        assert_eq!(startup.polarity, Polarity::Open);
        assert_eq!(
            startup.original.as_ref().map(|o| o.value.as_str()),
            Some("early_stage"),
            "what Narrow had read is kept"
        );
        assert!(find(&p, "team:small_team").is_some(), "the rest is re-read");
        let keys: Vec<String> = p.assertions.iter().map(ComposedAssertion::key).collect();
        let mut unique = keys.clone();
        unique.dedup();
        assert_eq!(keys, unique, "no duplicate statements");
    }

    #[test]
    fn removal_and_neutral_stick() {
        let mut d = data();
        let r = reading(vec![
            read(TasteDimension::Company, "startup", Polarity::Prefer),
            read(TasteDimension::Team, "small_team", Polarity::Prefer),
        ]);
        apply_reading(
            &mut d,
            &r,
            InterpretationOutcome::Read,
            None,
            "a".into(),
            at(1),
        );
        let p = compose(&d, &[]);
        remove(&mut d, &p, find(&p, "company:startup").unwrap().id, at(2)).unwrap();
        let p = compose(&d, &[]);
        let decided = correct(
            &mut d,
            &p,
            find(&p, "team:small_team").unwrap().id,
            Some(Polarity::Neutral),
            None,
            None,
            at(3),
        )
        .unwrap();
        assert_eq!(decided.polarity, None);
        apply_reading(
            &mut d,
            &r,
            InterpretationOutcome::Read,
            None,
            "b".into(),
            at(4),
        );
        let p = compose(&d, &[]);
        assert!(find(&p, "company:startup").is_none());
        assert_eq!(p.removed.len(), 1);
        let team = find(&p, "team:small_team").unwrap();
        assert_eq!(team.polarity, Polarity::Neutral);
        assert_eq!(team.text, "Small technical teams: doesn't matter");
    }

    #[test]
    fn confirming_all_takes_the_summary_and_legacy_settings() {
        let mut d = data();
        let value = PreferenceValue::Company {
            company: CompanyTrait::SmallTeam,
        };
        d.preferences.push(Preference {
            id: PreferenceId::derive(&["small"]),
            value,
            stance: Stance::Wanted,
            origin: PreferenceOrigin::UserEntered,
            statement: None,
            snippet: None,
            certainty: Certainty::Certain,
            note: None,
            active: true,
            superseded_by: None,
            created_at: at(0),
            updated_at: at(0),
        });
        let r = reading(vec![read(
            TasteDimension::Company,
            "startup",
            Polarity::Prefer,
        )]);
        apply_reading(
            &mut d,
            &r,
            InterpretationOutcome::Read,
            None,
            "a".into(),
            at(1),
        );
        let p = compose(&d, &[]);
        assert_eq!(confirm(&mut d, &p, &[], at(2)).unwrap(), 2);
        let p = compose(&d, &[]);
        assert!(p.assertions.iter().all(ComposedAssertion::is_persons));
        assert!(p.assertions.iter().all(|a| a.stored), "legacy materialized");
        assert_eq!(d.taste_brief.as_ref().unwrap().confirmed_at, Some(at(2)));
        // Confirming again changes nothing.
        assert_eq!(confirm(&mut d, &p, &[], at(3)).unwrap(), 0);
    }

    #[test]
    fn legacy_corrections_report_their_preferences() {
        let mut d = data();
        let pid = PreferenceId::derive(&["early"]);
        d.preferences.push(Preference {
            id: pid,
            value: PreferenceValue::Company {
                company: CompanyTrait::EarlyStage,
            },
            stance: Stance::Wanted,
            origin: PreferenceOrigin::UserEntered,
            statement: None,
            snippet: None,
            certainty: Certainty::Certain,
            note: None,
            active: true,
            superseded_by: None,
            created_at: at(0),
            updated_at: at(0),
        });
        let p = compose(&d, &[]);
        let id = find(&p, "company:early_stage").unwrap().id;
        let decided = correct(&mut d, &p, id, Some(Polarity::Avoid), None, None, at(1)).unwrap();
        assert_eq!(decided.preferences, [pid]);
        assert_eq!(decided.polarity, Some(Polarity::Avoid));
        let p = compose(&d, &[]);
        let early = find(&p, "company:early_stage").unwrap();
        assert_eq!(early.polarity, Polarity::Avoid);
        assert_eq!(early.review, TasteReview::Corrected);
        assert_eq!(
            early.against.len(),
            1,
            "until the caller updates the setting"
        );
    }

    #[test]
    fn added_sentences_are_the_persons_and_unreadable_ones_are_kept() {
        let mut d = data();
        let ids = add(
            &mut d,
            "Something with real users",
            &reading(Vec::new()),
            at(1),
        )
        .unwrap();
        let p = compose(&d, &[]);
        let a = p.find(ids[0]).unwrap();
        assert_eq!(a.dimension, TasteDimension::Other);
        assert_eq!(a.text, "Something with real users");
        assert_eq!(a.origin, TasteOrigin::Stated);
        assert!(add(&mut d, "  !! ", &reading(Vec::new()), at(2)).is_err());
    }
}
