//! The person's side: where they are and what they can accept, read from
//! the profile domain ([`ProfileData`]), never from storage directly.
//!
//! Missing information stays missing. The location comes from the
//! `current_location` preference, or else the resume header (and says so);
//! it is never guessed from the machine's locale or IP. Work authorization
//! comes only from what the person stated: "authorized to work in X"
//! preferences, and "no sponsorship needed" read as authorization where
//! they live (documented, and named in every reason that relies on it).

use jobhunt_profile::{Engagement, PreferenceValue, ProfileData, Stance, WorkMode};

use crate::geo::{Area, COUNTRIES, Country, Membership, parse_places};
use crate::zones::{Offsets, span, zones_in};

/// Where a profile fact came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactBasis {
    /// A preference the person set or stated.
    Preference,
    /// The resume header; used only when no preference says otherwise.
    Resume,
    /// Derived from another fact (a country's time zones).
    Derived,
}

impl FactBasis {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preference => "preference",
            Self::Resume => "resume",
            Self::Derived => "derived",
        }
    }
}

/// Where the person lives, normalized.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileLocation {
    /// As written.
    pub raw: String,
    /// The most specific recognized area; `None` when unrecognized.
    pub area: Option<Area>,
    pub basis: FactBasis,
}

impl ProfileLocation {
    pub fn country(&self) -> Option<&'static Country> {
        self.area.and_then(|a| a.country())
    }

    pub fn city(&self) -> Option<&'static str> {
        match self.area {
            Some(Area::City { name, .. }) => Some(name),
            _ => None,
        }
    }
}

/// Everything eligibility needs from a profile.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProfileFacts {
    /// `prof_…`, when read from a stored profile.
    pub profile_id: Option<String>,
    /// The profile revision these facts were read from.
    pub revision: u64,
    pub location: Option<ProfileLocation>,
    /// Time zones the person stated they can work in.
    pub zones: Vec<(String, Offsets)>,
    pub work_modes: Vec<(WorkMode, Stance)>,
    pub relocation: Option<bool>,
    /// When willing to relocate only to some places: those places,
    /// normalized (unrecognized ones kept as written). Empty is anywhere.
    pub relocation_only_to: Vec<(Option<Area>, String)>,
    pub needs_sponsorship: Option<bool>,
    /// Places the person stated they may already work in, normalized
    /// (unrecognized ones kept as written).
    pub authorized_in: Vec<(Option<Area>, String)>,
    pub engagements: Vec<(Engagement, Stance)>,
}

impl ProfileFacts {
    /// Reads the active preferences of a profile.
    pub fn from_profile(data: &ProfileData) -> Self {
        let mut out = Self {
            profile_id: Some(data.profile.id.to_string()),
            revision: data.profile.revision,
            ..Self::default()
        };
        for p in data.preferences().active() {
            match &p.value {
                PreferenceValue::CurrentLocation { place } => {
                    out.location = Some(ProfileLocation {
                        raw: place.clone(),
                        area: place_area(place),
                        basis: FactBasis::Preference,
                    });
                }
                PreferenceValue::WorkMode { mode } => out.work_modes.push((*mode, p.stance)),
                PreferenceValue::Timezone { zone } if p.stance != Stance::Unwanted => {
                    let ranges = zones_in(zone);
                    if ranges.is_empty() {
                        // "Europe" alone, as a zone.
                        if let Some(range) = place_area(zone).and_then(|a| a.utc_offsets()) {
                            out.zones.push((zone.clone(), range));
                        }
                    } else {
                        out.zones.extend(ranges);
                    }
                }
                PreferenceValue::Relocation { willing, only_to } => {
                    out.relocation = Some(*willing);
                    out.relocation_only_to = only_to
                        .iter()
                        .map(|place| (place_area(place), place.clone()))
                        .collect();
                }
                PreferenceValue::Sponsorship { needed } => out.needs_sponsorship = Some(*needed),
                PreferenceValue::WorkAuthorization { place } => {
                    out.authorized_in.push((place_area(place), place.clone()));
                }
                PreferenceValue::Engagement { engagement } => {
                    out.engagements.push((*engagement, p.stance));
                }
                PreferenceValue::Timezone { .. }
                | PreferenceValue::Role { .. }
                | PreferenceValue::Compensation { .. }
                | PreferenceValue::Region { .. }
                | PreferenceValue::Company { .. }
                | PreferenceValue::Domain { .. }
                | PreferenceValue::WorkStyle { .. }
                | PreferenceValue::UnknownPay { .. }
                | PreferenceValue::UnclearEligibility { .. } => {}
            }
        }
        if out.location.is_none()
            && let Some(location) = &data.profile.location
        {
            out.location = Some(ProfileLocation {
                raw: location.clone(),
                area: place_area(location),
                basis: FactBasis::Resume,
            });
        }
        out
    }

    /// A profile that only says where the person lives (tests, examples).
    pub fn living_in(place: &str) -> Self {
        Self {
            location: Some(ProfileLocation {
                raw: place.to_owned(),
                area: place_area(place),
                basis: FactBasis::Preference,
            }),
            ..Self::default()
        }
    }

    pub fn country(&self) -> Option<&'static Country> {
        self.location.as_ref().and_then(ProfileLocation::country)
    }

    /// The person's working time zones: stated ones, or those of where
    /// they live (city, else country), with how it is known.
    pub fn zone_offsets(&self) -> Option<(Offsets, FactBasis)> {
        if let Some(range) = span(&self.zones.iter().map(|(_, r)| *r).collect::<Vec<_>>()) {
            return Some((range, FactBasis::Preference));
        }
        self.location
            .as_ref()
            .and_then(|l| l.area)
            .and_then(|a| a.utc_offsets())
            .map(|r| (r, FactBasis::Derived))
    }

    /// The stance on a work mode, when stated.
    pub fn stance_on(&self, mode: WorkMode) -> Option<Stance> {
        self.work_modes
            .iter()
            .find(|(m, _)| *m == mode)
            .map(|(_, s)| *s)
    }

    /// Work modes the person requires.
    pub fn required_modes(&self) -> Vec<WorkMode> {
        self.work_modes
            .iter()
            .filter(|(_, s)| *s == Stance::Required)
            .map(|(m, _)| *m)
            .collect()
    }

    /// Whether the person would relocate to an office in `place`:
    /// `Some(Yes)` when willing (anywhere, or `place` is certainly within
    /// one of the places they named); `Some(No)` when not willing, or when
    /// `place` is certainly outside every place they named; `Some(Maybe)`
    /// when that can't be established (a place Narrow doesn't recognize, or
    /// an uncertain membership): unknown stays unknown. `None` when they
    /// haven't said.
    pub fn would_relocate_to(&self, place: Area) -> Option<Membership> {
        match self.relocation? {
            false => Some(Membership::No),
            true if self.relocation_only_to.is_empty() => Some(Membership::Yes),
            true => {
                let each: Vec<Membership> = self
                    .relocation_only_to
                    .iter()
                    .map(|(area, _)| match area {
                        None => Membership::Maybe,
                        Some(a) => destination(*a, place),
                    })
                    .collect();
                Some(if each.contains(&Membership::Yes) {
                    Membership::Yes
                } else if each.iter().all(|m| *m == Membership::No) {
                    Membership::No
                } else {
                    Membership::Maybe
                })
            }
        }
    }

    /// The places the person would only relocate to, as written.
    pub fn relocation_places(&self) -> Vec<String> {
        self.relocation_only_to
            .iter()
            .map(|(_, raw)| raw.clone())
            .collect()
    }

    pub fn engagement(&self, engagement: Engagement) -> Option<Stance> {
        self.engagements
            .iter()
            .find(|(e, _)| *e == engagement)
            .map(|(_, s)| *s)
    }

    /// Whether the person stated they may work in `country` (directly, or
    /// through a region such as "the EU" they named).
    pub fn authorized_for(&self, country: &Country) -> Option<String> {
        self.authorized_in.iter().find_map(|(area, raw)| {
            area.filter(|a| a.contains(country) == crate::geo::Membership::Yes)
                .map(|_| raw.clone())
        })
    }
}

/// Whether an office in `place` is within a relocation destination:
/// `Yes` when certainly, `No` only when certainly not, and `Maybe` when the
/// places Narrow knows can't settle it (a city against its own country or
/// state, or an office named only by a broad region that may include the
/// destination). Missing detail is never a conflict.
fn destination(allowed: Area, place: Area) -> Membership {
    if allowed == place || allowed == Area::Worldwide {
        return Membership::Yes;
    }
    // An office named only by a region (or "anywhere"): it may be in the
    // destination if the region may include any country of it.
    if place.country().is_none() {
        let possible = |c: &Country| place.contains(c) != Membership::No;
        let may = match allowed.country() {
            Some(c) => possible(c),
            None => COUNTRIES
                .iter()
                .any(|c| allowed.contains(c) != Membership::No && possible(c)),
        };
        return if may {
            Membership::Maybe
        } else {
            Membership::No
        };
    }
    match (allowed, place) {
        // Another city is another place.
        (Area::City { .. }, Area::City { .. }) => Membership::No,
        // A city or state named as the destination, and an office elsewhere
        // in the same country (or a state and a city of it): can't say.
        (Area::City { country: a, .. } | Area::Subdivision { country: a, .. }, _) => {
            match place.country() {
                Some(c) if c.code == a.code => Membership::Maybe,
                _ => Membership::No,
            }
        }
        _ => match place.country() {
            Some(c) => allowed.contains(c),
            None => Membership::Maybe,
        },
    }
}

/// The most specific recognized area in a place description ("São Paulo,
/// Brazil" → the city; "the US" → the country).
pub fn place_area(text: &str) -> Option<Area> {
    let text = text.trim();
    let text = text
        .strip_prefix("the ")
        .or_else(|| text.strip_prefix("The "))
        .unwrap_or(text);
    parse_places(text)
        .iter()
        .filter_map(|p| p.area)
        .max_by_key(|a| match a {
            Area::City { .. } => 3,
            Area::Subdivision { .. } => 2,
            Area::Country(_) => 1,
            _ => 0,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_places_people_write() {
        let area = place_area("São Paulo, Brazil").unwrap();
        assert_eq!(area.country().unwrap().code, "BR");
        assert!(matches!(area, Area::City { .. }));
        assert_eq!(place_area("Lisbon").unwrap().country().unwrap().code, "PT");
        assert_eq!(
            place_area("Porto Alegre, RS")
                .unwrap()
                .country()
                .unwrap()
                .code,
            "BR"
        );
        assert_eq!(place_area("the US").unwrap().country().unwrap().code, "US");
        assert_eq!(place_area("Atlantis"), None);
    }

    #[test]
    fn time_zones_come_from_the_city_when_known() {
        let sf = ProfileFacts::living_in("San Francisco, CA");
        assert_eq!(sf.zone_offsets(), Some(((-480, -480), FactBasis::Derived)));
        let us = ProfileFacts::living_in("United States");
        assert_eq!(us.zone_offsets(), Some(((-600, -300), FactBasis::Derived)));
        let mut stated = ProfileFacts::living_in("United States");
        stated.zones = zones_in("UTC-3");
        assert_eq!(
            stated.zone_offsets(),
            Some(((-180, -180), FactBasis::Preference))
        );
        assert_eq!(ProfileFacts::default().zone_offsets(), None);
    }
}
