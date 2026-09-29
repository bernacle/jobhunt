//! Exact field values supplied by each source of a shared record.

use std::collections::BTreeMap;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::ids::ExperienceId;
use crate::model::{Origin, RecordMeta, RecordSourceSnapshot, SourceDocument};
use crate::resume::{ParsedEducation, ParsedExperience, ParsedProject};
use crate::support::supporting_origins;

/// The data fields of a record, without its identity, display order or meta.
pub(crate) fn fields<T: Serialize>(record: &T) -> BTreeMap<String, Value> {
    let Ok(Value::Object(mut map)) = serde_json::to_value(record) else {
        return BTreeMap::new();
    };
    for key in ["id", "position", "meta"] {
        map.remove(key);
    }
    map.into_iter().collect()
}

fn map(value: Value) -> BTreeMap<String, Value> {
    match value {
        Value::Object(object) => object.into_iter().collect(),
        _ => BTreeMap::new(),
    }
}

pub(crate) fn experience(parsed: &ParsedExperience) -> BTreeMap<String, Value> {
    map(serde_json::json!({
        "company": parsed.company, "title": parsed.title,
        "employment": parsed.employment, "start": parsed.start,
        "end": parsed.end, "current": parsed.current,
        "location": parsed.location, "summary": parsed.summary,
    }))
}

pub(crate) fn project(
    parsed: &ParsedProject,
    experience: Option<ExperienceId>,
) -> BTreeMap<String, Value> {
    map(serde_json::json!({
        "name": parsed.name, "description": parsed.description,
        "role": parsed.role, "url": parsed.url,
        "start": parsed.start, "end": parsed.end,
        "current": parsed.current, "experience": experience,
    }))
}

pub(crate) fn education(parsed: &ParsedEducation) -> BTreeMap<String, Value> {
    map(serde_json::json!({
        "institution": parsed.institution, "degree": parsed.degree,
        "field": parsed.field, "start": parsed.start,
        "end": parsed.end, "current": parsed.current,
    }))
}

pub(crate) fn put(
    meta: &mut RecordMeta,
    origin: Origin,
    import_key: String,
    fields: BTreeMap<String, Value>,
) {
    if let Some(snapshot) = meta
        .source_snapshots
        .iter_mut()
        .find(|s| s.origin == origin)
    {
        snapshot.import_key = import_key;
        snapshot.fields = fields;
    } else {
        meta.source_snapshots.push(RecordSourceSnapshot {
            origin,
            import_key,
            fields,
        });
    }
}

/// A legacy resume record has no snapshot until another source meets it.
pub(crate) fn backfill<T: Serialize>(record: &T, meta: &mut RecordMeta) {
    if meta.source_snapshots.is_empty() && meta.origin.is_imported() {
        let origin = meta.origin;
        let import_key = meta.import_key.clone().unwrap_or_default();
        let mut stated = fields(record);
        for field in &meta.edited_fields {
            stated.remove(field);
        }
        put(meta, origin, import_key, stated);
    }
}

pub(crate) fn forget(meta: &mut RecordMeta, origin: Origin) {
    meta.source_snapshots.retain(|s| s.origin != origin);
}

pub(crate) fn has_key(meta: &RecordMeta, origin: Origin, key: &str) -> bool {
    meta.source_snapshots
        .iter()
        .any(|s| s.origin == origin && s.import_key == key)
}

pub(crate) fn snapshot(meta: &RecordMeta, origin: Origin) -> Option<&RecordSourceSnapshot> {
    meta.source_snapshots.iter().find(|s| s.origin == origin)
}

/// Keep an existing field only if a live source states that exact value;
/// otherwise take a sole surviving value or clear it. User edits are fixed.
pub(crate) fn reconcile<T: Serialize + DeserializeOwned>(
    record: &mut T,
    meta: &RecordMeta,
    documents: &[SourceDocument],
    names: &[&str],
) -> bool {
    if meta.origin == Origin::User || meta.source_snapshots.is_empty() {
        return false;
    }
    let origins = supporting_origins(
        documents,
        meta.source.as_ref(),
        &meta.corroborations,
        meta.origin,
        meta.stale_since.is_some(),
    );
    let Ok(Value::Object(mut object)) = serde_json::to_value(&*record) else {
        return false;
    };
    let before = object.clone();
    for &name in names {
        if meta.is_edited(name) {
            continue;
        }
        let current = object.get(name).filter(|v| !v.is_null()).cloned();
        let mut supported: Vec<Value> = Vec::new();
        for source in &meta.source_snapshots {
            if !origins.contains(&source.origin) {
                continue;
            }
            if let Some(value) = source.fields.get(name).filter(|v| !v.is_null())
                && !supported.contains(value)
            {
                supported.push(value.clone());
            }
        }
        let selected = current.filter(|v| supported.contains(v)).or_else(|| {
            if supported.len() == 1 {
                supported.pop()
            } else if matches!(name, "name" | "institution") {
                // These fields are required by their record schema. When
                // live sources disagree, display one supported value in a
                // stable order; never retain the removed source's value or
                // fail the whole record reconciliation.
                supported.into_iter().min_by_key(Value::to_string)
            } else {
                None
            }
        });
        if let Some(value) = selected {
            object.insert(name.to_owned(), value);
        } else {
            object.remove(name);
        }
    }
    if object == before {
        return false;
    }
    if let Ok(updated) = serde_json::from_value(Value::Object(object)) {
        *record = updated;
        true
    } else {
        false
    }
}

pub(crate) const EXPERIENCE_FIELDS: &[&str] = &[
    "company",
    "title",
    "employment",
    "start",
    "end",
    "current",
    "location",
    "summary",
];
pub(crate) const PROJECT_FIELDS: &[&str] = &[
    "name",
    "description",
    "role",
    "url",
    "start",
    "end",
    "current",
    "experience",
];
pub(crate) const EDUCATION_FIELDS: &[&str] =
    &["institution", "degree", "field", "start", "end", "current"];
