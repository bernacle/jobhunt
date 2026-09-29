//! The structured controls of the Preferences page: work setup,
//! relocation, where the person lives and may work, remote geography, pay,
//! and company and team, each with the layer it is in.
//!
//! There is no second preference system here. Every control is *read*
//! from the same active [`Preference`] records that a statement in the
//! person's words, `jobhunt preferences set` and the MCP
//! `update_preferences` tool write, and every control is *changed* by
//! sending those same [`PreferenceInput`](crate::preferences::PreferenceInput)s.
//! So a sentence ("remote from Brazil, at least USD 140k, prefer small
//! teams") shows up in the controls, and editing a control replaces what
//! the sentence set, with the statement itself kept verbatim.
//!
//! The layers:
//!
//! * **requirements**: a posting that states the opposite is left out (a
//!   stated conflict); one that says nothing is unresolved, never a strong
//!   fit, and never taken as meeting it;
//! * **preferences**: they only change the order;
//! * **learned**: taste from decisions, ranking only (see
//!   [`crate::taste_view`]), never shown here as a setting.

use jobhunt_eligibility::geo::{Area, Membership, Region};
use jobhunt_eligibility::profile::{FactBasis, ProfileLocation, place_area};
use jobhunt_eligibility::rules::{area_name, country_name};
use jobhunt_profile::{CompanyTrait, Preference, PreferenceValue, ProfileData, Stance, WorkMode};
use jobhunt_ranking::person::region_area;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::preferences::{PreferenceView, WorkSetupInput, layer};

/// Every structured control, read from the preferences in effect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PreferenceControls {
    pub work: WorkControls,
    pub location: LocationControls,
    pub pay: PayControls,
    pub company: CompanyControls,
}

/// Remote, hybrid or on-site, and relocation: two separate answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WorkControls {
    /// `remote_only`, `prefer_remote`, `hybrid_okay`, `onsite_okay`,
    /// `no_preference`, or `custom` when the stored work modes are none of
    /// those (a statement said "hybrid, not on-site"; see `custom`).
    pub setup: String,
    /// `requirement`, `preference`, or `none` (no preference).
    pub setup_layer: String,
    /// The stored work modes in words, when `setup` is `custom`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<String>,
    /// The work-mode preferences behind the answer.
    pub setup_records: Vec<PreferenceView>,
    /// `not_willing`, `open`, `only_selected` or `unset`.
    pub relocation: String,
    /// The places, when `only_selected`.
    pub relocation_only_to: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relocation_record: Option<PreferenceView>,
}

/// A place the person named, as written and as Narrow reads it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlaceControl {
    pub record: PreferenceView,
    /// As written.
    pub place: String,
    /// How Narrow reads it ("Brazil", "Latin America", "anywhere"); absent
    /// when it doesn't recognize the place, which then never counts as
    /// including anywhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_as: Option<String>,
    /// Stable code of the place (`worldwide`, `americas`, `latam`, `BR`),
    /// when recognized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// Where the person lives and may work, and what to do when eligibility
/// is unclear. The job's own location, its remote scope, the person's work
/// authorization and relocation are separate things, kept apart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LocationControls {
    /// The `current_location` preference, when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_record: Option<PreferenceView>,
    /// Where the person lives, as written (from the preference, else the
    /// resume header).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    /// `preference` or `resume`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_basis: Option<String>,
    /// The country Narrow reads it as ("Brazil"); absent when unrecognized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_country: Option<String>,
    /// Remote scopes that include where the person lives, broadest first
    /// ("anywhere", "the Americas", "Latin America", "South America",
    /// "Brazil"). A posting that says only "Remote" is none of these.
    pub remote_open_to_you: Vec<String>,
    /// Countries or regions the person stated they may already work in.
    pub authorized_in: Vec<PlaceControl>,
    /// Geographies remote roles should be open to, with their stance.
    pub remote_geography: Vec<PlaceControl>,
    /// `show` (marked unresolved; the default) or `hide` (until
    /// eligibility is confirmed).
    pub unclear_eligibility: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unclear_eligibility_record: Option<PreferenceView>,
}

/// One pay figure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PayControl {
    pub record: PreferenceView,
    pub amount: u64,
    /// ISO code; absent when the person's words didn't say (never
    /// assumed: until it is set, pay isn't compared).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// `year`, `month`, `day` or `hour`.
    pub period: String,
    /// `employment` or `contract`; absent for both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applies_to: Option<String>,
}

/// A minimum (a requirement) and a target (a preference), and what to do
/// with jobs whose pay isn't published.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PayControls {
    pub minimum: Vec<PayControl>,
    pub target: Vec<PayControl>,
    /// `show` (marked unresolved; the default) or `hide`.
    pub unknown_pay: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unknown_pay_record: Option<PreferenceView>,
}

/// One kind of team or company.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CompanyControl {
    /// `small_team`, `small_company`, `large_company`, `early_stage`,
    /// `startup`, `scaleup`.
    pub value: String,
    /// `team`, `company_size` or `stage`: a company's size never stands in
    /// for a team's, and neither is a stage.
    pub scope: String,
    /// `must_have`, `nice_to_have`, `avoid` or `off`.
    pub importance: String,
    /// `requirement`, `preference`, or `none` when off.
    pub layer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<PreferenceView>,
}

/// Team size, company size and stage, kept apart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CompanyControls {
    pub items: Vec<CompanyControl>,
}

/// The kinds offered as controls, with their scope. Others stay editable
/// as preferences of their own.
const COMPANY_CONTROLS: [(CompanyTrait, &str); 6] = [
    (CompanyTrait::SmallTeam, "team"),
    (CompanyTrait::SmallCompany, "company_size"),
    (CompanyTrait::LargeCompany, "company_size"),
    (CompanyTrait::EarlyStage, "stage"),
    (CompanyTrait::Startup, "stage"),
    (CompanyTrait::Scaleup, "stage"),
];

fn mode_name(mode: WorkMode) -> &'static str {
    match mode {
        WorkMode::Remote => "remote",
        WorkMode::Hybrid => "hybrid",
        WorkMode::Onsite => "on-site",
    }
}

/// "Brazil", "Latin America", "anywhere", and a stable code.
fn read_place(area: Option<Area>) -> (Option<String>, Option<String>) {
    match area {
        None => (None, None),
        Some(Area::Worldwide) => (Some("anywhere".into()), Some("worldwide".into())),
        Some(Area::Region(r)) => (Some(r.name().to_owned()), Some(r.code().to_owned())),
        Some(a) => (Some(area_name(a)), Some(a.code())),
    }
}

fn place_control(p: &Preference, place: &str, area: Option<Area>) -> PlaceControl {
    let (read_as, code) = read_place(area);
    PlaceControl {
        record: PreferenceView::of(p),
        place: place.to_owned(),
        read_as,
        code,
    }
}

impl PreferenceControls {
    pub fn of(data: &ProfileData) -> Self {
        let active: Vec<&Preference> = data.preferences.iter().filter(|p| p.active).collect();

        // Work setup and relocation.
        let modes: Vec<&Preference> = active
            .iter()
            .copied()
            .filter(|p| matches!(p.value, PreferenceValue::WorkMode { .. }))
            .collect();
        let pairs: Vec<(WorkMode, Stance)> = modes
            .iter()
            .filter_map(|p| match p.value {
                PreferenceValue::WorkMode { mode } => Some((mode, p.stance)),
                _ => None,
            })
            .collect();
        let answer = WorkSetupInput::of(&pairs);
        let setup_layer = if pairs.is_empty() {
            "none"
        } else if pairs.iter().any(|(_, s)| *s == Stance::Required) {
            "requirement"
        } else {
            "preference"
        };
        let custom = answer.is_none().then(|| {
            pairs
                .iter()
                .map(|(m, s)| format!("{} {}", s.as_str(), mode_name(*m)))
                .collect::<Vec<_>>()
                .join(", ")
        });
        let relocation_pref = active
            .iter()
            .copied()
            .find(|p| matches!(p.value, PreferenceValue::Relocation { .. }));
        let (relocation, only_to) = match relocation_pref.map(|p| &p.value) {
            Some(PreferenceValue::Relocation { willing: false, .. }) => ("not_willing", Vec::new()),
            Some(PreferenceValue::Relocation {
                willing: true,
                only_to,
            }) if !only_to.is_empty() => ("only_selected", only_to.clone()),
            Some(_) => ("open", Vec::new()),
            None => ("unset", Vec::new()),
        };
        let work = WorkControls {
            setup: answer.map_or("custom", WorkSetupInput::as_str).to_owned(),
            setup_layer: setup_layer.to_owned(),
            custom,
            setup_records: modes.iter().map(|p| PreferenceView::of(p)).collect(),
            relocation: relocation.to_owned(),
            relocation_only_to: only_to,
            relocation_record: relocation_pref.map(PreferenceView::of),
        };

        // Where the person lives and may work.
        let home_pref = active
            .iter()
            .copied()
            .find(|p| matches!(p.value, PreferenceValue::CurrentLocation { .. }));
        let (home, home_basis) = match home_pref.map(|p| &p.value) {
            Some(PreferenceValue::CurrentLocation { place }) => {
                (Some(place.clone()), Some("preference"))
            }
            _ => (
                data.profile.location.clone(),
                data.profile.location.as_ref().map(|_| "resume"),
            ),
        };
        // Read as eligibility reads it: a home that could be several places
        // ("Portland") still has the country they share.
        let home_country = home
            .as_deref()
            .map(|h| ProfileLocation::read(h, FactBasis::Preference))
            .and_then(|l| l.country());
        let remote_open_to_you = match home_country {
            None => Vec::new(),
            Some(c) => {
                let mut out = vec!["anywhere".to_owned()];
                // Broadest first: regions with more members come first.
                let mut regions: Vec<Region> = Region::ALL
                    .into_iter()
                    .filter(|r| r.contains(c) == Membership::Yes)
                    .collect();
                regions.sort_by_key(|r| {
                    std::cmp::Reverse(
                        jobhunt_eligibility::geo::COUNTRIES
                            .iter()
                            .filter(|x| r.contains(x) == Membership::Yes)
                            .count(),
                    )
                });
                out.extend(regions.into_iter().map(|r| r.name().to_owned()));
                out.push(country_name(c));
                out
            }
        };
        let authorized_in = active
            .iter()
            .filter_map(|p| match &p.value {
                PreferenceValue::WorkAuthorization { place } => {
                    Some(place_control(p, place, place_area(place)))
                }
                _ => None,
            })
            .collect();
        let remote_geography = active
            .iter()
            .filter_map(|p| match &p.value {
                PreferenceValue::Region { region } => {
                    Some(place_control(p, region, region_area(region)))
                }
                _ => None,
            })
            .collect();
        let policy = |show: Option<bool>| if show == Some(false) { "hide" } else { "show" };
        let unclear_pref = active
            .iter()
            .copied()
            .find(|p| matches!(p.value, PreferenceValue::UnclearEligibility { .. }));
        let location = LocationControls {
            home_record: home_pref.map(PreferenceView::of),
            home,
            home_basis: home_basis.map(str::to_owned),
            home_country: home_country.map(|c| c.name.to_owned()),
            remote_open_to_you,
            authorized_in,
            remote_geography,
            unclear_eligibility: policy(unclear_pref.and_then(|p| match p.value {
                PreferenceValue::UnclearEligibility { show } => Some(show),
                _ => None,
            }))
            .to_owned(),
            unclear_eligibility_record: unclear_pref.map(PreferenceView::of),
        };

        // Pay.
        let mut minimum = Vec::new();
        let mut target = Vec::new();
        for p in &active {
            if let PreferenceValue::Compensation {
                bound,
                amount,
                currency,
                period,
                arrangement,
            } = &p.value
            {
                let control = PayControl {
                    record: PreferenceView::of(p),
                    amount: *amount,
                    currency: currency.clone(),
                    period: period.as_str().to_owned(),
                    applies_to: arrangement.map(|a| a.as_str().to_owned()),
                };
                match bound {
                    jobhunt_profile::CompensationBound::Minimum => minimum.push(control),
                    jobhunt_profile::CompensationBound::Target => target.push(control),
                }
            }
        }
        let unknown_pref = active
            .iter()
            .copied()
            .find(|p| matches!(p.value, PreferenceValue::UnknownPay { .. }));
        let pay = PayControls {
            minimum,
            target,
            unknown_pay: policy(unknown_pref.and_then(|p| match p.value {
                PreferenceValue::UnknownPay { show } => Some(show),
                _ => None,
            }))
            .to_owned(),
            unknown_pay_record: unknown_pref.map(PreferenceView::of),
        };

        // Company and team.
        let items = COMPANY_CONTROLS
            .iter()
            .map(|(kind, scope)| {
                let record = active.iter().copied().find(
                    |p| matches!(p.value, PreferenceValue::Company { company } if company == *kind),
                );
                let importance = match record.map(|p| p.stance) {
                    Some(Stance::Required) => "must_have",
                    Some(Stance::Wanted | Stance::Acceptable) => "nice_to_have",
                    Some(Stance::Unwanted) => "avoid",
                    None => "off",
                };
                CompanyControl {
                    value: kind.as_str().to_owned(),
                    scope: (*scope).to_owned(),
                    importance: importance.to_owned(),
                    layer: record.map_or("none", layer).to_owned(),
                    record: record.map(PreferenceView::of),
                }
            })
            .collect();

        Self {
            work,
            location,
            pay,
            company: CompanyControls { items },
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use jobhunt_profile::{
        Certainty, CompensationBound, PayPeriod, PreferenceId, PreferenceOrigin, ProfileId,
    };

    use super::*;

    fn pref(value: PreferenceValue, stance: Stance) -> Preference {
        let now = Utc.with_ymd_and_hms(2026, 9, 26, 12, 0, 0).unwrap();
        Preference {
            id: PreferenceId::derive(&[&value.key(), stance.as_str()]),
            value,
            stance,
            origin: PreferenceOrigin::UserEntered,
            statement: None,
            snippet: None,
            certainty: Certainty::Certain,
            note: None,
            active: true,
            superseded_by: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn data(prefs: Vec<Preference>) -> ProfileData {
        let now = Utc.with_ymd_and_hms(2026, 9, 26, 12, 0, 0).unwrap();
        let mut d = ProfileData::new(ProfileId::local(), now);
        d.preferences = prefs;
        d
    }

    #[test]
    fn empty_profile_has_neutral_controls() {
        let c = PreferenceControls::of(&data(Vec::new()));
        assert_eq!(c.work.setup, "no_preference");
        assert_eq!(c.work.setup_layer, "none");
        assert_eq!(c.work.relocation, "unset");
        assert_eq!(c.location.unclear_eligibility, "show");
        assert_eq!(c.pay.unknown_pay, "show");
        assert!(c.location.remote_open_to_you.is_empty());
        assert!(c.company.items.iter().all(|i| i.importance == "off"));
    }

    #[test]
    fn reads_the_brazil_scenario() {
        let c = PreferenceControls::of(&data(vec![
            pref(
                PreferenceValue::CurrentLocation {
                    place: "Brazil".into(),
                },
                Stance::Required,
            ),
            pref(
                PreferenceValue::WorkMode {
                    mode: WorkMode::Remote,
                },
                Stance::Required,
            ),
            pref(
                PreferenceValue::Relocation {
                    willing: false,
                    only_to: Vec::new(),
                },
                Stance::Required,
            ),
            pref(
                PreferenceValue::Compensation {
                    bound: CompensationBound::Minimum,
                    amount: 140_000,
                    currency: Some("USD".into()),
                    period: PayPeriod::Year,
                    arrangement: None,
                },
                Stance::Required,
            ),
            pref(PreferenceValue::UnknownPay { show: true }, Stance::Required),
            pref(
                PreferenceValue::Company {
                    company: CompanyTrait::SmallTeam,
                },
                Stance::Wanted,
            ),
            pref(
                PreferenceValue::Region {
                    region: "Latin America".into(),
                },
                Stance::Wanted,
            ),
        ]));
        assert_eq!(c.work.setup, "remote_only");
        assert_eq!(c.work.setup_layer, "requirement");
        assert_eq!(c.work.relocation, "not_willing");
        assert_eq!(c.location.home_country.as_deref(), Some("Brazil"));
        assert_eq!(c.location.remote_open_to_you[0], "anywhere");
        assert!(
            c.location
                .remote_open_to_you
                .contains(&"Latin America".to_owned())
        );
        assert!(
            c.location
                .remote_open_to_you
                .contains(&"the Americas".to_owned())
        );
        assert_eq!(c.location.remote_open_to_you.last().unwrap(), "Brazil");
        assert_eq!(
            c.location.remote_geography[0].code.as_deref(),
            Some("latam")
        );
        assert_eq!(c.pay.minimum[0].currency.as_deref(), Some("USD"));
        assert_eq!(c.pay.minimum[0].record.layer, "requirement");
        assert_eq!(c.pay.unknown_pay, "show");
        let team = &c.company.items[0];
        assert_eq!(
            (team.value.as_str(), team.scope.as_str()),
            ("small_team", "team")
        );
        assert_eq!(team.importance, "nice_to_have");
        assert_eq!(team.layer, "preference");
        let company = &c.company.items[1];
        assert_eq!(company.importance, "off", "a team is not a company");
    }

    #[test]
    fn work_setup_answers_round_trip_and_odd_mixes_are_custom() {
        for answer in [
            WorkSetupInput::RemoteOnly,
            WorkSetupInput::PreferRemote,
            WorkSetupInput::HybridOkay,
            WorkSetupInput::OnsiteOkay,
            WorkSetupInput::NoPreference,
        ] {
            let prefs = answer
                .modes()
                .into_iter()
                .map(|(mode, stance)| pref(PreferenceValue::WorkMode { mode }, stance))
                .collect();
            assert_eq!(
                PreferenceControls::of(&data(prefs)).work.setup,
                answer.as_str()
            );
        }
        let c = PreferenceControls::of(&data(vec![
            pref(
                PreferenceValue::WorkMode {
                    mode: WorkMode::Hybrid,
                },
                Stance::Wanted,
            ),
            pref(
                PreferenceValue::WorkMode {
                    mode: WorkMode::Onsite,
                },
                Stance::Unwanted,
            ),
        ]));
        assert_eq!(c.work.setup, "custom");
        assert_eq!(
            c.work.custom.as_deref(),
            Some("wanted hybrid, unwanted on-site")
        );
        assert_eq!(c.work.setup_layer, "preference");
    }
}
