//! The person's side of ranking, read from the profile domain
//! ([`ProfileData`]), never from storage.
//!
//! Two different things are kept apart:
//!
//! * **what they want**: the preferences they set or stated (roles,
//!   pay, company and team kinds, domains, work style, work modes). These
//!   are explicit and always outrank anything learned from feedback;
//! * **what they have done**: evidence from the resume (role kinds,
//!   domains, technologies, the level of their latest title). Having
//!   worked in payments is not wanting to work in payments, so experience
//!   only ever says "you have relevant experience", never "you want this".
//!
//! Location, work authorization and engagement preferences are
//! eligibility's; ranking doesn't read them again.

use jobhunt_core::text::search_key;
use jobhunt_profile::infer::{canonical_domain, topic_key};
use jobhunt_profile::{
    Arrangement, Certainty, ClaimKind, CompensationBound, EvidenceStrength, PayPeriod,
    PreferenceValue, ProfileData, Stance, WorkMode,
};

use crate::facets::{Level, title_level, title_roles};
use crate::key::{Dimension, Direction, TasteKey};

/// How a stated preference is recognized in a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Matcher {
    /// Every key must be among the job's facets ("staff backend" is
    /// `role:backend` and `seniority:staff`).
    Keys(Vec<TasteKey>),
    /// A role JobHunt's vocabulary doesn't know: matched as words of the
    /// job title (normalized).
    TitleWords(String),
}

/// A preference the person set or stated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatedPreference {
    /// `pref_…`.
    pub id: String,
    pub dimension: Dimension,
    pub matcher: Matcher,
    pub stance: Stance,
    /// As shown: "backend roles", "small teams".
    pub text: String,
    /// JobHunt read it with doubts.
    pub uncertain: bool,
}

impl StatedPreference {
    pub fn direction(&self) -> Direction {
        match self.stance {
            Stance::Unwanted => Direction::Avoid,
            Stance::Required | Stance::Wanted | Stance::Acceptable => Direction::Prefer,
        }
    }

    /// The single key this preference is about, when it is about one.
    pub fn key(&self) -> Option<&TasteKey> {
        match &self.matcher {
            Matcher::Keys(keys) if keys.len() == 1 => keys.first(),
            _ => None,
        }
    }
}

/// A pay floor or goal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayPreference {
    pub bound: CompensationBound,
    pub amount: u64,
    /// ISO code; `None` when the person's statement didn't say which.
    pub currency: Option<String>,
    pub period: PayPeriod,
    pub arrangement: Option<Arrangement>,
    pub stance: Stance,
    /// "at least USD 150,000 per year".
    pub text: String,
}

/// Evidence of having done something, with where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Experience {
    /// Role kind or domain (`backend`, `payments`).
    pub topic: String,
    /// "Ledgerly", or the claim text when there is no company.
    pub places: Vec<String>,
}

/// Everything ranking reads about the person.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Person {
    pub profile_id: String,
    pub revision: u64,
    pub stated: Vec<StatedPreference>,
    pub pay: Vec<PayPreference>,
    pub work_modes: Vec<(WorkMode, Stance)>,
    /// Role kinds evidenced by the resume.
    pub roles: Vec<Experience>,
    pub domains: Vec<Experience>,
    /// Technologies with how well they are backed.
    pub technologies: Vec<(String, EvidenceStrength)>,
    /// The level of the latest position, with its title.
    pub level: Option<(Level, String)>,
    /// The resume shows engineering work.
    pub engineer: bool,
}

/// The keys a role preference is about.
pub fn role_matcher(role: &str) -> Matcher {
    let mut keys: Vec<TasteKey> = title_roles(role)
        .into_iter()
        .map(|(value, _)| TasteKey::role(value))
        .collect();
    if let Some((level, _)) = title_level(role) {
        keys.push(TasteKey::new(Dimension::Seniority, level.as_str()));
    }
    if keys.is_empty() {
        Matcher::TitleWords(search_key(role))
    } else {
        Matcher::Keys(keys)
    }
}

/// The key a domain preference is about.
pub fn domain_key(domain: &str) -> TasteKey {
    let value = canonical_domain(domain).map_or_else(|| topic_key(domain), str::to_owned);
    TasteKey::new(Dimension::Domain, value)
}

const ENGINEERING_WORDS: [&str; 5] = ["engineer", "developer", "programmer", "sre", "architect"];

impl Person {
    pub fn from_profile(data: &ProfileData) -> Self {
        let mut out = Self {
            profile_id: data.profile.id.to_string(),
            revision: data.profile.revision,
            ..Self::default()
        };
        for p in data.preferences().active() {
            let uncertain = p.certainty == Certainty::Uncertain;
            let stated = |dimension, matcher| StatedPreference {
                id: p.id.to_string(),
                dimension,
                matcher,
                stance: p.stance,
                text: p.value.to_string(),
                uncertain,
            };
            match &p.value {
                PreferenceValue::Role { role } => {
                    out.stated.push(stated(Dimension::Role, role_matcher(role)));
                }
                PreferenceValue::Company { company } => out.stated.push(stated(
                    Dimension::CompanyTrait,
                    Matcher::Keys(vec![TasteKey::new(
                        Dimension::CompanyTrait,
                        company.as_str(),
                    )]),
                )),
                PreferenceValue::Domain { domain } => out.stated.push(stated(
                    Dimension::Domain,
                    Matcher::Keys(vec![domain_key(domain)]),
                )),
                PreferenceValue::WorkStyle { aspect } => out.stated.push(stated(
                    Dimension::WorkStyle,
                    Matcher::Keys(vec![TasteKey::new(Dimension::WorkStyle, aspect.as_str())]),
                )),
                PreferenceValue::Compensation {
                    bound,
                    amount,
                    currency,
                    period,
                    arrangement,
                } => out.pay.push(PayPreference {
                    bound: *bound,
                    amount: *amount,
                    currency: currency.as_ref().map(|c| c.to_uppercase()),
                    period: *period,
                    arrangement: *arrangement,
                    stance: p.stance,
                    text: p.value.to_string(),
                }),
                PreferenceValue::WorkMode { mode } => out.work_modes.push((*mode, p.stance)),
                PreferenceValue::CurrentLocation { .. }
                | PreferenceValue::Region { .. }
                | PreferenceValue::Timezone { .. }
                | PreferenceValue::Relocation { .. }
                | PreferenceValue::Sponsorship { .. }
                | PreferenceValue::WorkAuthorization { .. }
                | PreferenceValue::Engagement { .. } => {}
            }
        }
        let place = |claim: &jobhunt_profile::Claim| match claim.subject {
            jobhunt_profile::Subject::Experience(id) => data
                .experience(id)
                .and_then(|e| e.company.clone().or_else(|| e.title.clone())),
            _ => None,
        };
        let experience = |topic: String, claims: Vec<&jobhunt_profile::Claim>| {
            let mut places: Vec<String> = Vec::new();
            for claim in claims {
                if let Some(p) = place(claim)
                    && !places.contains(&p)
                {
                    places.push(p);
                }
            }
            Experience { topic, places }
        };
        out.roles = data
            .signals(ClaimKind::Role)
            .into_iter()
            .map(|(topic, claims)| experience(topic, claims))
            .collect();
        out.domains = data
            .domains()
            .into_iter()
            .map(|d| experience(d.domain, d.claims))
            .collect();
        out.technologies = data
            .skills_with_evidence()
            .into_iter()
            .filter(|s| s.strength > EvidenceStrength::Unsupported)
            .map(|s| (s.skill.name.clone(), s.strength))
            .collect();
        let titled: Vec<&str> = data
            .visible_experiences()
            .into_iter()
            .filter_map(|e| e.title.as_deref())
            .collect();
        out.level = titled
            .first()
            .and_then(|t| title_level(t).map(|(l, _)| (l, (*t).to_owned())));
        out.engineer = !out.roles.is_empty()
            || titled.iter().any(|t| {
                let key = search_key(t);
                ENGINEERING_WORDS.iter().any(|w| key.contains(w))
            });
        out
    }

    /// Stated preferences about exactly one key, with their direction:
    /// what learned taste must never override.
    pub fn explicit_keys(&self) -> Vec<(TasteKey, Direction, String)> {
        self.stated
            .iter()
            .filter_map(|p| p.key().map(|k| (k.clone(), p.direction(), p.text.clone())))
            .collect()
    }

    /// How well the resume backs a technology.
    pub fn technology(&self, name: &str) -> Option<EvidenceStrength> {
        self.technologies
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, s)| *s)
    }

    pub fn role_experience(&self, role: &str) -> Option<&Experience> {
        self.roles.iter().find(|r| r.topic == role)
    }

    pub fn domain_experience(&self, domain: &str) -> Option<&Experience> {
        self.domains.iter().find(|d| d.topic == domain)
    }

    /// Whether the person said anything about what they want.
    pub fn has_preferences(&self) -> bool {
        !self.stated.is_empty() || !self.pay.is_empty() || !self.work_modes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_preferences_become_keys() {
        assert_eq!(
            role_matcher("backend"),
            Matcher::Keys(vec![TasteKey::role("backend")])
        );
        assert_eq!(
            role_matcher("staff backend"),
            Matcher::Keys(vec![
                TasteKey::role("backend"),
                TasteKey::new(Dimension::Seniority, "staff")
            ])
        );
        assert_eq!(
            role_matcher("pure SRE"),
            Matcher::Keys(vec![TasteKey::role("sre")])
        );
        assert_eq!(
            role_matcher("founding engineer"),
            Matcher::Keys(vec![TasteKey::new(Dimension::Seniority, "founding")])
        );
        assert_eq!(
            role_matcher("Developer Advocate"),
            Matcher::TitleWords("developer advocate".into())
        );
        assert_eq!(domain_key("devtools").value, "developer tools");
        assert_eq!(domain_key("Weather").value, "weather");
    }
}
