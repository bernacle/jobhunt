//! Just enough geography to read job locations and user constraints.
//!
//! Two sources, with different jobs:
//!
//! * Narrow's own tables: the business regions job postings use
//!   ("Americas", "LATAM", "EMEA", …) and their membership, the countries
//!   postings name with their aliases and demonyms ("USA", "British"),
//!   state and province codes ("CA", "NSW", "RS"), and the nicknames of
//!   the cities that host most tech jobs ("NYC", "Bay Area"). Sentences of
//!   a description are only ever read against these, so an ordinary word
//!   that happens to be a town somewhere is not taken for a place.
//! * The GeoNames subset ([`crate::gazetteer`]): every country, first-level
//!   region and place of more than 15,000 people, with alternate and native
//!   names and IANA time zones. Location fields and the places a person
//!   states are resolved against it too.
//!
//! A name that fits several places is not guessed at. It resolves to one
//! place only when every other place it could mean has less than a tenth
//! of that one's population, or lies inside it ("New York" the city, not
//! the state; "Singapore" the country): "London" is London, England, but
//! "Cambridge", "Santiago", "San José" and "Georgia" (the country or the
//! US state) stay [`Resolution::Ambiguous`] until the text around them
//! ("Cambridge, MA", "Santiago, Chile") or a source's country field
//! chooses. "Worldwide" and "anywhere" are a scope ([`Area::Worldwide`]),
//! not a place.
//!
//! Region membership has three answers. "North America" certainly includes
//! the United States and Canada, but only *maybe* Mexico: postings disagree,
//! so an assessment built on it is "likely", never "yes". Countries not in
//! the tables are placed by their continent: certainly in the continent's
//! own region ("Europe", "Africa"), maybe in business regions on it.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use jobhunt_core::text::search_key;

use crate::gazetteer::{self, Entry, GAZETTEER, GeoId};
use crate::zones::{Clock, Zone};

/// A country.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Country {
    /// ISO 3166-1 alpha-2.
    pub code: &'static str,
    pub name: &'static str,
    /// Other names and demonyms, lowercase ("usa", "united states", "american").
    aliases: &'static [&'static str],
    /// Currency written with a shared symbol ("$"), when it is one.
    pub dollar: Option<&'static str>,
    /// GeoNames' continent code (`AF`, `AS`, `EU`, `NA`, `OC`, `SA`, `AN`).
    pub continent: &'static str,
    /// In Narrow's own table: its region memberships are listed there, and
    /// its names are read in sentences.
    pub curated: bool,
}

impl fmt::Display for Country {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

macro_rules! country {
    ($code:literal, $name:literal, [$($alias:literal),*]) => {
        Country { code: $code, name: $name, aliases: &[$($alias),*], dollar: None, continent: "", curated: true }
    };
    ($code:literal, $name:literal, [$($alias:literal),*], $dollar:literal) => {
        Country { code: $code, name: $name, aliases: &[$($alias),*], dollar: Some($dollar), continent: "", curated: true }
    };
}

/// The countries postings and people name most, with the names they use.
static CURATED: &[Country] = &[
    country!(
        "US",
        "United States",
        [
            "usa",
            "u s",
            "u s a",
            "united states of america",
            "america",
            "american",
            "us based"
        ],
        "USD"
    ),
    country!("CA", "Canada", ["canadian"], "CAD"),
    country!("MX", "Mexico", ["méxico", "mexican"], "MXN"),
    country!("BR", "Brazil", ["brasil", "brazilian"]),
    country!("AR", "Argentina", ["argentinian", "argentine"]),
    country!("CL", "Chile", ["chilean"]),
    country!("CO", "Colombia", ["colombian"]),
    country!("PE", "Peru", ["perú", "peruvian"]),
    country!("UY", "Uruguay", ["uruguayan"]),
    country!("PY", "Paraguay", []),
    country!("BO", "Bolivia", []),
    country!("EC", "Ecuador", []),
    country!("VE", "Venezuela", []),
    country!("CR", "Costa Rica", []),
    country!("GT", "Guatemala", []),
    country!("PA", "Panama", ["panamá"]),
    country!("DO", "Dominican Republic", []),
    country!(
        "GB",
        "United Kingdom",
        [
            "uk",
            "u k",
            "great britain",
            "britain",
            "england",
            "scotland",
            "wales",
            "british"
        ]
    ),
    country!("IE", "Ireland", ["irish"]),
    country!("PT", "Portugal", ["portuguese"]),
    country!("ES", "Spain", ["españa", "spanish"]),
    country!("FR", "France", ["french"]),
    country!("DE", "Germany", ["deutschland", "german"]),
    country!("NL", "Netherlands", ["the netherlands", "holland", "dutch"]),
    country!("BE", "Belgium", ["belgian"]),
    country!("LU", "Luxembourg", []),
    country!("CH", "Switzerland", ["swiss"]),
    country!("AT", "Austria", ["austrian"]),
    country!("IT", "Italy", ["italian"]),
    country!("PL", "Poland", ["polish"]),
    country!("CZ", "Czechia", ["czech republic", "czech"]),
    country!("SK", "Slovakia", []),
    country!("HU", "Hungary", ["hungarian"]),
    country!("DK", "Denmark", ["danish"]),
    country!("SE", "Sweden", ["swedish"]),
    country!("NO", "Norway", ["norwegian"]),
    country!("FI", "Finland", ["finnish"]),
    country!("IS", "Iceland", []),
    country!("EE", "Estonia", []),
    country!("LV", "Latvia", []),
    country!("LT", "Lithuania", []),
    country!("GR", "Greece", ["greek"]),
    country!("RO", "Romania", ["romanian"]),
    country!("BG", "Bulgaria", []),
    country!("HR", "Croatia", []),
    country!("SI", "Slovenia", []),
    country!("RS", "Serbia", []),
    country!("UA", "Ukraine", ["ukrainian"]),
    country!("CY", "Cyprus", []),
    country!("MT", "Malta", []),
    country!("TR", "Turkey", ["türkiye", "turkiye"]),
    country!("IL", "Israel", ["israeli"]),
    country!("AE", "United Arab Emirates", ["uae", "emirates"]),
    country!("SA", "Saudi Arabia", []),
    country!("EG", "Egypt", []),
    country!("ZA", "South Africa", []),
    country!("NG", "Nigeria", ["nigerian"]),
    country!("KE", "Kenya", []),
    country!("GH", "Ghana", []),
    country!("MA", "Morocco", []),
    country!("IN", "India", ["indian"]),
    country!("PK", "Pakistan", []),
    country!("BD", "Bangladesh", []),
    country!("LK", "Sri Lanka", []),
    country!("SG", "Singapore", [], "SGD"),
    country!("MY", "Malaysia", []),
    country!("ID", "Indonesia", []),
    country!("PH", "Philippines", ["filipino"]),
    country!("VN", "Vietnam", ["viet nam"]),
    country!("TH", "Thailand", []),
    country!("JP", "Japan", ["japanese"]),
    country!("KR", "South Korea", ["korea", "republic of korea"]),
    country!("CN", "China", ["chinese", "mainland china"]),
    country!("HK", "Hong Kong", [], "HKD"),
    country!("TW", "Taiwan", []),
    country!("AU", "Australia", ["australian"], "AUD"),
    country!("NZ", "New Zealand", ["aotearoa"], "NZD"),
    country!("BZ", "Belize", [], "BZD"),
    country!("SV", "El Salvador", ["salvadoran"]),
    country!("HN", "Honduras", []),
    country!("NI", "Nicaragua", []),
    country!("CU", "Cuba", ["cuban"]),
    country!("JM", "Jamaica", ["jamaican"], "JMD"),
    country!("TT", "Trinidad and Tobago", [], "TTD"),
    country!("PR", "Puerto Rico", []),
    country!("GY", "Guyana", [], "GYD"),
    country!("SR", "Suriname", []),
    country!("LI", "Liechtenstein", []),
    country!("BA", "Bosnia and Herzegovina", ["bosnia"]),
    country!("AL", "Albania", []),
    country!("MK", "North Macedonia", ["macedonia"]),
    country!("ME", "Montenegro", []),
    country!("MD", "Moldova", []),
    country!("BY", "Belarus", []),
    country!("RU", "Russia", ["russian federation"]),
    country!("IR", "Iran", []),
    country!("SY", "Syria", []),
    country!("KP", "North Korea", ["dprk"]),
    country!("QA", "Qatar", []),
    country!("KW", "Kuwait", []),
    country!("BH", "Bahrain", []),
    country!("OM", "Oman", []),
    country!("JO", "Jordan", []),
    country!("LB", "Lebanon", []),
    country!("TN", "Tunisia", []),
    country!("DZ", "Algeria", []),
    country!("ET", "Ethiopia", []),
    country!("UG", "Uganda", []),
    country!("TZ", "Tanzania", []),
    country!("RW", "Rwanda", []),
    country!("SN", "Senegal", []),
    country!("CI", "Côte d'Ivoire", ["ivory coast", "cote d ivoire"]),
    country!("CM", "Cameroon", []),
    country!("NP", "Nepal", []),
    country!("KH", "Cambodia", []),
];

/// Every country: Narrow's own, then the rest of GeoNames' (ISO 3166).
pub static COUNTRIES: LazyLock<Vec<Country>> = LazyLock::new(|| {
    let g = &*GAZETTEER;
    let continent = |code: &str| g.country(code).map_or("", |c| c.continent);
    let mut out: Vec<Country> = CURATED
        .iter()
        .map(|c| Country {
            continent: continent(c.code),
            ..*c
        })
        .collect();
    for c in &g.countries {
        if !CURATED.iter().any(|x| x.code == c.code) {
            out.push(Country {
                code: c.code,
                name: c.name,
                aliases: &[],
                dollar: None,
                continent: c.continent,
                curated: false,
            });
        }
    }
    out
});

static COUNTRY_INDEX: LazyLock<HashMap<&'static str, &'static Country>> =
    LazyLock::new(|| COUNTRIES.iter().map(|c| (c.code, c)).collect());

/// A country by its ISO 3166-1 alpha-2 code (any case).
pub fn country(code: &str) -> Option<&'static Country> {
    if code.len() != 2 {
        return None;
    }
    COUNTRY_INDEX
        .get(code.to_ascii_uppercase().as_str())
        .copied()
}

/// Business regions job postings name.
///
/// Membership is defined in one place, [`Region::contains`], from the
/// tables below; see the README ("Region definitions") for the product
/// definitions and the reasoning behind every "maybe".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Region {
    NorthAmerica,
    LatinAmerica,
    SouthAmerica,
    CentralAmerica,
    Caribbean,
    Americas,
    Europe,
    EuropeanUnion,
    /// European Economic Area: the EU plus Iceland, Liechtenstein, Norway.
    Eea,
    Nordics,
    Dach,
    Emea,
    MiddleEast,
    Africa,
    Apac,
    Asia,
    Oceania,
}

/// Whether a region includes a country.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Membership {
    Yes,
    /// Usage disagrees (Mexico in "North America", the UK in "EU" as
    /// postings loosely use it); an answer built on it is uncertain.
    Maybe,
    No,
}

// Building blocks, as ISO 3166-1 alpha-2 codes.
const EU: &[&str] = &[
    "AT", "BE", "BG", "HR", "CY", "CZ", "DK", "EE", "FI", "FR", "DE", "GR", "HU", "IE", "IT", "LV",
    "LT", "LU", "MT", "NL", "PL", "PT", "RO", "SK", "SI", "ES", "SE",
];
/// EEA members outside the EU.
const EEA_EXTRA: &[&str] = &["IS", "LI", "NO"];
/// Other European countries: in "Europe", not in the EU or EEA.
const EUROPE_OTHER: &[&str] = &["GB", "CH", "RS", "UA", "BA", "AL", "MK", "ME", "MD"];
/// Transcontinental or disputed as "Europe".
const EUROPE_MAYBE: &[&str] = &["TR", "RU", "BY", "GE", "AM", "AZ"];
/// Between Europe and Asia.
const CAUCASUS: &[&str] = &["GE", "AM", "AZ"];
const SOUTH_AMERICA: &[&str] = &[
    "AR", "BO", "BR", "CL", "CO", "EC", "GY", "PY", "PE", "SR", "UY", "VE",
];
const CENTRAL_AMERICA: &[&str] = &["BZ", "CR", "SV", "GT", "HN", "NI", "PA"];
const CARIBBEAN: &[&str] = &["CU", "DO", "JM", "TT", "PR"];
/// Americas countries whose inclusion in "Latin America" usage disagrees
/// on (not Spanish- or Portuguese-speaking).
const LATAM_MAYBE: &[&str] = &["BZ", "GY", "SR", "JM", "TT"];
const MIDDLE_EAST: &[&str] = &[
    "AE", "SA", "IL", "QA", "KW", "BH", "OM", "JO", "LB", "SY", "IR", "IQ", "YE", "PS",
];
const MIDDLE_EAST_MAYBE: &[&str] = &["TR", "EG"];
const AFRICA: &[&str] = &[
    "ZA", "NG", "KE", "GH", "MA", "EG", "TN", "DZ", "ET", "UG", "TZ", "RW", "SN", "CI", "CM",
];
const EAST_ASIA: &[&str] = &["CN", "JP", "KR", "KP", "HK", "TW"];
const SOUTHEAST_ASIA: &[&str] = &["SG", "MY", "ID", "PH", "VN", "TH", "KH"];
const SOUTH_ASIA: &[&str] = &["IN", "PK", "BD", "LK", "NP"];
const OCEANIA: &[&str] = &["AU", "NZ"];

impl Region {
    pub const ALL: [Region; 17] = [
        Self::NorthAmerica,
        Self::LatinAmerica,
        Self::SouthAmerica,
        Self::CentralAmerica,
        Self::Caribbean,
        Self::Americas,
        Self::Europe,
        Self::EuropeanUnion,
        Self::Eea,
        Self::Nordics,
        Self::Dach,
        Self::Emea,
        Self::MiddleEast,
        Self::Africa,
        Self::Apac,
        Self::Asia,
        Self::Oceania,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::NorthAmerica => "North America",
            Self::LatinAmerica => "Latin America",
            Self::SouthAmerica => "South America",
            Self::CentralAmerica => "Central America",
            Self::Caribbean => "the Caribbean",
            Self::Americas => "the Americas",
            Self::Europe => "Europe",
            Self::EuropeanUnion => "the EU",
            Self::Eea => "the EEA",
            Self::Nordics => "the Nordics",
            Self::Dach => "DACH",
            Self::Emea => "EMEA",
            Self::MiddleEast => "the Middle East",
            Self::Africa => "Africa",
            Self::Apac => "APAC",
            Self::Asia => "Asia",
            Self::Oceania => "Oceania",
        }
    }

    /// Stable identifier (`latam`, `eu`, `emea`, …).
    pub fn code(self) -> &'static str {
        match self {
            Self::NorthAmerica => "north_america",
            Self::LatinAmerica => "latam",
            Self::SouthAmerica => "south_america",
            Self::CentralAmerica => "central_america",
            Self::Caribbean => "caribbean",
            Self::Americas => "americas",
            Self::Europe => "europe",
            Self::EuropeanUnion => "eu",
            Self::Eea => "eea",
            Self::Nordics => "nordics",
            Self::Dach => "dach",
            Self::Emea => "emea",
            Self::MiddleEast => "middle_east",
            Self::Africa => "africa",
            Self::Apac => "apac",
            Self::Asia => "asia",
            Self::Oceania => "oceania",
        }
    }

    fn names(self) -> &'static [&'static str] {
        match self {
            Self::NorthAmerica => &[
                "north america",
                "north american",
                "north americas",
                "us canada",
                "us and canada",
                "usa canada",
            ],
            Self::LatinAmerica => &[
                "latin america",
                "latam",
                "lat am",
                "latinoamérica",
                "latinoamerica",
                "america latina",
                "américa latina",
            ],
            Self::SouthAmerica => &["south america", "south american"],
            Self::CentralAmerica => &["central america"],
            Self::Caribbean => &["caribbean", "the caribbean"],
            Self::Americas => &["americas", "the americas", "amer"],
            Self::Europe => &["europe", "european", "eu uk", "uk eu"],
            Self::EuropeanUnion => &["eu", "european union"],
            Self::Eea => &["eea", "european economic area", "eu eea"],
            Self::Nordics => &["nordics", "nordic", "scandinavia", "scandinavian"],
            Self::Dach => &["dach"],
            Self::Emea => &["emea"],
            Self::MiddleEast => &["middle east", "mena"],
            Self::Africa => &["africa", "african"],
            Self::Apac => &["apac", "asia pacific", "asia-pacific", "apj"],
            Self::Asia => &["asia", "asian"],
            Self::Oceania => &["oceania", "anz", "australia and new zealand"],
        }
    }

    /// The region's members: certain ones and disputed ones. Every
    /// membership decision in JobHunt comes from here.
    fn members(self) -> (Vec<&'static str>, Vec<&'static str>) {
        let cat = |lists: &[&[&'static str]]| -> Vec<&'static str> {
            lists.iter().flat_map(|l| l.iter().copied()).collect()
        };
        match self {
            Self::NorthAmerica => (vec!["US", "CA"], vec!["MX"]),
            Self::CentralAmerica => (cat(&[CENTRAL_AMERICA]), vec!["MX"]),
            Self::SouthAmerica => (cat(&[SOUTH_AMERICA]), vec![]),
            Self::Caribbean => (cat(&[CARIBBEAN]), vec![]),
            Self::LatinAmerica => {
                let yes = cat(&[&["MX"], CENTRAL_AMERICA, SOUTH_AMERICA, CARIBBEAN])
                    .into_iter()
                    .filter(|c| !LATAM_MAYBE.contains(c))
                    .collect();
                (yes, cat(&[LATAM_MAYBE]))
            }
            Self::Americas => (
                cat(&[
                    &["US", "CA", "MX"],
                    CENTRAL_AMERICA,
                    SOUTH_AMERICA,
                    CARIBBEAN,
                ]),
                vec![],
            ),
            Self::Europe => (cat(&[EU, EEA_EXTRA, EUROPE_OTHER]), cat(&[EUROPE_MAYBE])),
            // "EU" in postings is often used loosely for Europe.
            Self::EuropeanUnion => (cat(&[EU]), cat(&[EEA_EXTRA, EUROPE_OTHER])),
            Self::Eea => (cat(&[EU, EEA_EXTRA]), vec![]),
            Self::Nordics => (
                vec!["SE", "NO", "DK", "FI", "IS"],
                vec!["FO", "AX", "GL", "SJ"],
            ),
            Self::Dach => (vec!["DE", "AT", "CH"], vec!["LI"]),
            Self::MiddleEast => (cat(&[MIDDLE_EAST]), cat(&[MIDDLE_EAST_MAYBE])),
            Self::Africa => (cat(&[AFRICA]), vec![]),
            Self::Emea => (
                cat(&[
                    EU,
                    EEA_EXTRA,
                    EUROPE_OTHER,
                    EUROPE_MAYBE,
                    MIDDLE_EAST,
                    MIDDLE_EAST_MAYBE,
                    AFRICA,
                ]),
                vec![],
            ),
            Self::Asia => (
                cat(&[EAST_ASIA, SOUTHEAST_ASIA, SOUTH_ASIA]),
                cat(&[MIDDLE_EAST, &["TR"], CAUCASUS]),
            ),
            Self::Apac => (
                cat(&[EAST_ASIA, SOUTHEAST_ASIA, OCEANIA, &["IN"]]),
                vec!["PK", "BD", "LK", "NP"],
            ),
            Self::Oceania => (cat(&[OCEANIA]), vec![]),
        }
    }

    /// Whether the region includes `country`: from the lists above, or,
    /// for a country they don't cover, from its continent.
    pub fn contains(self, country: &Country) -> Membership {
        type Members = (HashSet<&'static str>, HashSet<&'static str>);
        static MEMBERS: LazyLock<HashMap<Region, Members>> = LazyLock::new(|| {
            Region::ALL
                .into_iter()
                .map(|r| {
                    let (yes, maybe) = r.members();
                    (r, (yes.into_iter().collect(), maybe.into_iter().collect()))
                })
                .collect()
        });
        let Some((yes, maybe)) = MEMBERS.get(&self) else {
            return Membership::No;
        };
        if yes.contains(country.code) {
            Membership::Yes
        } else if maybe.contains(country.code) {
            Membership::Maybe
        } else if country.curated {
            Membership::No
        } else {
            self.by_continent(country.continent)
        }
    }

    /// A country the lists don't cover, by its continent: certainly in the
    /// continent's own region, maybe in a business region on it, and never
    /// in a formal list (the EEA, DACH).
    fn by_continent(self, continent: &str) -> Membership {
        use Membership::{Maybe, No, Yes};
        match (self, continent) {
            (Self::Americas, "NA" | "SA")
            | (Self::SouthAmerica, "SA")
            | (Self::Europe, "EU")
            | (Self::Africa, "AF")
            | (Self::Emea, "EU" | "AF")
            | (Self::Asia, "AS")
            | (Self::Oceania | Self::Apac, "OC") => Yes,
            (Self::NorthAmerica | Self::CentralAmerica | Self::Caribbean, "NA")
            | (Self::LatinAmerica, "NA" | "SA")
            | (Self::EuropeanUnion, "EU")
            | (Self::Emea | Self::MiddleEast | Self::Apac, "AS") => Maybe,
            _ => No,
        }
    }

    /// The time zones of the countries the region certainly includes.
    pub fn zone(self) -> Option<Zone> {
        static ZONES: LazyLock<Mutex<HashMap<Region, Option<Zone>>>> =
            LazyLock::new(Mutex::default);
        if let Some(found) = ZONES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&self)
        {
            return found.clone();
        }
        let zone = Zone::new(
            COUNTRIES
                .iter()
                .filter(|c| self.contains(c) == Membership::Yes)
                .flat_map(|c| country_clocks(c.code)),
        );
        ZONES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(self, zone.clone());
        zone
    }
}

/// The IANA zones of a country.
fn country_clocks(code: &str) -> Vec<Clock> {
    GAZETTEER
        .country(code)
        .map(|c| c.zones.iter().map(|z| Clock::Iana(*z)).collect())
        .unwrap_or_default()
}

/// The time zones of a country (remembered).
pub fn country_zone(country: &Country) -> Option<Zone> {
    static ZONES: LazyLock<Mutex<HashMap<&'static str, Option<Zone>>>> =
        LazyLock::new(Mutex::default);
    let code = self::country(country.code)?.code;
    if let Some(found) = ZONES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(code)
    {
        return found.clone();
    }
    let zone = Zone::new(country_clocks(code));
    ZONES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(code, zone.clone());
    zone
}

/// First-level subdivisions that postings use to qualify a city, with the
/// codes postings write ("CA", "ON", "NSW", "RS").
struct Subdivision {
    country: &'static str,
    code: &'static str,
    name: &'static str,
}

macro_rules! subdivisions {
    ($country:literal: $(($code:literal, $name:literal)),* $(,)?) => {
        &[$(Subdivision { country: $country, code: $code, name: $name }),*]
    };
}

static US_STATES: &[Subdivision] = subdivisions!("US":
    ("AL", "Alabama"), ("AK", "Alaska"), ("AZ", "Arizona"), ("AR", "Arkansas"),
    ("CA", "California"), ("CO", "Colorado"), ("CT", "Connecticut"), ("DE", "Delaware"),
    ("DC", "District of Columbia"), ("FL", "Florida"), ("GA", "Georgia"), ("HI", "Hawaii"),
    ("ID", "Idaho"), ("IL", "Illinois"), ("IN", "Indiana"), ("IA", "Iowa"), ("KS", "Kansas"),
    ("KY", "Kentucky"), ("LA", "Louisiana"), ("ME", "Maine"), ("MD", "Maryland"),
    ("MA", "Massachusetts"), ("MI", "Michigan"), ("MN", "Minnesota"), ("MS", "Mississippi"),
    ("MO", "Missouri"), ("MT", "Montana"), ("NE", "Nebraska"), ("NV", "Nevada"),
    ("NH", "New Hampshire"), ("NJ", "New Jersey"), ("NM", "New Mexico"), ("NY", "New York State"),
    ("NC", "North Carolina"), ("ND", "North Dakota"), ("OH", "Ohio"), ("OK", "Oklahoma"),
    ("OR", "Oregon"), ("PA", "Pennsylvania"), ("RI", "Rhode Island"), ("SC", "South Carolina"),
    ("SD", "South Dakota"), ("TN", "Tennessee"), ("TX", "Texas"), ("UT", "Utah"),
    ("VT", "Vermont"), ("VA", "Virginia"), ("WA", "Washington State"), ("WV", "West Virginia"),
    ("WI", "Wisconsin"), ("WY", "Wyoming"),
);
static CA_PROVINCES: &[Subdivision] = subdivisions!("CA":
    ("ON", "Ontario"), ("QC", "Quebec"), ("BC", "British Columbia"), ("AB", "Alberta"),
    ("MB", "Manitoba"), ("SK", "Saskatchewan"), ("NS", "Nova Scotia"), ("NB", "New Brunswick"),
    ("NL", "Newfoundland and Labrador"), ("PE", "Prince Edward Island"),
);
static AU_STATES: &[Subdivision] = subdivisions!("AU":
    ("NSW", "New South Wales"), ("VIC", "Victoria"), ("QLD", "Queensland"),
    ("TAS", "Tasmania"), ("ACT", "Australian Capital Territory"),
);
// Brazilian states. Codes shared with US states ("MS", "PA") mean the
// Brazilian state only after a Brazilian place ("Dourados, MS"); alone,
// the US state (listed first) wins.
static BR_STATES: &[Subdivision] = subdivisions!("BR":
    ("SP", "São Paulo State"), ("RJ", "Rio de Janeiro State"), ("RS", "Rio Grande do Sul"),
    ("MG", "Minas Gerais"), ("PR", "Paraná"), ("SC", "Santa Catarina"), ("BA", "Bahia"),
    ("PE", "Pernambuco"), ("CE", "Ceará"), ("DF", "Distrito Federal"), ("GO", "Goiás"),
    ("ES", "Espírito Santo"), ("AC", "Acre"), ("AL", "Alagoas"), ("AP", "Amapá"),
    ("AM", "Amazonas"), ("MA", "Maranhão"), ("MT", "Mato Grosso"), ("MS", "Mato Grosso do Sul"),
    ("PA", "Pará"), ("PB", "Paraíba"), ("PI", "Piauí"), ("RN", "Rio Grande do Norte"),
    ("RO", "Rondônia"), ("RR", "Roraima"), ("SE", "Sergipe"), ("TO", "Tocantins"),
);

static SUBDIVISION_LISTS: [&[Subdivision]; 4] = [US_STATES, CA_PROVINCES, AU_STATES, BR_STATES];

/// Each listed subdivision's GeoNames region, found by name within its
/// country ("São Paulo State" is GeoNames' "São Paulo").
static SUBDIVISION_IDS: LazyLock<HashMap<(&'static str, &'static str), GeoId>> =
    LazyLock::new(|| {
        let g = &*GAZETTEER;
        let mut out = HashMap::new();
        for sub in SUBDIVISION_LISTS.iter().flat_map(|l| l.iter()) {
            let name = sub.name.strip_suffix(" State").unwrap_or(sub.name);
            let id = g.lookup(name).iter().find_map(|e| match e {
                Entry::Admin1(i) if g.admin1[*i].country == sub.country => Some(g.admin1[*i].id),
                _ => None,
            });
            if let Some(id) = id {
                out.insert((sub.country, sub.code), id);
            }
        }
        out
    });

/// The names Narrow's lists give GeoNames regions ("São Paulo State").
static SUBDIVISION_NAMES: LazyLock<HashMap<GeoId, &'static str>> = LazyLock::new(|| {
    SUBDIVISION_LISTS
        .iter()
        .flat_map(|l| l.iter())
        .filter_map(|s| {
            SUBDIVISION_IDS
                .get(&(s.country, s.code))
                .map(|id| (*id, s.name))
        })
        .collect()
});

fn subdivision_area(sub: &'static Subdivision) -> Option<Area> {
    Some(Area::Subdivision {
        country: country(sub.country)?,
        name: sub.name,
        id: SUBDIVISION_IDS
            .get(&(sub.country, sub.code))
            .copied()
            .unwrap_or_default(),
    })
}

/// A city Narrow names itself: its display name and the nicknames postings
/// use ("NYC", "Bay Area").
struct City {
    names: &'static [&'static str],
    display: &'static str,
    country: &'static str,
}

macro_rules! cities {
    ($(($country:literal, $display:literal $(, $alias:literal)*)),* $(,)?) => {
        &[$(City { names: &[$display $(, $alias)*], display: $display, country: $country }),*]
    };
}

static CITIES: &[City] = cities!(
    (
        "US",
        "San Francisco",
        "sf",
        "san francisco bay area",
        "sf bay area",
        "bay area"
    ),
    ("US", "New York", "new york city", "nyc", "ny"),
    ("US", "Seattle"),
    ("US", "Austin"),
    ("US", "Boston"),
    ("US", "Chicago"),
    ("US", "Denver"),
    ("US", "Los Angeles", "la"),
    ("US", "Miami"),
    ("US", "Atlanta"),
    (
        "US",
        "Washington, D.C.",
        "washington dc",
        "washington d c",
        "dc"
    ),
    ("US", "San Jose"),
    ("US", "Palo Alto"),
    ("US", "Mountain View"),
    ("US", "Menlo Park"),
    ("US", "Oakland"),
    ("US", "Berkeley"),
    ("US", "Sunnyvale"),
    ("US", "San Mateo"),
    ("US", "San Diego"),
    ("US", "Portland"),
    ("US", "Philadelphia"),
    ("US", "Pittsburgh"),
    ("US", "Salt Lake City"),
    ("US", "Dallas"),
    ("US", "Houston"),
    ("US", "Phoenix"),
    ("US", "Minneapolis"),
    ("US", "Nashville"),
    ("US", "Raleigh"),
    ("US", "Boulder"),
    ("US", "Detroit"),
    ("US", "Baltimore"),
    ("CA", "Toronto"),
    ("CA", "Vancouver"),
    ("CA", "Montreal", "montréal"),
    ("CA", "Ottawa"),
    ("CA", "Calgary"),
    ("CA", "Waterloo"),
    ("MX", "Mexico City", "ciudad de méxico", "cdmx"),
    ("MX", "Guadalajara"),
    ("MX", "Monterrey"),
    ("BR", "São Paulo", "sao paulo"),
    ("BR", "Rio de Janeiro"),
    ("BR", "Belo Horizonte"),
    ("BR", "Porto Alegre"),
    ("BR", "Curitiba"),
    ("BR", "Florianópolis", "florianopolis"),
    ("BR", "Recife"),
    ("BR", "Brasília", "brasilia"),
    ("BR", "Campinas"),
    ("AR", "Buenos Aires"),
    ("CL", "Santiago"),
    ("CO", "Bogotá", "bogota"),
    ("CO", "Medellín", "medellin"),
    ("PE", "Lima"),
    ("UY", "Montevideo"),
    ("GB", "London"),
    ("GB", "Manchester"),
    ("GB", "Edinburgh"),
    ("GB", "Cambridge"),
    ("GB", "Bristol"),
    ("IE", "Dublin"),
    ("DE", "Berlin"),
    ("DE", "Munich", "münchen", "muenchen"),
    ("DE", "Hamburg"),
    ("DE", "Frankfurt"),
    ("DE", "Cologne", "köln"),
    ("NL", "Amsterdam"),
    ("NL", "Rotterdam"),
    ("BE", "Brussels"),
    ("FR", "Paris"),
    ("FR", "Lyon"),
    ("ES", "Madrid"),
    ("ES", "Barcelona"),
    ("ES", "Valencia"),
    ("PT", "Lisbon", "lisboa"),
    ("PT", "Porto"),
    ("IT", "Milan", "milano"),
    ("IT", "Rome", "roma"),
    ("CH", "Zurich", "zürich"),
    ("CH", "Geneva"),
    ("AT", "Vienna", "wien"),
    ("SE", "Stockholm"),
    ("DK", "Copenhagen"),
    ("NO", "Oslo"),
    ("FI", "Helsinki"),
    ("PL", "Warsaw", "warszawa"),
    ("PL", "Kraków", "krakow"),
    ("PL", "Wrocław", "wroclaw"),
    ("CZ", "Prague", "praha"),
    ("HU", "Budapest"),
    ("RO", "Bucharest"),
    ("EE", "Tallinn"),
    ("GR", "Athens"),
    ("UA", "Kyiv", "kiev"),
    ("IL", "Tel Aviv"),
    ("AE", "Dubai"),
    ("TR", "Istanbul"),
    ("IN", "Bengaluru", "bangalore"),
    ("IN", "Hyderabad"),
    ("IN", "Mumbai"),
    ("IN", "Pune"),
    ("IN", "Delhi", "new delhi"),
    ("IN", "Gurgaon", "gurugram"),
    ("IN", "Chennai"),
    ("IN", "Noida"),
    ("SG", "Singapore"),
    ("JP", "Tokyo"),
    ("KR", "Seoul"),
    ("CN", "Shanghai"),
    ("CN", "Beijing"),
    ("CN", "Shenzhen"),
    ("HK", "Hong Kong"),
    ("TW", "Taipei"),
    ("ID", "Jakarta"),
    ("PH", "Manila"),
    ("VN", "Ho Chi Minh City", "ho chi minh", "saigon"),
    ("TH", "Bangkok"),
    ("MY", "Kuala Lumpur"),
    ("AU", "Sydney"),
    ("AU", "Melbourne"),
    ("AU", "Brisbane"),
    ("AU", "Perth"),
    ("NZ", "Auckland"),
    ("NZ", "Wellington"),
    ("ZA", "Cape Town"),
    ("ZA", "Johannesburg"),
    ("NG", "Lagos"),
    ("KE", "Nairobi"),
    ("EG", "Cairo"),
);

/// Each listed city's GeoNames place: the most populous place of its
/// country with one of its names.
static CITY_IDS: LazyLock<HashMap<&'static str, GeoId>> = LazyLock::new(|| {
    let g = &*GAZETTEER;
    let mut out = HashMap::new();
    for city in CITIES {
        let found = city
            .names
            .iter()
            .flat_map(|n| g.lookup(n))
            .filter_map(|e| match e {
                Entry::City(i) if g.cities[*i].country == city.country => Some(&g.cities[*i]),
                _ => None,
            })
            .max_by_key(|c| (c.population, std::cmp::Reverse(c.id)));
        if let Some(c) = found {
            out.insert(city.display, c.id);
        }
    }
    out
});

/// The names Narrow's list gives GeoNames places ("New York", not GeoNames'
/// "New York City").
static CITY_NAMES: LazyLock<HashMap<GeoId, &'static str>> = LazyLock::new(|| {
    CITIES
        .iter()
        .filter_map(|c| CITY_IDS.get(c.display).map(|id| (*id, c.display)))
        .collect()
});

/// What a place names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    /// Anywhere in the world: a scope, not a place.
    Worldwide,
    Region(Region),
    Country(&'static Country),
    /// A state, province or other first-level region.
    Subdivision {
        country: &'static Country,
        name: &'static str,
        /// Its GeoNames id (0 when GeoNames doesn't list it).
        id: GeoId,
    },
    City {
        name: &'static str,
        country: &'static Country,
        /// Its GeoNames id.
        id: GeoId,
    },
}

impl Area {
    pub fn country(&self) -> Option<&'static Country> {
        match self {
            Self::Country(c)
            | Self::Subdivision { country: c, .. }
            | Self::City { country: c, .. } => Some(c),
            Self::Worldwide | Self::Region(_) => None,
        }
    }

    /// Whether this area includes `country`.
    pub fn contains(&self, country: &Country) -> Membership {
        match self {
            Self::Worldwide => Membership::Yes,
            Self::Region(r) => r.contains(country),
            Self::Country(c) => {
                if c.code == country.code {
                    Membership::Yes
                } else {
                    Membership::No
                }
            }
            // A city or state is inside the country, not all of it.
            Self::Subdivision { country: c, .. } | Self::City { country: c, .. } => {
                if c.code == country.code {
                    Membership::Maybe
                } else {
                    Membership::No
                }
            }
        }
    }

    /// Whether this area lies within `outer` (a city within its state or
    /// country, a state within its country, a country within a region
    /// that certainly includes it).
    pub fn within(&self, outer: &Area) -> bool {
        if self == outer || *outer == Self::Worldwide {
            return true;
        }
        match (*self, *outer) {
            (Self::City { id, .. }, Self::Subdivision { id: region, .. }) => {
                GAZETTEER.city(id).and_then(|c| c.admin1) == Some(region) && region != 0
            }
            (
                Self::City { country: c, .. } | Self::Subdivision { country: c, .. },
                Self::Country(outer),
            ) => c.code == outer.code,
            (area, Self::Region(r)) => area
                .country()
                .is_some_and(|c| r.contains(c) == Membership::Yes),
            _ => false,
        }
    }

    /// The time zones kept here: a city's own, a region's or country's
    /// every zone. `None` for [`Area::Worldwide`].
    pub fn zone(&self) -> Option<Zone> {
        let g = &*GAZETTEER;
        match self {
            Self::Worldwide => None,
            Self::Region(r) => r.zone(),
            Self::Country(c) => country_zone(c),
            Self::Subdivision { country, id, .. } => g
                .admin1(*id)
                .and_then(|a| Zone::new(a.zones.iter().map(|z| Clock::Iana(*z))))
                .or_else(|| country_zone(country)),
            Self::City { country, id, .. } => g
                .city(*id)
                .map(|c| Zone::of(Clock::Iana(c.zone)))
                .or_else(|| country_zone(country)),
        }
    }

    /// Stable identifier: `worldwide`, `region:latam`, `country:BR`,
    /// `city:BR:São Paulo`, `subdivision:US:California`.
    pub fn code(&self) -> String {
        match self {
            Self::Worldwide => "worldwide".into(),
            Self::Region(r) => format!("region:{}", r.code()),
            Self::Country(c) => format!("country:{}", c.code),
            Self::Subdivision { country, name, .. } => {
                format!("subdivision:{}:{name}", country.code)
            }
            Self::City { name, country, .. } => format!("city:{}:{name}", country.code),
        }
    }

    /// How many people live here, as far as the GeoNames subset says: the
    /// weight that tells places of one name apart.
    fn population(&self) -> u64 {
        let g = &*GAZETTEER;
        match self {
            Self::Worldwide | Self::Region(_) => u64::MAX,
            Self::Country(c) => g.country(c.code).map_or(0, |c| c.population),
            Self::Subdivision { id, .. } => g.admin1(*id).map_or(0, |a| a.population),
            Self::City { id, .. } => g.city(*id).map_or(0, |c| c.population),
        }
    }
}

impl fmt::Display for Area {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Worldwide => f.write_str("anywhere"),
            Self::Region(r) => f.write_str(r.name()),
            Self::Country(c) => f.write_str(c.name),
            Self::Subdivision { country, name, .. } => write!(f, "{name}, {}", country.name),
            Self::City { name, country, .. } => write!(f, "{name}, {}", country.name),
        }
    }
}

fn admin_area(a: &'static gazetteer::Admin1) -> Option<Area> {
    Some(Area::Subdivision {
        country: country(a.country)?,
        name: SUBDIVISION_NAMES.get(&a.id).copied().unwrap_or(a.name),
        id: a.id,
    })
}

fn city_area(c: &'static gazetteer::City) -> Option<Area> {
    Some(Area::City {
        name: CITY_NAMES.get(&c.id).copied().unwrap_or(c.name),
        country: country(c.country)?,
        id: c.id,
    })
}

fn entry_area(entry: Entry) -> Option<Area> {
    let g = &*GAZETTEER;
    match entry {
        Entry::Country(i) => g
            .countries
            .get(i)
            .and_then(|c| country(c.code))
            .map(Area::Country),
        Entry::Admin1(i) => g.admin1.get(i).and_then(admin_area),
        Entry::City(i) => g.cities.get(i).and_then(city_area),
    }
}

/// Narrow's own names (regions, countries' names and aliases, listed
/// cities with their nicknames, subdivisions), normalized with
/// [`search_key`], in order of precedence: "Singapore" is the country,
/// "Washington" the state.
static CURATED_NAMES: LazyLock<HashMap<String, Area>> = LazyLock::new(|| {
    let mut index: HashMap<String, Area> = HashMap::new();
    let mut add = |name: &str, area: Area| {
        let key = search_key(name);
        if !key.is_empty() {
            index.entry(key).or_insert(area);
        }
    };
    for name in [
        "worldwide",
        "anywhere",
        "global",
        "globally",
        "anywhere in the world",
        "international",
    ] {
        add(name, Area::Worldwide);
    }
    for region in Region::ALL {
        for name in region.names() {
            if search_key(name).len() > 2 {
                add(name, Area::Region(region));
            }
        }
    }
    for c in COUNTRIES.iter().filter(|c| c.curated) {
        add(c.name, Area::Country(c));
        for alias in c.aliases {
            add(alias, Area::Country(c));
        }
    }
    let g = &*GAZETTEER;
    for city in CITIES {
        if let Some(area) = CITY_IDS
            .get(city.display)
            .and_then(|id| g.city(*id))
            .and_then(city_area)
        {
            for name in city.names {
                add(name, area);
            }
        }
    }
    for sub in SUBDIVISION_LISTS.iter().flat_map(|l| l.iter()) {
        let Some(area) = subdivision_area(sub) else {
            continue;
        };
        add(sub.name, area);
        let key = search_key(sub.name);
        if let Some(short) = key.strip_suffix(" state")
            && sub.country != "BR"
        {
            add(short, area);
        }
    }
    index
});

/// The names sentences are read against: Narrow's own, less those that
/// mean several places ("Cambridge", "Georgia"), so a sentence never picks
/// one of them.
static NAMES: LazyLock<HashMap<String, Area>> = LazyLock::new(|| {
    CURATED_NAMES
        .iter()
        .filter(|(key, area)| match area {
            Area::City { .. } | Area::Subdivision { .. } => {
                choose(&candidates_of(key)) == Some(Resolution::Place(**area))
            }
            _ => true,
        })
        .map(|(k, a)| (k.clone(), *a))
        .collect()
});

/// The first word of every name, so text scanning can skip words that
/// start none.
static FIRST_WORDS: LazyLock<HashSet<String>> = LazyLock::new(|| {
    NAMES
        .keys()
        .filter_map(|k| k.split(' ').next())
        .map(str::to_owned)
        .collect()
});

/// Words that describe a place without naming it ("Japan Locations").
const PLACE_NOISE: [&str; 9] = [
    "locations",
    "location",
    "office",
    "offices",
    "hub",
    "hubs",
    "hq",
    "headquarters",
    "campus",
];

/// The text without words that describe a place without naming it; `None`
/// when there are none (or nothing else).
fn without_noise(text: &str) -> Option<String> {
    let key = search_key(text);
    let trimmed: Vec<&str> = key
        .split(' ')
        .filter(|w| !PLACE_NOISE.contains(w))
        .collect();
    (trimmed.len() < key.split(' ').count() && !trimmed.is_empty()).then(|| trimmed.join(" "))
}

/// Looks up a single name among Narrow's own (a region, country, listed
/// city or subdivision), as sentences are read: never a name that means
/// several places. Two- and three-letter codes are handled by
/// [`lookup_code`]; location fields are read with [`resolve_name`].
pub fn lookup_name(text: &str) -> Option<Area> {
    let key = search_key(text);
    if key.is_empty() {
        return None;
    }
    NAMES
        .get(&key)
        .copied()
        .or_else(|| without_noise(text).and_then(|k| NAMES.get(&k).copied()))
}

/// What a name means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// One place.
    Place(Area),
    /// Several places the name plausibly means, most populous first; the
    /// name alone doesn't choose.
    Ambiguous(Vec<Area>),
}

/// A place is plausible for a name when it has at least this share of the
/// most populous candidate's population (1 in 10).
const PLAUSIBLE_SHARE: u64 = 10;

/// Every place a name can mean, with its population: Narrow's own names
/// and GeoNames' (any language kept, with or without diacritics).
fn candidates_of(text: &str) -> Vec<(Area, u64)> {
    let mut out: Vec<(Area, u64)> = Vec::new();
    let mut push = |area: Area| {
        if !out.iter().any(|(a, _)| *a == area) {
            out.push((area, area.population()));
        }
    };
    if let Some(area) = CURATED_NAMES.get(&search_key(text)) {
        push(*area);
    }
    for entry in GAZETTEER.lookup(text) {
        if let Some(area) = entry_area(*entry) {
            push(area);
        }
    }
    out
}

/// [`candidates_of`], or of the text without place-describing words
/// ("Japan Locations").
fn candidates(text: &str) -> Vec<(Area, u64)> {
    let found = candidates_of(text);
    if found.is_empty()
        && let Some(trimmed) = without_noise(text)
    {
        return candidates_of(&trimmed);
    }
    found
}

/// What candidates add up to. Scope words and regions are Narrow's own
/// and never ambiguous. Otherwise a place counts only if it is plausible
/// (see [`PLAUSIBLE_SHARE`]), and a plausible place inside another
/// plausible reading gives way to it: the country over its own cities and
/// regions ("Singapore", "Mexico"). A region also gives way to a city of
/// the same name that is in it ("New York", "São Paulo"), or that keeps
/// the same time in the same country (a capital that is a region of its
/// own: "Kyiv", "Buenos Aires"): the readings differ only in extent.
/// "Washington" (the state, or D.C., three hours apart) stays ambiguous.
/// One place left is the answer; several are ambiguous.
fn choose(candidates: &[(Area, u64)]) -> Option<Resolution> {
    if let Some((area, _)) = candidates
        .iter()
        .find(|(a, _)| matches!(a, Area::Worldwide | Area::Region(_)))
    {
        return Some(Resolution::Place(*area));
    }
    let top = candidates.iter().map(|(_, p)| *p).max()?;
    let mut plausible: Vec<(Area, u64)> = candidates
        .iter()
        .filter(|(_, p)| p.saturating_mul(PLAUSIBLE_SHARE) >= top)
        .copied()
        .collect();
    plausible.sort_by_key(|(_, p)| std::cmp::Reverse(*p));
    let gives_way = |a: &Area, b: &Area| match (a, b) {
        (Area::City { .. } | Area::Subdivision { .. }, Area::Country(_)) => a.within(b),
        (Area::Subdivision { country: c, .. }, Area::City { country: d, .. }) => {
            b.within(a) || (c.code == d.code && a.zone().is_some() && a.zone() == b.zone())
        }
        _ => false,
    };
    let kept: Vec<Area> = plausible
        .iter()
        .map(|(a, _)| *a)
        .filter(|a| !plausible.iter().any(|(b, _)| b != a && gives_way(a, b)))
        .collect();
    match kept.as_slice() {
        [] => None,
        [one] => Some(Resolution::Place(*one)),
        _ => Some(Resolution::Ambiguous(kept)),
    }
}

/// Resolves a name as written in a location field or stated by a person:
/// Narrow's names and GeoNames', with ambiguity kept.
pub fn resolve_name(text: &str) -> Option<Resolution> {
    choose(&candidates(text))
}

/// What `candidates` mean inside `outer` ("Cambridge" in Massachusetts).
fn narrowed(candidates: &[(Area, u64)], outer: &Area) -> Option<Resolution> {
    let inside: Vec<(Area, u64)> = candidates
        .iter()
        .filter(|(a, _)| a != outer && a.within(outer))
        .copied()
        .collect();
    choose(&inside)
}

/// Looks up a code as written after a comma ("CA", "NSW", "US"):
/// subdivisions first (a city's "CA" is California), then countries. After
/// a city whose country is known, codes of that country win ("Berlin, DE"
/// is Germany, not Delaware).
pub fn lookup_code_near(text: &str, after_city: bool, near: Option<&Country>) -> Option<Area> {
    if let Some(near) = near {
        let code = text.trim().trim_end_matches('.').replace('.', "");
        if code == near.code {
            return Some(Area::Country(country(near.code)?));
        }
        let own = SUBDIVISION_LISTS
            .iter()
            .flat_map(|list| list.iter())
            .find(|s| s.code == code && s.country == near.code);
        if let Some(s) = own {
            return subdivision_area(s);
        }
    }
    lookup_code(text, after_city)
}

/// Read a code from location or restriction text. A bare "TN" there names
/// Tennessee; an ISO code in a structured country field is read with
/// [`country`] instead. A preceding place still supplies its own context.
fn lookup_location_code_near(text: &str, after_city: bool, near: Option<&Country>) -> Option<Area> {
    if near.is_none() && !after_city && text.trim().trim_end_matches('.') == "TN" {
        return US_STATES
            .iter()
            .find(|s| s.code == "TN")
            .and_then(subdivision_area);
    }
    lookup_code_near(text, after_city, near)
}

/// [`lookup_code_near`] without a nearby country. Alone, Narrow's own
/// countries come first ("CA" is Canada), then subdivisions, and only then
/// the other ISO codes: "GA" is Georgia, not Gabon; "NC" North Carolina,
/// not New Caledonia.
pub fn lookup_code(text: &str, after_city: bool) -> Option<Area> {
    let code = text.trim().trim_end_matches('.').replace('.', "");
    if !(2..=3).contains(&code.len()) || !code.chars().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let subdivision = || {
        SUBDIVISION_LISTS
            .iter()
            .flat_map(|list| list.iter())
            .find(|s| s.code == code)
            .and_then(subdivision_area)
    };
    let by_country = |any: bool| match code.as_str() {
        "UK" => country("GB").map(Area::Country),
        "USA" => country("US").map(Area::Country),
        _ => country(&code)
            .filter(|c| any || c.curated)
            .map(Area::Country),
    };
    match code.as_str() {
        "EU" => return Some(Area::Region(Region::EuropeanUnion)),
        "EEA" => return Some(Area::Region(Region::Eea)),
        _ => {}
    }
    if after_city {
        subdivision().or_else(|| by_country(true))
    } else {
        by_country(false)
            .or_else(subdivision)
            .or_else(|| by_country(true))
    }
}

/// The subdivision a code names that contains one of the places a name
/// before it could be: "MS" after "Campo Grande" is Mato Grosso do Sul,
/// after "Jackson" Mississippi. `None` unless exactly one does.
fn subdivision_containing(text: &str, candidates: &[(Area, u64)]) -> Option<Area> {
    let code = text.trim().trim_end_matches('.').replace('.', "");
    if !(2..=3).contains(&code.len()) || !code.chars().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let mut containing = SUBDIVISION_LISTS
        .iter()
        .flat_map(|list| list.iter())
        .filter(|s| s.code == code)
        .filter_map(subdivision_area)
        .filter(|sub| candidates.iter().any(|(a, _)| a != sub && a.within(sub)));
    match (containing.next(), containing.next()) {
        (Some(one), None) => Some(one),
        _ => None,
    }
}

/// What a name means inside `outer`, from its readings there only:
/// "Alexandria" in the United States (Virginia's or Louisiana's), not
/// Egypt's. `None` when it has no reading there.
pub fn resolve_within(text: &str, outer: &Area) -> Option<Resolution> {
    narrowed(&candidates(text), outer)
}

/// A place named in a location string or a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The text it came from, as written.
    pub raw: String,
    /// What Narrow recognized; `None` when it did not, or when the text
    /// means several places and nothing around it chooses.
    pub area: Option<Area>,
    /// Marked as remote ("Remote (Canada)", "US Remote").
    pub remote: bool,
    /// The places the text plausibly means when it doesn't say which
    /// ("Cambridge": in England, Ontario or Massachusetts), most populous
    /// first; empty otherwise.
    pub ambiguous: Vec<Area>,
}

impl Place {
    fn new(raw: impl Into<String>, area: Option<Area>, remote: bool) -> Self {
        Self {
            raw: raw.into(),
            area,
            remote,
            ambiguous: Vec::new(),
        }
    }

    /// A place from what a name resolved to.
    fn resolved(raw: impl Into<String>, resolution: Option<Resolution>, remote: bool) -> Self {
        match resolution {
            Some(Resolution::Place(area)) => Self::new(raw, Some(area), remote),
            Some(Resolution::Ambiguous(areas)) => Self {
                ambiguous: areas,
                ..Self::new(raw, None, remote)
            },
            None => Self::new(raw, None, remote),
        }
    }

    /// The narrowest area every reading of an ambiguous place shares (the
    /// region or country of "Portland": Oregon's or Maine's); `None` when
    /// they share none, or the place isn't ambiguous.
    pub fn common_area(&self) -> Option<Area> {
        let first = self.ambiguous.first()?;
        let mut outer: Vec<Area> = Vec::new();
        if let Area::City { id, .. } = first
            && let Some(region) = GAZETTEER.city(*id).and_then(|c| c.admin1)
            && let Some(area) = GAZETTEER.admin1(region).and_then(admin_area)
        {
            outer.push(area);
        }
        if let Some(c) = first.country() {
            outer.push(Area::Country(c));
        }
        outer
            .into_iter()
            .find(|o| self.ambiguous.iter().all(|a| a == o || a.within(o)))
    }
}

const REMOTE_WORDS: [&str; 10] = [
    "remote",
    "remotely",
    "remote friendly",
    "fully remote",
    "distributed",
    "work from home",
    "wfh",
    "virtual",
    "home based",
    "telecommute",
];

/// How many location strings [`parse_places`] remembers; past that it
/// starts over.
const REMEMBERED_PLACES: usize = 20_000;

/// Parses a location string as sources write them: "San Francisco, CA",
/// "Remote (Canada)", "US Remote", "New York City, NY; San Francisco, CA |
/// Seattle, WA", "Chicago, Seattle, NYC, San Francisco, Remote". Location
/// strings repeat across postings ("Remote", "New York, NY"), so each is
/// read once and remembered.
pub fn parse_places(text: &str) -> Vec<Place> {
    static REMEMBERED: LazyLock<Mutex<HashMap<String, Arc<Vec<Place>>>>> =
        LazyLock::new(Mutex::default);
    if let Some(found) = REMEMBERED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(text)
    {
        return found.as_ref().clone();
    }
    let mut out = Vec::new();
    for option in split_options(text) {
        out.extend(parse_option(&option));
    }
    let mut remembered = REMEMBERED.lock().unwrap_or_else(PoisonError::into_inner);
    if remembered.len() >= REMEMBERED_PLACES {
        remembered.clear();
    }
    remembered.insert(text.to_owned(), Arc::new(out.clone()));
    out
}

fn split_options(text: &str) -> Vec<String> {
    // Separators only count outside parentheses: "Remote (SF, CA; Oakland, CA)".
    const SEPARATORS: [&str; 6] = ["|", ";", " / ", "•", "\n", " or "];
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let Some(c) = rest.chars().next() else { break };
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0
            && let Some(sep) = SEPARATORS.iter().find(|sep| rest.starts_with(**sep))
        {
            parts.push(std::mem::take(&mut current));
            i += sep.len();
            continue;
        }
        current.push(c);
        i += c.len_utf8();
    }
    parts.push(current);
    parts
        .into_iter()
        .map(|p| p.trim().to_owned())
        .filter(|p| !p.is_empty())
        .collect()
}

fn strip_remote(text: &str) -> (String, bool) {
    let key = search_key(text);
    let mut remote = false;
    let mut rest = format!(" {key} ");
    // Longest first, so "remote friendly" wins over "remote".
    let mut words = REMOTE_WORDS.to_vec();
    words.sort_by_key(|w| std::cmp::Reverse(w.len()));
    for word in words {
        let pattern = format!(" {word} ");
        if rest.contains(&pattern) {
            remote = true;
            rest = rest.replace(&pattern, " ");
        }
    }
    (rest.trim().to_owned(), remote)
}

/// "Remote-Friendly US" → "Remote US", so the hyphen does not split it.
fn normalize_remote(text: &str) -> String {
    let mut out = text.to_owned();
    for compound in [
        "remote-friendly",
        "remote-first",
        "remote-ok",
        "remote friendly",
        "remote first",
    ] {
        while let Some(at) = out.to_lowercase().find(compound) {
            out.replace_range(at..at + compound.len(), "Remote");
        }
    }
    // "Remote in United States", "Remote within Canada" (how Stripe's jobs
    // site and others list a remote scope) → "Remote - United States", so
    // the place is read as the scope rather than "in United States".
    for connective in [
        "remote in the ",
        "remote within the ",
        "remote from the ",
        "remote in ",
        "remote within ",
        "remote from ",
    ] {
        while let Some(at) = out.to_lowercase().find(connective) {
            out.replace_range(at..at + connective.len(), "Remote - ");
        }
    }
    out
}

fn parse_option(option: &str) -> Vec<Place> {
    let original = option.trim();
    let normalized = normalize_remote(option);
    let option = normalized.as_str();
    // Parenthesized parts: "Remote (Canada)", "New York, NY (HQ)".
    let mut main = String::new();
    let mut inner: Vec<String> = Vec::new();
    let mut depth = 0;
    let mut current = String::new();
    for c in option.chars() {
        match c {
            '(' | '[' => {
                depth += 1;
                if depth == 1 {
                    continue;
                }
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    inner.push(std::mem::take(&mut current));
                    continue;
                }
            }
            _ => {}
        }
        if depth > 0 {
            current.push(c);
        } else {
            main.push(c);
        }
    }
    let (main_rest, main_remote) = strip_remote(&main);
    let inner_remote = inner.iter().any(|i| strip_remote(i).1);
    // "Remote - LATAM" and "US Remote" are remote work in that area;
    // "Chicago, Seattle, Remote" lists remote as an option of its own.
    let parts: Vec<&str> = main
        .split([',', '-', '–', '—'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let areas = parts
        .iter()
        .filter(|p| !strip_remote(p).0.is_empty())
        .count();
    let remote = inner_remote || (main_remote && areas <= 1);

    let mut places = if main_rest.is_empty() {
        Vec::new()
    } else {
        parse_list(&main, remote)
    };
    // Geography inside parentheses qualifies the option ("Remote (US)").
    for part in &inner {
        let found: Vec<Place> = parse_places(part)
            .into_iter()
            .map(|mut p| {
                p.remote |= remote;
                p
            })
            .collect();
        let recognized: Vec<Place> = found.into_iter().filter(|p| p.area.is_some()).collect();
        if recognized.is_empty() {
            continue;
        }
        if places.iter().all(|p| p.area.is_none()) {
            places = recognized
                .into_iter()
                .map(|mut p| {
                    p.raw = original.to_owned();
                    p
                })
                .collect();
        }
    }
    if places.is_empty() {
        places.push(Place::new(original, None, remote || main_remote));
    }
    places
}

/// Pending names and what each could mean.
type Pending = Vec<(String, Vec<(Area, u64)>)>;

/// Adds the pending names as places of their own: unrecognized, or
/// ambiguous with what they could mean.
fn flush(
    pending: &mut Pending,
    out: &mut Vec<Place>,
    meant: &mut Vec<Vec<(Area, u64)>>,
    remote: bool,
) {
    for (name, found) in pending.drain(..) {
        out.push(Place::resolved(name, choose(&found), remote));
        meant.push(found);
    }
}

/// "San Francisco, CA, US" → one place; "Chicago, Seattle, NYC" → three.
fn parse_list(text: &str, remote: bool) -> Vec<Place> {
    // A whole-string match first ("Washington, D.C.", "Rio de Janeiro").
    // A code ("WA", "PA", "VIC") is read as a code below, never as a
    // GeoNames place of that name (Wa, Ghana); only Narrow's own names
    // ("LA", "NYC") match it whole.
    let whole = text.trim().trim_matches(|c: char| c == ',' || c == '-');
    let code_like = (2..=3).contains(&whole.len()) && whole.chars().all(|c| c.is_ascii_uppercase());
    let resolution = if code_like {
        lookup_name(whole).map(Resolution::Place)
    } else {
        resolve_name(whole)
    };
    if let Some(resolution) = resolution {
        return vec![Place::resolved(whole, Some(resolution), remote)];
    }
    // Commas separate places; dashes too, unless the name has one
    // ("Winston-Salem").
    let mut parts: Vec<&str> = Vec::new();
    for segment in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        if segment.contains(['-', '–', '—']) && resolve_name(segment).is_none() {
            parts.extend(
                segment
                    .split(['-', '–', '—'])
                    .map(str::trim)
                    .filter(|p| !p.is_empty()),
            );
        } else {
            parts.push(segment);
        }
    }
    let mut out: Vec<Place> = Vec::new();
    // What each place's name could have meant, for a qualifier after it.
    let mut meant: Vec<Vec<(Area, u64)>> = Vec::new();
    // Names not settled yet: unrecognized, or meaning several places.
    let mut pending: Pending = Vec::new();
    for part in parts {
        let (stripped, part_remote) = strip_remote(part);
        if stripped.is_empty() {
            if part_remote && !remote {
                // "Remote" as an option of its own: no known area.
                flush(&mut pending, &mut out, &mut meant, remote);
                out.push(Place::new(part, None, true));
                meant.push(Vec::new());
            }
            continue;
        }
        let near = out.last().and_then(|p| p.area).and_then(|a| a.country());
        let after_city = out
            .last()
            .is_some_and(|p| matches!(p.area, Some(Area::City { .. }) | None) && !p.remote)
            || !pending.is_empty();
        // In a location field a short leftover is a code ("US Remote").
        let code = (stripped.len() <= 3).then(|| stripped.to_uppercase());
        // A code after a name it can settle means the subdivision holding
        // one of that name's places ("Campo Grande, MS", "Jackson, MS").
        let containing = pending.last().and_then(|(_, before)| {
            subdivision_containing(part, before).or_else(|| {
                code.as_deref()
                    .and_then(|c| subdivision_containing(c, before))
            })
        });
        let coded = containing
            .or_else(|| lookup_location_code_near(part.trim(), after_city, near))
            .or_else(|| {
                code.as_deref()
                    .and_then(|c| lookup_location_code_near(c, after_city, near))
            });
        let (found, resolution) = match coded {
            Some(area) => (vec![(area, 0)], Some(Resolution::Place(area))),
            None => {
                let mut found = candidates(&stripped);
                if found.is_empty() {
                    found = candidates(part);
                }
                let resolution = choose(&found);
                (found, resolution)
            }
        };
        // A country or state after a place qualifies it ("Seattle, WA");
        // an ambiguous one does when exactly one of its readings contains
        // the place before it ("Seattle, Washington", "Atlanta, Georgia").
        let qualifier = match &resolution {
            Some(Resolution::Place(a @ (Area::Country(_) | Area::Subdivision { .. }))) => Some(*a),
            Some(Resolution::Ambiguous(options)) if pending.is_empty() => {
                let before = out.last().and_then(|p| p.area);
                let containing: Vec<Area> = options
                    .iter()
                    .filter(|o| matches!(o, Area::Country(_) | Area::Subdivision { .. }))
                    .filter(|o| before.is_some_and(|b| b.within(o)))
                    .copied()
                    .collect();
                match containing.as_slice() {
                    [one] => Some(*one),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(q) = qualifier {
            // "Cambridge, MA", "Hayward, CA": what the name before means
            // there, else the qualifier itself, with the name kept.
            if let Some((name, found_before)) = pending.pop() {
                flush(&mut pending, &mut out, &mut meant, remote);
                let area = match narrowed(&found_before, &q) {
                    Some(Resolution::Place(a)) => a,
                    _ => q,
                };
                out.push(Place::new(format!("{name}, {part}"), Some(area), remote));
                meant.push(found_before);
                continue;
            }
            if qualifies(out.last(), q) {
                if let Some(last) = out.last_mut() {
                    last.raw = format!("{}, {part}", last.raw);
                    if last.area.is_none() {
                        last.area = Some(q);
                    }
                }
                continue;
            }
            // "London, Ontario", "Paris, TX": another place of the name
            // before.
            if let (Some(last), Some(found_before)) = (out.last_mut(), meant.last())
                && let Some(Resolution::Place(a)) = narrowed(found_before, &q)
            {
                last.raw = format!("{}, {part}", last.raw);
                last.area = Some(a);
                last.ambiguous.clear();
                continue;
            }
        }
        match resolution {
            Some(Resolution::Place(area)) => {
                flush(&mut pending, &mut out, &mut meant, remote);
                out.push(Place::new(part, Some(area), remote || part_remote));
                meant.push(found);
            }
            Some(Resolution::Ambiguous(_)) | None => pending.push((part.to_owned(), found)),
        }
    }
    flush(&mut pending, &mut out, &mut meant, remote);
    out
}

/// Whether a country or state after a comma qualifies the previous place
/// (the same country) rather than starting a new one.
fn qualifies(previous: Option<&Place>, qualifier: Area) -> bool {
    let Some(country) = qualifier.country() else {
        return false;
    };
    match previous.and_then(|p| p.area) {
        Some(Area::City { country: c, .. })
        | Some(Area::Subdivision { country: c, .. })
        | Some(Area::Country(c)) => c.code == country.code,
        _ => false,
    }
}

/// Every region, country and known city named in free text (a sentence of a
/// description), in order. Two-letter codes count only when written in
/// capitals ("US", "UK", "EU").
pub fn places_in_text(text: &str) -> Vec<Area> {
    let words: Vec<&str> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '.' || c == '-'))
        .map(|w| w.trim_matches(['.', '-']))
        // "US-based", "Brazil-based": the place, not a word of its own.
        .map(|w| {
            w.strip_suffix("-based")
                .or_else(|| w.strip_suffix("-Based"))
                .unwrap_or(w)
        })
        .filter(|w| !w.is_empty())
        .collect();
    let mut out: Vec<Area> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        let code = word.len() <= 3 && word.chars().all(|c| c.is_ascii_uppercase());
        let key = search_key(word);
        if !code && !FIRST_WORDS.contains(key.split(' ').next().unwrap_or_default()) {
            i += 1;
            continue;
        }
        let mut found = None;
        // Longest phrases first (up to four words).
        for len in (1..=4).rev() {
            if i + len > words.len() {
                continue;
            }
            let phrase = words[i..i + len].join(" ");
            let area = if len == 1 && phrase.len() <= 3 {
                // Codes must be capitals, and not ordinary words.
                if phrase.chars().all(|c| c.is_ascii_uppercase())
                    && !matches!(
                        phrase.as_str(),
                        "IT" | "IN"
                            | "OR"
                            | "ME"
                            | "OK"
                            | "HI"
                            | "AI"
                            | "ML"
                            | "QA"
                            | "UI"
                            | "UX"
                            | "PR"
                            | "CI"
                            | "CD"
                            | "HR"
                            | "BA"
                            | "PE"
                            | "ES"
                            | "DE"
                            | "NO"
                            | "NA"
                            | "ID"
                            | "CO"
                            | "SO"
                            | "AS"
                            | "AT"
                            | "BE"
                            | "IS"
                            | "AM"
                            | "PM"
                            | "TO"
                    )
                {
                    // Only Narrow's own countries and subdivisions ("AM"
                    // and "PM" are times, not Amazonas or Saint Pierre).
                    lookup_location_code_near(&phrase, false, None)
                        .filter(|a| !matches!(a, Area::Country(c) if !c.curated))
                } else {
                    None
                }
            } else {
                lookup_name(&phrase).filter(|a| {
                    // Demonyms and ordinary words are too loose alone
                    // ("American Express", "Asian cuisine", "a 10-acre
                    // campus").
                    !(len == 1
                        && matches!(
                            a,
                            Area::Region(_) | Area::Country(_) | Area::Subdivision { .. }
                        )
                        && phrase.chars().next().is_some_and(char::is_lowercase))
                })
            };
            if let Some(area) = area {
                found = Some((area, len));
                break;
            }
        }
        match found {
            // "anywhere" in prose is rarely about hiring; callers that care
            // look for explicit phrases.
            Some((Area::Worldwide, len)) => i += len,
            Some((area, len)) => {
                if !out.contains(&area) {
                    out.push(area);
                }
                i += len;
            }
            None => i += 1,
        }
    }
    out
}

/// Formats minutes east of UTC as "UTC-3" / "UTC+5:30".
pub fn format_offset(minutes: i16) -> String {
    let sign = if minutes < 0 { '-' } else { '+' };
    let abs = minutes.unsigned_abs();
    match abs % 60 {
        0 => format!("UTC{sign}{}", abs / 60),
        m => format!("UTC{sign}{}:{m:02}", abs / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn areas(text: &str) -> Vec<(String, bool)> {
        parse_places(text)
            .into_iter()
            .map(|p| {
                (
                    p.area
                        .map_or_else(|| format!("? {}", p.raw), |a| a.to_string()),
                    p.remote,
                )
            })
            .collect()
    }

    fn s(v: &[(&str, bool)]) -> Vec<(String, bool)> {
        v.iter().map(|(a, r)| ((*a).to_owned(), *r)).collect()
    }

    #[test]
    fn parses_real_location_strings() {
        assert_eq!(
            areas("San Francisco, CA"),
            s(&[("San Francisco, United States", false)])
        );
        assert_eq!(
            areas("New York, NY (HQ)"),
            s(&[("New York, United States", false)])
        );
        assert_eq!(areas("Remote (Canada)"), s(&[("Canada", true)]));
        assert_eq!(areas("Remote (US)"), s(&[("United States", true)]));
        assert_eq!(areas("Europe"), s(&[("Europe", false)]));
        assert_eq!(
            areas("London, United Kingdom"),
            s(&[("London, United Kingdom", false)])
        );
        assert_eq!(areas("Bengaluru, India"), s(&[("Bengaluru, India", false)]));
        assert_eq!(areas("Singapore, Singapore"), s(&[("Singapore", false)]));
        assert_eq!(
            areas("Washington, D.C."),
            s(&[("Washington, D.C., United States", false)])
        );
        assert_eq!(
            areas("Chicago, Seattle, NYC, San Francisco, Remote "),
            s(&[
                ("Chicago, United States", false),
                ("Seattle, United States", false),
                ("New York, United States", false),
                ("San Francisco, United States", false),
                ("? Remote", true),
            ])
        );
        assert_eq!(
            areas("Remote-Friendly (Travel-Required) | San Francisco, CA | Seattle, WA"),
            s(&[
                ("? Remote-Friendly (Travel-Required)", true),
                ("San Francisco, United States", false),
                ("Seattle, United States", false),
            ])
        );
        assert_eq!(
            areas("Berkeley, CA, US / Remote (San Francisco, CA, US; Oakland, CA, US)"),
            s(&[
                ("Berkeley, United States", false),
                ("San Francisco, United States", true),
                ("Oakland, United States", true),
            ])
        );
        assert_eq!(
            areas("Hybrid (UK) / Remote (US)"),
            s(&[("United Kingdom", false), ("United States", true)])
        );
        assert_eq!(
            areas("Porto Alegre, RS"),
            s(&[("Porto Alegre, Brazil", false)])
        );
        // Any place GeoNames lists, not only Narrow's own cities.
        assert_eq!(
            areas("Hayward, CA, US"),
            s(&[("Hayward, United States", false)])
        );
        // Unknown places stay unknown, with their qualifier.
        assert_eq!(
            areas("Springfieldtown, CA"),
            s(&[("California, United States", false)])
        );
        assert_eq!(areas("Remote - LATAM"), s(&[("Latin America", true)]));
        assert_eq!(areas("US Remote"), s(&[("United States", true)]));
        assert_eq!(areas("Anywhere"), s(&[("anywhere", false)]));
        assert_eq!(areas("Narnia"), s(&[("? Narnia", false)]));
    }

    fn one(text: &str) -> Place {
        let places = parse_places(text);
        assert_eq!(places.len(), 1, "{text}: {places:?}");
        places.into_iter().next().unwrap()
    }

    fn ambiguous(text: &str) -> Vec<String> {
        let p = one(text);
        assert_eq!(p.area, None, "{text} is not settled");
        p.ambiguous.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn keeps_kinds_of_places_apart() {
        let kind = |text: &str| match one(text).area {
            Some(Area::Worldwide) => "scope".to_owned(),
            Some(Area::Region(r)) => format!("region:{}", r.code()),
            Some(Area::Country(c)) => format!("country:{}", c.code),
            Some(Area::Subdivision { country, name, .. }) => {
                format!("subdivision:{}:{name}", country.code)
            }
            Some(Area::City { country, name, .. }) => format!("city:{}:{name}", country.code),
            None => "unknown".to_owned(),
        };
        assert_eq!(kind("Brazil"), "country:BR");
        assert_eq!(kind("São Paulo, Brazil"), "city:BR:São Paulo");
        assert_eq!(kind("São Paulo"), "city:BR:São Paulo");
        assert_eq!(kind("São Paulo State"), "subdivision:BR:São Paulo State");
        assert_eq!(kind("Latin America"), "region:latam");
        assert_eq!(kind("LATAM"), "region:latam");
        assert_eq!(kind("Americas"), "region:americas");
        assert_eq!(kind("Europe"), "region:europe");
        // "Worldwide" is a scope, not a place.
        assert_eq!(kind("Worldwide"), "scope");
        assert_eq!(kind("Anywhere"), "scope");
        assert_eq!(kind("Narnia"), "unknown");
        // A first-level region anywhere GeoNames lists one.
        assert_eq!(kind("Bavaria"), "subdivision:DE:Bavaria");
        assert_eq!(kind("Ontario"), "subdivision:CA:Ontario");
        // Remote scopes.
        let remote = |text: &str| {
            parse_places(text)
                .into_iter()
                .map(|p| (p.area.map(|a| a.code()), p.remote))
                .collect::<Vec<_>>()
        };
        assert_eq!(remote("Remote (US)"), [(Some("country:US".into()), true)]);
        assert_eq!(
            remote("Remote — Americas"),
            [(Some("region:americas".into()), true)]
        );
        assert_eq!(remote("Remote, Global"), [(Some("worldwide".into()), true)]);
        assert_eq!(remote("Remote"), [(None, true)]);
        // A remote region Narrow can't read stays unknown.
        assert_eq!(remote("Remote - Region TBD"), [(None, true)]);
        // Options stay options.
        assert_eq!(
            remote("Lisbon or London"),
            [
                (Some("city:PT:Lisbon".into()), false),
                (Some("city:GB:London".into()), false)
            ]
        );
        assert_eq!(
            remote("US / Canada"),
            [
                (Some("country:US".into()), false),
                (Some("country:CA".into()), false)
            ]
        );
    }

    #[test]
    fn reads_alternate_and_native_names() {
        let name = |text: &str| one(text).area.map(|a| a.to_string());
        assert_eq!(name("Sao Paulo"), Some("São Paulo, Brazil".into()));
        assert_eq!(name("SÃO PAULO"), Some("São Paulo, Brazil".into()));
        assert_eq!(name("Lisboa"), Some("Lisbon, Portugal".into()));
        assert_eq!(name("München"), Some("Munich, Germany".into()));
        assert_eq!(name("Muenchen, Germany"), Some("Munich, Germany".into()));
        assert_eq!(name("Bangalore"), Some("Bengaluru, India".into()));
        assert_eq!(name("Brasil"), Some("Brazil".into()));
        assert_eq!(name("Deutschland"), Some("Germany".into()));
        assert_eq!(
            name("Dourados, MS, Brasil"),
            Some("Dourados, Brazil".into())
        );
        assert_eq!(
            name("Winston-Salem, NC"),
            Some("Winston-Salem, United States".into())
        );
    }

    #[test]
    fn ambiguous_names_stay_unresolved() {
        assert_eq!(
            ambiguous("Cambridge"),
            [
                "Cambridge, United Kingdom",
                "Cambridge, Canada",
                "Cambridge, United States",
                "Cambridge, New Zealand"
            ]
        );
        // A country, or a US state.
        assert_eq!(ambiguous("Georgia"), ["Georgia", "Georgia, United States"]);
        // The state, or D.C.: three hours apart.
        assert_eq!(
            ambiguous("Washington"),
            [
                "Washington State, United States",
                "Washington, D.C., United States"
            ]
        );
        // A capital that is a region of its own, next to a region of the
        // same name: the same country and clock, so the city.
        assert_eq!(
            one("Kyiv").area.map(|a| a.to_string()),
            Some("Kyiv, Ukraine".into())
        );
        assert!(matches!(one("Buenos Aires").area, Some(Area::City { .. })));
        let santiago = ambiguous("Santiago");
        assert_eq!(santiago[0], "Santiago, Chile");
        assert!(santiago.contains(&"Santiago de los Caballeros, Dominican Republic".to_owned()));
        let san_jose = ambiguous("San José");
        assert!(san_jose.contains(&"San Jose, United States".to_owned()));
        assert!(san_jose.contains(&"San José, Costa Rica".to_owned()));
        // "London" is London, England: every other London is less than a
        // tenth of its size.
        assert_eq!(
            one("London").area.map(|a| a.to_string()),
            Some("London, United Kingdom".into())
        );
        // What they share is kept: both Portlands are in the US.
        let portland = one("Portland");
        assert_eq!(portland.area, None);
        assert_eq!(
            portland.common_area().map(|a| a.code()),
            Some("country:US".into())
        );
        assert_eq!(one("Cambridge").common_area(), None);
        // Sentences never pick one.
        assert_eq!(lookup_name("Cambridge"), None);
        assert_eq!(lookup_name("Georgia"), None);
        assert!(places_in_text("hybrid in Cambridge three days a week").is_empty());
    }

    #[test]
    fn context_chooses_among_places() {
        let name = |text: &str| {
            parse_places(text)
                .into_iter()
                .map(|p| {
                    p.area
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| format!("? {}", p.raw))
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(name("Cambridge, MA"), ["Cambridge, United States"]);
        assert_eq!(name("Cambridge, UK"), ["Cambridge, United Kingdom"]);
        assert_eq!(name("Cambridge, Ontario"), ["Cambridge, Canada"]);
        assert_eq!(name("Santiago, Chile"), ["Santiago, Chile"]);
        assert_eq!(name("San José, Costa Rica"), ["San José, Costa Rica"]);
        assert_eq!(name("San Jose, CA"), ["San Jose, United States"]);
        assert_eq!(name("Georgia, US"), ["Georgia, United States"]);
        assert_eq!(name("Tbilisi, Georgia"), ["Tbilisi, Georgia"]);
        assert_eq!(name("Atlanta, Georgia"), ["Atlanta, United States"]);
        assert_eq!(name("Seattle, Washington"), ["Seattle, United States"]);
        assert_eq!(name("Portland, ME"), ["Portland, United States"]);
        // The dominant reading gives way to the one the qualifier names.
        assert_eq!(name("London, Ontario"), ["London, Canada"]);
        assert_eq!(name("London, ON, Canada"), ["London, Canada"]);
        assert_eq!(name("Paris, TX"), ["Paris, United States"]);
        // A qualifier that fits no reading is kept as the place.
        assert_eq!(name("Cambridge, Brazil"), ["Brazil"]);
    }

    #[test]
    fn regions_answer_membership_honestly() {
        let br = country("BR").unwrap();
        let mx = country("MX").unwrap();
        let us = country("US").unwrap();
        assert_eq!(Region::Americas.contains(br), Membership::Yes);
        assert_eq!(Region::LatinAmerica.contains(br), Membership::Yes);
        assert_eq!(Region::NorthAmerica.contains(br), Membership::No);
        assert_eq!(Region::NorthAmerica.contains(mx), Membership::Maybe);
        assert_eq!(Region::NorthAmerica.contains(us), Membership::Yes);
        assert_eq!(Region::Europe.contains(br), Membership::No);
        assert_eq!(
            Region::EuropeanUnion.contains(country("GB").unwrap()),
            Membership::Maybe
        );
        assert_eq!(Area::Worldwide.contains(br), Membership::Yes);
        // Countries outside Narrow's lists, by continent.
        let andorra = country("AD").unwrap();
        assert!(!andorra.curated);
        assert_eq!(Region::Europe.contains(andorra), Membership::Yes);
        assert_eq!(Region::EuropeanUnion.contains(andorra), Membership::Maybe);
        assert_eq!(Region::Eea.contains(andorra), Membership::No);
        assert_eq!(Region::Americas.contains(andorra), Membership::No);
        let kazakhstan = country("KZ").unwrap();
        assert_eq!(Region::Asia.contains(kazakhstan), Membership::Yes);
        assert_eq!(Region::Emea.contains(kazakhstan), Membership::Maybe);
        assert_eq!(
            Region::Europe.contains(country("GE").unwrap()),
            Membership::Maybe
        );
        assert_eq!(format_offset(-180), "UTC-3");
        assert_eq!(format_offset(330), "UTC+5:30");
    }

    #[test]
    fn finds_places_in_sentences() {
        let names = |t: &str| {
            places_in_text(t)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names("This role is open to candidates based in North America and Europe"),
            ["North America", "Europe"]
        );
        assert_eq!(
            names("We're open to remote within the US or hybrid in Seattle"),
            ["United States", "Seattle, United States"]
        );
        assert_eq!(
            names("people (in the US - Pacific timezone)"),
            ["United States"]
        );
        assert_eq!(
            names("Experience with IT in the NO-code world"),
            Vec::<String>::new()
        );
        assert_eq!(names("We hire contractors across LATAM"), ["Latin America"]);
        assert_eq!(names("our american customers"), Vec::<String>::new());
    }

    #[test]
    fn state_codes_are_not_other_countries_or_towns() {
        // Every ISO code is a country now, and GeoNames lists towns called
        // "Wa" and "Pa": a code alone is still a state, as before.
        assert_eq!(areas("Remote - GA"), s(&[("Georgia, United States", true)]));
        assert_eq!(
            areas("Remote, NC"),
            s(&[("North Carolina, United States", true)])
        );
        assert_eq!(
            areas("Remote (VA)"),
            s(&[("Virginia, United States", true)])
        );
        assert_eq!(areas("Remote - AZ"), s(&[("Arizona, United States", true)]));
        assert_eq!(
            areas("Remote (WA)"),
            s(&[("Washington State, United States", true)])
        );
        assert_eq!(
            areas("Remote - TN"),
            s(&[("Tennessee, United States", true)])
        );
        assert_eq!(lookup_code("TN", false), country("TN").map(Area::Country));
        assert_eq!(areas("VIC"), s(&[("Victoria, Australia", false)]));
        // Narrow's own names still come first, as before ("PA" alone is
        // Panama, "LA" Los Angeles), and never a town of that name.
        assert_eq!(areas("Remote (PA)"), s(&[("Panama", true)]));
        assert_eq!(areas("LA"), s(&[("Los Angeles, United States", false)]));
        // Other ISO codes still name their countries.
        assert_eq!(areas("Tbilisi, GE"), s(&[("Tbilisi, Georgia", false)]));
        assert_eq!(areas("Remote - KZ"), s(&[("Kazakhstan", true)]));

        let names = |t: &str| {
            places_in_text(t)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        };
        let found = names("Candidates must reside in GA, NC, SC, VA or TN.");
        for state in [
            "Georgia",
            "North Carolina",
            "South Carolina",
            "Virginia",
            "Tennessee",
        ] {
            assert!(
                found.contains(&format!("{state}, United States")),
                "{state} in {found:?}"
            );
        }
        assert_eq!(found.len(), 5, "unexpected place in {found:?}");
        // Codes that are words stay words.
        assert!(names("Standup is at 9 AM, demo at 4 PM.").is_empty());
        assert!(names("WELCOME TO THE TEAM").is_empty());
    }

    #[test]
    fn a_code_after_a_name_means_the_subdivision_holding_it() {
        // The state a location resolves to, or the state of its city.
        let state = |text: &str| -> String {
            match one(text).area {
                Some(Area::City { id, .. }) => GAZETTEER
                    .city(id)
                    .and_then(|c| c.admin1)
                    .and_then(|a| GAZETTEER.admin1(a))
                    .map_or_else(|| format!("{text}: no region"), |a| a.name.to_owned()),
                Some(Area::Subdivision { name, .. }) => name.to_owned(),
                other => format!("{text}: {other:?}"),
            }
        };
        // Several places of the name, or a place in another country: the
        // code picks the state that holds one of them, not the US state
        // that shares its code.
        assert_eq!(state("Campo Grande, MS"), "Mato Grosso do Sul");
        // (GeoNames lists two São Josés, both in Santa Catarina: the state
        // is certain, the place is not guessed.)
        assert_eq!(state("São José, SC"), "Santa Catarina");
        assert_eq!(
            areas("São José, SC"),
            s(&[("Santa Catarina, Brazil", false)])
        );
        assert_eq!(state("Sinop, MT"), "Mato Grosso");
        assert_eq!(state("Santarém, PA"), "Pará");
        assert_eq!(state("Dourados, MS"), "Mato Grosso do Sul");
        // The same codes after US cities stay US states.
        assert_eq!(state("Jackson, MS"), "Mississippi");
        assert_eq!(state("Charleston, SC"), "South Carolina");
        assert_eq!(state("Pittsburgh, PA"), "Pennsylvania");
        // A town Narrow doesn't list gives no evidence: the code's usual
        // reading, as before.
        assert_eq!(
            areas("Smallville Junction, MS"),
            s(&[("Mississippi, United States", false)])
        );
    }
}
