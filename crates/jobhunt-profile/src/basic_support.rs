//! Ownership of basics filled from a LinkedIn export.

use crate::model::{BasicSourceSnapshot, Origin, Profile};
use crate::resume::ParsedBasics;

fn snapshot(profile: &Profile, origin: Origin) -> Option<&BasicSourceSnapshot> {
    profile.basic_sources.iter().find(|s| s.origin == origin)
}

fn stated(basics: &ParsedBasics, origin: Origin) -> BasicSourceSnapshot {
    BasicSourceSnapshot {
        origin,
        headline: basics.headline.clone(),
        location: basics.location.clone(),
        summary: basics.summary.clone(),
        languages: basics.languages.clone(),
    }
}

fn resolve<T: Clone + PartialEq>(current: &mut T, values: Vec<T>, empty: T) {
    if values.contains(current) {
        return;
    }
    let mut distinct = Vec::new();
    for value in values {
        if !distinct.contains(&value) {
            distinct.push(value);
        }
    }
    *current = if distinct.len() == 1 {
        distinct.remove(0)
    } else {
        empty
    };
}

fn reconcile(profile: &mut Profile) {
    let sources = profile.basic_sources.clone();
    if !profile.is_edited("headline") {
        resolve(
            &mut profile.headline,
            sources
                .iter()
                .filter_map(|s| s.headline.clone())
                .map(Some)
                .collect(),
            None,
        );
    }
    if !profile.is_edited("location") {
        resolve(
            &mut profile.location,
            sources
                .iter()
                .filter_map(|s| s.location.clone())
                .map(Some)
                .collect(),
            None,
        );
    }
    if !profile.is_edited("summary") {
        resolve(
            &mut profile.summary,
            sources
                .iter()
                .filter_map(|s| s.summary.clone())
                .map(Some)
                .collect(),
            None,
        );
    }
    if !profile.is_edited("languages") {
        resolve(
            &mut profile.languages,
            sources
                .iter()
                .filter(|s| !s.languages.is_empty())
                .map(|s| s.languages.clone())
                .collect(),
            Vec::new(),
        );
    }
}

/// Returns whether a hand-edited basic differed from this source's value.
pub(crate) fn import(
    profile: &mut Profile,
    basics: &ParsedBasics,
    origin: Origin,
    has_resume: bool,
) -> bool {
    if profile.basic_sources.is_empty() && origin == Origin::Linkedin && has_resume {
        profile.basic_sources.push(BasicSourceSnapshot {
            origin: Origin::Resume,
            headline: (!profile.is_edited("headline"))
                .then(|| profile.headline.clone())
                .flatten(),
            location: (!profile.is_edited("location"))
                .then(|| profile.location.clone())
                .flatten(),
            summary: (!profile.is_edited("summary"))
                .then(|| profile.summary.clone())
                .flatten(),
            languages: if profile.is_edited("languages") {
                Vec::new()
            } else {
                profile.languages.clone()
            },
        });
    }
    if origin == Origin::Resume && profile.basic_sources.is_empty() {
        return false; // legacy resume-only behavior is handled by the caller
    }
    let old = snapshot(profile, origin).cloned();
    let incoming = stated(basics, origin);
    let mut preserved = false;
    let edited = profile.edited_fields.clone();
    let mut update = |name: &str,
                      target: &mut Option<String>,
                      old: Option<&Option<String>>,
                      new: &Option<String>| {
        if edited.iter().any(|f| f == name) {
            preserved |= target != new;
        } else if origin == Origin::Resume && new.is_some()
            || target.is_none()
            || old.is_some_and(|old| target == old)
        {
            *target = new.clone();
        }
    };
    update(
        "headline",
        &mut profile.headline,
        old.as_ref().map(|s| &s.headline),
        &incoming.headline,
    );
    update(
        "location",
        &mut profile.location,
        old.as_ref().map(|s| &s.location),
        &incoming.location,
    );
    update(
        "summary",
        &mut profile.summary,
        old.as_ref().map(|s| &s.summary),
        &incoming.summary,
    );
    if profile.is_edited("languages") {
        preserved |= profile.languages != incoming.languages;
    } else if origin == Origin::Resume && !incoming.languages.is_empty()
        || profile.languages.is_empty()
        || old
            .as_ref()
            .is_some_and(|s| profile.languages == s.languages)
    {
        profile.languages = incoming.languages.clone();
    }
    if let Some(existing) = profile
        .basic_sources
        .iter_mut()
        .find(|s| s.origin == origin)
    {
        *existing = incoming;
    } else {
        profile.basic_sources.push(incoming);
    }
    reconcile(profile);
    preserved
}

pub(crate) fn remove(profile: &mut Profile, origin: Origin) {
    profile.basic_sources.retain(|s| s.origin != origin);
    reconcile(profile);
    // A single resume again has the legacy v1 shape. Its displayed values
    // have already been reconciled against the snapshot.
    if profile
        .basic_sources
        .iter()
        .all(|s| s.origin == Origin::Resume)
    {
        profile.basic_sources.clear();
    }
}
