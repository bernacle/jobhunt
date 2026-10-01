//! "What kind of role are you looking for?": the person's explicit choice
//! of the kinds of engineering work they want next (BRU-324).
//!
//! Career history says what someone **has done**; it doesn't say what they
//! **want to do next**. Inferring the second from the first (or from free
//! text alone) left Today empty for people whose words named only company
//! and team taste. So the kind of work is asked once, as a short
//! multi-select over the `work_shape` vocabulary ([`CHOICES`]), with an
//! optional title in their words for hybrid roles the list misses.
//!
//! A choice is stored as ordinary taste ([`edit`](super::edit)): one
//! `work_shape` statement per kind of work, `stated` and `confirmed`, so
//! ranking reads it through the same composed profile as everything else
//! (firm). There is no separate ranking of target roles.
//!
//! * **Precedence** ([`super::compose()`]): once the person has chosen the
//!   kinds of work they want, kinds of work only read or inferred (from
//!   their words, their profile, a setting read with doubts, or feedback)
//!   are kept as supporting context and no longer wanted; a reading that
//!   avoids what they chose ("avoid AI" for someone choosing ML product
//!   work) is set aside, and said, rather than holding their choice back.
//! * **The title** is supplemental: kept verbatim as one statement
//!   ([`TITLE_VALUE`] under [`TasteDimension::Other`]) that ranking doesn't
//!   read, never parsed into kinds of work. It is context for the person
//!   and for the semantic reviewer.
//! * **Depth stays apart**: the list offers kinds of work, not
//!   specializations. Database internals, distributed systems, research
//!   and ML research aren't offered: choosing Backend never implies
//!   storage-engine work, nor Platform Kubernetes internals.

use crate::aggregate::ProfileData;
use crate::evidence::ClaimKind;

use super::{TasteDimension, rules};

/// The kinds of work offered, in the order shown: canonical `work_shape`
/// values and their short labels.
pub const CHOICES: [(&str, &str); 12] = [
    ("backend", "Backend"),
    ("platform", "Platform"),
    ("infrastructure", "Infrastructure"),
    ("product", "Product engineering"),
    ("full_stack", "Full stack"),
    ("frontend", "Frontend"),
    ("mobile", "Mobile"),
    ("developer_tooling", "Developer tooling"),
    ("sre", "SRE"),
    ("security", "Security"),
    ("data", "Data"),
    ("ml_product", "ML product"),
];

/// At most this many kinds of work: the question is what they want next,
/// not everything they could do.
pub const MAX_CHOICES: usize = 3;

/// The longest title kept.
pub const MAX_TITLE: usize = 80;

/// The value of the title statement (`other:target_role`).
pub const TITLE_VALUE: &str = "target_role";

/// The short label of a kind of work offered ("Backend"), if it is one.
pub fn label(value: &str) -> Option<&'static str> {
    CHOICES.iter().find(|(v, _)| *v == value).map(|(_, l)| *l)
}

/// Every kind of work that can be chosen: the ones offered first, then the
/// rest of the `work_shape` vocabulary (specialties such as
/// `database_internals`, for someone who wants exactly that and says so
/// through the API or the CLI; never offered in the list).
fn choosable() -> impl Iterator<Item = (&'static str, &'static str)> {
    CHOICES.iter().copied().chain(
        super::vocab::known(TasteDimension::WorkShape)
            .iter()
            .copied()
            .filter(|(v, _)| label(v).is_none()),
    )
}

/// How a kind of work reads in a list of choices: "Backend", "Database
/// internals and storage engines".
pub fn display(value: &str) -> String {
    label(value).map_or_else(
        || super::vocab::phrase(TasteDimension::WorkShape, value),
        str::to_owned,
    )
}

fn position(value: &str) -> usize {
    choosable()
        .position(|(v, _)| v == value)
        .unwrap_or(usize::MAX)
}

/// Whether a statement is the title in the person's words.
pub fn is_title(dimension: TasteDimension, value: &str) -> bool {
    dimension == TasteDimension::Other && value == TITLE_VALUE
}

/// Why a choice can't be stored.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChoiceError {
    #[error("{0:?} is not a kind of role Narrow offers (choose from: {list})", list = choices_list())]
    Unknown(String),
    #[error("choose at most {MAX_CHOICES} kinds of role")]
    TooMany,
    #[error("choose at least one kind of role, or describe it in a few words")]
    Empty,
    #[error("keep the title under {MAX_TITLE} characters")]
    TitleTooLong,
}

fn choices_list() -> String {
    CHOICES
        .iter()
        .map(|(v, _)| *v)
        .collect::<Vec<_>>()
        .join(", ")
}

/// A validated choice: canonical values, deduplicated, in [`CHOICES`]
/// order, and the title trimmed to one line (or none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleChoice {
    pub shapes: Vec<&'static str>,
    pub title: Option<String>,
}

impl RoleChoice {
    /// Validates what the person chose. Values are matched loosely
    /// ("Full stack", "full-stack" and `full_stack` are one).
    pub fn parse(shapes: &[String], title: Option<&str>) -> Result<Self, ChoiceError> {
        let mut chosen: Vec<&'static str> = Vec::new();
        for raw in shapes {
            let value = super::normalize_value(raw);
            let found = choosable()
                .find(|(v, l)| *v == value || super::normalize_value(l) == value)
                .map(|(v, _)| v)
                .ok_or_else(|| ChoiceError::Unknown(raw.trim().to_owned()))?;
            if !chosen.contains(&found) {
                chosen.push(found);
            }
        }
        if chosen.len() > MAX_CHOICES {
            return Err(ChoiceError::TooMany);
        }
        chosen.sort_by_key(|v| position(v));
        let title = title
            .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|t| !t.is_empty());
        if title
            .as_ref()
            .is_some_and(|t| t.chars().count() > MAX_TITLE)
        {
            return Err(ChoiceError::TitleTooLong);
        }
        if chosen.is_empty() && title.is_none() {
            return Err(ChoiceError::Empty);
        }
        Ok(Self {
            shapes: chosen,
            title,
        })
    }
}

/// Kinds of work, among those offered, that the person's career evidence
/// shows they **have done** (the role signals ranking reads: "backend",
/// "platform engineering"). A hint for answering the question, never a statement of
/// what they want.
pub fn demonstrated(data: &ProfileData) -> Vec<&'static str> {
    let mut found: Vec<&'static str> = Vec::new();
    for (topic, _) in data.signals(ClaimKind::Role) {
        for (dimension, value) in rules::read_terms(&topic) {
            if dimension != TasteDimension::WorkShape {
                continue;
            }
            if let Some((v, _)) = CHOICES.iter().find(|(v, _)| *v == value)
                && !found.contains(v)
            {
                found.push(v);
            }
        }
    }
    found.sort_by_key(|v| CHOICES.iter().position(|(c, _)| c == v));
    found
}

/// Domains (and technologies) whose avoidance would contradict choosing a
/// kind of work: "avoid AI" read from someone's words goes against their
/// own choice of ML product work.
pub fn contradicting(shape: &str) -> &'static [&'static str] {
    match shape {
        "ml_product" => &[
            "ai",
            "ml",
            "machine_learning",
            "artificial_intelligence",
            "llm",
            "llms",
        ],
        "security" => &["security", "cybersecurity"],
        "data" => &["data", "data_engineering"],
        "mobile" => &["mobile"],
        "frontend" => &["frontend", "front_end"],
        "developer_tooling" => &["developer_tools", "developer_tooling", "devtools"],
        "infrastructure" => &["infrastructure", "cloud_infrastructure"],
        "platform" => &["platform", "platform_engineering"],
        "sre" => &["sre", "reliability", "operations"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(shapes: &[&str], title: Option<&str>) -> Result<RoleChoice, ChoiceError> {
        RoleChoice::parse(
            &shapes.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
            title,
        )
    }

    #[test]
    fn choices_are_canonical_work_shapes_without_depth() {
        let known: Vec<&str> = super::super::vocab::known(TasteDimension::WorkShape)
            .iter()
            .map(|(v, _)| *v)
            .collect();
        for (value, _) in CHOICES {
            assert!(known.contains(&value), "{value} is a work_shape token");
        }
        for depth in [
            "database_internals",
            "distributed_systems",
            "ml_research",
            "research",
        ] {
            assert!(label(depth).is_none(), "{depth} is depth, not offered");
        }
    }

    #[test]
    fn a_choice_is_validated_and_ordered() {
        let c = parse(&["Platform", "backend", "full-stack", "backend"], None).unwrap();
        assert_eq!(c.shapes, ["backend", "platform", "full_stack"]);
        assert_eq!(c.title, None);
        assert_eq!(
            parse(&["backend", "platform", "product", "sre"], None),
            Err(ChoiceError::TooMany)
        );
        assert!(matches!(
            parse(&["storage engines please"], None),
            Err(ChoiceError::Unknown(_))
        ));
        // Not offered, but a specialist can say exactly that.
        assert_eq!(
            parse(&["database_internals", "backend"], None)
                .unwrap()
                .shapes,
            ["backend", "database_internals"]
        );
        assert_eq!(parse(&[], Some("  ")), Err(ChoiceError::Empty));
        let titled = parse(&[], Some("  Infrastructure-focused   Product Engineer ")).unwrap();
        assert!(titled.shapes.is_empty());
        assert_eq!(
            titled.title.as_deref(),
            Some("Infrastructure-focused Product Engineer")
        );
        assert_eq!(
            parse(&["backend"], Some(&"x".repeat(MAX_TITLE + 1))),
            Err(ChoiceError::TitleTooLong)
        );
    }
}
