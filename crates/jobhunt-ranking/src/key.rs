//! What a preference is about: a [`TasteKey`] (a dimension and a canonical
//! value) and a [`Direction`].
//!
//! Keys are shared by everything that talks about taste: the facets read
//! from a job ([`crate::facets`](mod@crate::facets)(mod@crate::facets)), the reasons people give
//! ([`crate::reason`]), the preferences they set in their profile
//! ([`crate::person`]) and what is learned from feedback
//! ([`crate::taste`]). One vocabulary means "pure SRE" in a rejection,
//! "SRE" in a job title and an unwanted "SRE" role preference all land on
//! the same key.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The aspects of a job a person can like or dislike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    /// The shape of the role: backend, frontend, SRE, sales, ….
    Role,
    /// Level: junior, senior, staff, ….
    Seniority,
    /// How the work is done: ownership, management, on-call, ….
    WorkStyle,
    /// A technology the job works with.
    Technology,
    /// A business domain: fintech, developer tools, ….
    Domain,
    /// A kind of company or team: startup, large company, founder-led, ….
    CompanyTrait,
    /// One particular employer.
    Company,
    /// Pay: its level, or pay pegged to a local market.
    Compensation,
    /// Interest in the product or problem itself.
    Product,
    /// Something the posting left unclear (a remote policy, …).
    Information,
}

impl Dimension {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Role => "role",
            Self::Seniority => "seniority",
            Self::WorkStyle => "work_style",
            Self::Technology => "technology",
            Self::Domain => "domain",
            Self::CompanyTrait => "company_trait",
            Self::Company => "company",
            Self::Compensation => "compensation",
            Self::Product => "product",
            Self::Information => "information",
        }
    }

    /// As shown to the person.
    pub fn label(self) -> &'static str {
        match self {
            Self::Role => "role",
            Self::Seniority => "seniority",
            Self::WorkStyle => "work style",
            Self::Technology => "technology",
            Self::Domain => "domain",
            Self::CompanyTrait => "company",
            Self::Company => "employer",
            Self::Compensation => "pay",
            Self::Product => "product",
            Self::Information => "information",
        }
    }
}

/// A dimension and a canonical value: `role:sre`, `domain:fintech`,
/// `company_trait:large_company`, `technology:Rust`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TasteKey {
    pub dimension: Dimension,
    pub value: String,
}

impl TasteKey {
    pub fn new(dimension: Dimension, value: impl Into<String>) -> Self {
        Self {
            dimension,
            value: value.into(),
        }
    }

    pub fn role(value: &str) -> Self {
        Self::new(Dimension::Role, value)
    }

    /// `role: SRE / DevOps`, `company: large companies`, `domain: fintech`.
    pub fn label(&self) -> String {
        format!("{}: {}", self.dimension.label(), value_label(self))
    }

    /// The value alone, readable: `SRE / DevOps`, `large companies`.
    pub fn value_label(&self) -> String {
        value_label(self)
    }
}

impl fmt::Display for TasteKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.dimension.as_str(), self.value)
    }
}

fn value_label(key: &TasteKey) -> String {
    use jobhunt_profile::{CompanyTrait, WorkAspect};
    match key.dimension {
        Dimension::Role => match key.value.as_str() {
            "sre" => "SRE / DevOps".into(),
            "solutions" => "solutions / forward-deployed engineering".into(),
            "product management" => "product management".into(),
            other => other.to_owned(),
        },
        Dimension::CompanyTrait => CompanyTrait::from_canonical(&key.value)
            .map_or_else(|| key.value.replace('_', " "), |t| t.label().to_owned()),
        Dimension::WorkStyle => WorkAspect::from_canonical(&key.value)
            .map_or_else(|| key.value.replace('_', " "), |a| a.label().to_owned()),
        Dimension::Compensation => match key.value.as_str() {
            "local_pay" => "pay pegged to a local market".into(),
            "pay_level" => "the pay level".into(),
            other => other.replace('_', " "),
        },
        Dimension::Product => match key.value.as_str() {
            "interest" => "the product or problem".into(),
            "team" => "the team".into(),
            other => other.to_owned(),
        },
        Dimension::Information => match key.value.as_str() {
            "remote_policy" => "the remote policy".into(),
            "description" => "the job description".into(),
            other => other.replace('_', " "),
        },
        Dimension::Seniority | Dimension::Technology | Dimension::Domain | Dimension::Company => {
            key.value.clone()
        }
    }
}

/// Toward or away from something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Prefer,
    Avoid,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prefer => "prefer",
            Self::Avoid => "avoid",
        }
    }

    /// `+1.0` or `-1.0`.
    pub fn sign(self) -> f64 {
        match self {
            Self::Prefer => 1.0,
            Self::Avoid => -1.0,
        }
    }

    pub fn of(value: f64) -> Option<Self> {
        if value > 0.0 {
            Some(Self::Prefer)
        } else if value < 0.0 {
            Some(Self::Avoid)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_read_well() {
        assert_eq!(TasteKey::role("sre").label(), "role: SRE / DevOps");
        assert_eq!(
            TasteKey::new(Dimension::CompanyTrait, "large_company").label(),
            "company: large companies"
        );
        assert_eq!(
            TasteKey::new(Dimension::Domain, "fintech").to_string(),
            "domain:fintech"
        );
        assert_eq!(Direction::of(-0.5), Some(Direction::Avoid));
        assert_eq!(Direction::of(0.0), None);
    }
}
