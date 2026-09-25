//! The user's side: where they are and what they require, read from the
//! profile's preferences (and, for location only, the resume).

use jobhunt_profile::{
    Arrangement, CompensationBound, PayPeriod, PreferenceValue, ProfileData, Stance, WorkMode,
};

use crate::geo::{Area, Country, parse_places};
use crate::zones::{Offsets, zones_in};

/// Where the user's location came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationBasis {
    /// `jobhunt preferences set location …` or a statement.
    Preference,
    /// The resume header; used only when no preference says otherwise.
    Resume,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UserLocation {
    /// As the user (or resume) wrote it.
    pub raw: String,
    /// `None` when JobHunt does not recognize the place.
    pub area: Option<Area>,
    pub basis: LocationBasis,
}

impl UserLocation {
    pub fn country(&self) -> Option<&'static Country> {
        self.area.and_then(|a| a.country())
    }
}

/// A compensation floor or target.
#[derive(Debug, Clone, PartialEq)]
pub struct MoneyPreference {
    pub bound: CompensationBound,
    pub amount: u64,
    pub currency: Option<String>,
    pub period: PayPeriod,
    pub arrangement: Option<Arrangement>,
    /// As displayed ("at least USD 120,000 per year").
    pub label: String,
}

impl MoneyPreference {
    /// The amount per year.
    pub fn annual(&self) -> f64 {
        annualize(self.amount as f64, self.period)
    }
}

/// An amount per `period`, as a yearly amount (2,080 working hours, 260
/// working days).
pub fn annualize(amount: f64, period: PayPeriod) -> f64 {
    match period {
        PayPeriod::Year => amount,
        PayPeriod::Month => amount * 12.0,
        PayPeriod::Day => amount * 260.0,
        PayPeriod::Hour => amount * 2_080.0,
    }
}

/// Everything eligibility needs from the profile.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserConstraints {
    pub location: Option<UserLocation>,
    pub work_modes: Vec<(WorkMode, Stance)>,
    /// Regions the user named (wanted, acceptable, or unwanted).
    pub regions: Vec<(Option<Area>, String, Stance)>,
    /// Time zones the user can work in.
    pub zones: Vec<(String, Offsets)>,
    pub relocation: Option<bool>,
    pub needs_sponsorship: Option<bool>,
    pub minimums: Vec<MoneyPreference>,
    pub targets: Vec<MoneyPreference>,
    pub wanted_roles: Vec<String>,
    pub unwanted_roles: Vec<String>,
}

impl UserConstraints {
    /// Reads the active preferences of a profile.
    pub fn from_profile(data: &ProfileData) -> Self {
        let mut out = Self::default();
        for p in data.preferences().active() {
            match &p.value {
                PreferenceValue::CurrentLocation { place } => {
                    out.location = Some(UserLocation {
                        raw: place.clone(),
                        area: place_area(place),
                        basis: LocationBasis::Preference,
                    });
                }
                PreferenceValue::WorkMode { mode } => out.work_modes.push((*mode, p.stance)),
                PreferenceValue::Region { region } => {
                    out.regions
                        .push((place_area(region), region.clone(), p.stance));
                }
                PreferenceValue::Timezone { zone } => {
                    let ranges = zones_in(zone);
                    match ranges.is_empty() {
                        false => out.zones.extend(ranges),
                        // "Europe" alone, as a zone.
                        true => {
                            if let Some(range) = place_area(zone).and_then(|a| a.utc_offsets()) {
                                out.zones.push((zone.clone(), range));
                            }
                        }
                    }
                }
                PreferenceValue::Relocation { willing } => out.relocation = Some(*willing),
                PreferenceValue::Sponsorship { needed } => out.needs_sponsorship = Some(*needed),
                PreferenceValue::Compensation {
                    bound,
                    amount,
                    currency,
                    period,
                    arrangement,
                } => {
                    let pref = MoneyPreference {
                        bound: *bound,
                        amount: *amount,
                        currency: currency.clone(),
                        period: *period,
                        arrangement: *arrangement,
                        label: p.value.to_string(),
                    };
                    match bound {
                        CompensationBound::Minimum => out.minimums.push(pref),
                        CompensationBound::Target => out.targets.push(pref),
                    }
                }
                PreferenceValue::Role { role } => match p.stance {
                    Stance::Required | Stance::Wanted | Stance::Acceptable => {
                        out.wanted_roles.push(role.clone());
                    }
                    Stance::Unwanted => out.unwanted_roles.push(role.clone()),
                },
                PreferenceValue::Company { .. }
                | PreferenceValue::Domain { .. }
                | PreferenceValue::WorkStyle { .. } => {}
            }
        }
        if out.location.is_none()
            && let Some(location) = &data.profile.location
        {
            out.location = Some(UserLocation {
                raw: location.clone(),
                area: place_area(location),
                basis: LocationBasis::Resume,
            });
        }
        out
    }

    /// The user's time zones: stated ones, or their country's.
    pub fn zone_offsets(&self) -> Option<(Offsets, bool)> {
        if let Some(range) =
            crate::zones::span(&self.zones.iter().map(|(_, r)| *r).collect::<Vec<_>>())
        {
            return Some((range, true));
        }
        self.location
            .as_ref()
            .and_then(|l| l.area)
            .and_then(|a| a.utc_offsets())
            .map(|r| (r, false))
    }

    /// Whether the user required, wanted, accepted or ruled out a mode.
    pub fn stance_on(&self, mode: WorkMode) -> Option<Stance> {
        self.work_modes
            .iter()
            .filter(|(m, _)| *m == mode)
            .map(|(_, s)| *s)
            .next()
    }
}

/// The most specific recognized area in a place description.
pub fn place_area(text: &str) -> Option<Area> {
    let places = parse_places(text);
    places
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
        assert_eq!(place_area("Atlantis"), None);
        assert_eq!(annualize(10_000.0, PayPeriod::Month), 120_000.0);
        assert_eq!(annualize(75.0, PayPeriod::Hour), 156_000.0);
    }
}
