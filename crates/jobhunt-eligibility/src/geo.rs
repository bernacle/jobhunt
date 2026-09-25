//! Just enough geography to read job locations and user constraints.
//!
//! Countries (ISO 3166 alpha-2) with their common names and standard-time
//! UTC offsets, the business regions job postings use ("Americas", "LATAM",
//! "EMEA", …), first-level subdivisions that appear in postings ("CA",
//! "Ontario", "NSW", "RS"), and the cities that host most tech jobs. It is
//! deliberately a table, not a gazetteer: anything not in it stays
//! unrecognized rather than guessed.
//!
//! Region membership has three answers. "North America" certainly includes
//! the United States and Canada, but only *maybe* Mexico: postings disagree,
//! so an assessment built on it is "likely", never "yes".

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::LazyLock;

use jobhunt_core::text::search_key;

/// A country.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Country {
    /// ISO 3166-1 alpha-2.
    pub code: &'static str,
    pub name: &'static str,
    /// Other names and demonyms, lowercase ("usa", "united states", "american").
    aliases: &'static [&'static str],
    /// Standard-time UTC offsets across the country, in minutes.
    pub utc_offsets: (i16, i16),
    /// Currency written with a shared symbol ("$"), when it is one.
    pub dollar: Option<&'static str>,
}

impl fmt::Display for Country {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

macro_rules! country {
    ($code:literal, $name:literal, [$($alias:literal),*], ($lo:expr, $hi:expr)) => {
        Country { code: $code, name: $name, aliases: &[$($alias),*], utc_offsets: ($lo, $hi), dollar: None }
    };
    ($code:literal, $name:literal, [$($alias:literal),*], ($lo:expr, $hi:expr), $dollar:literal) => {
        Country { code: $code, name: $name, aliases: &[$($alias),*], utc_offsets: ($lo, $hi), dollar: Some($dollar) }
    };
}

pub static COUNTRIES: &[Country] = &[
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
        (-600, -300),
        "USD"
    ),
    country!("CA", "Canada", ["canadian"], (-480, -210), "CAD"),
    country!("MX", "Mexico", ["méxico", "mexican"], (-480, -360), "MXN"),
    country!("BR", "Brazil", ["brasil", "brazilian"], (-300, -120)),
    country!(
        "AR",
        "Argentina",
        ["argentinian", "argentine"],
        (-180, -180)
    ),
    country!("CL", "Chile", ["chilean"], (-240, -180)),
    country!("CO", "Colombia", ["colombian"], (-300, -300)),
    country!("PE", "Peru", ["perú", "peruvian"], (-300, -300)),
    country!("UY", "Uruguay", ["uruguayan"], (-180, -180)),
    country!("PY", "Paraguay", [], (-240, -180)),
    country!("BO", "Bolivia", [], (-240, -240)),
    country!("EC", "Ecuador", [], (-300, -300)),
    country!("VE", "Venezuela", [], (-240, -240)),
    country!("CR", "Costa Rica", [], (-360, -360)),
    country!("GT", "Guatemala", [], (-360, -360)),
    country!("PA", "Panama", ["panamá"], (-300, -300)),
    country!("DO", "Dominican Republic", [], (-240, -240)),
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
        ],
        (0, 0)
    ),
    country!("IE", "Ireland", ["irish"], (0, 0)),
    country!("PT", "Portugal", ["portuguese"], (-60, 0)),
    country!("ES", "Spain", ["españa", "spanish"], (0, 60)),
    country!("FR", "France", ["french"], (60, 60)),
    country!("DE", "Germany", ["deutschland", "german"], (60, 60)),
    country!(
        "NL",
        "Netherlands",
        ["the netherlands", "holland", "dutch"],
        (60, 60)
    ),
    country!("BE", "Belgium", ["belgian"], (60, 60)),
    country!("LU", "Luxembourg", [], (60, 60)),
    country!("CH", "Switzerland", ["swiss"], (60, 60)),
    country!("AT", "Austria", ["austrian"], (60, 60)),
    country!("IT", "Italy", ["italian"], (60, 60)),
    country!("PL", "Poland", ["polish"], (60, 60)),
    country!("CZ", "Czechia", ["czech republic", "czech"], (60, 60)),
    country!("SK", "Slovakia", [], (60, 60)),
    country!("HU", "Hungary", ["hungarian"], (60, 60)),
    country!("DK", "Denmark", ["danish"], (60, 60)),
    country!("SE", "Sweden", ["swedish"], (60, 60)),
    country!("NO", "Norway", ["norwegian"], (60, 60)),
    country!("FI", "Finland", ["finnish"], (120, 120)),
    country!("IS", "Iceland", [], (0, 0)),
    country!("EE", "Estonia", [], (120, 120)),
    country!("LV", "Latvia", [], (120, 120)),
    country!("LT", "Lithuania", [], (120, 120)),
    country!("GR", "Greece", ["greek"], (120, 120)),
    country!("RO", "Romania", ["romanian"], (120, 120)),
    country!("BG", "Bulgaria", [], (120, 120)),
    country!("HR", "Croatia", [], (60, 60)),
    country!("SI", "Slovenia", [], (60, 60)),
    country!("RS", "Serbia", [], (60, 60)),
    country!("UA", "Ukraine", ["ukrainian"], (120, 120)),
    country!("CY", "Cyprus", [], (120, 120)),
    country!("MT", "Malta", [], (60, 60)),
    country!("TR", "Turkey", ["türkiye", "turkiye"], (180, 180)),
    country!("IL", "Israel", ["israeli"], (120, 120)),
    country!(
        "AE",
        "United Arab Emirates",
        ["uae", "emirates"],
        (240, 240)
    ),
    country!("SA", "Saudi Arabia", [], (180, 180)),
    country!("EG", "Egypt", [], (120, 120)),
    country!("ZA", "South Africa", [], (120, 120)),
    country!("NG", "Nigeria", ["nigerian"], (60, 60)),
    country!("KE", "Kenya", [], (180, 180)),
    country!("GH", "Ghana", [], (0, 0)),
    country!("MA", "Morocco", [], (0, 60)),
    country!("IN", "India", ["indian"], (330, 330)),
    country!("PK", "Pakistan", [], (300, 300)),
    country!("BD", "Bangladesh", [], (360, 360)),
    country!("LK", "Sri Lanka", [], (330, 330)),
    country!("SG", "Singapore", [], (480, 480), "SGD"),
    country!("MY", "Malaysia", [], (480, 480)),
    country!("ID", "Indonesia", [], (420, 540)),
    country!("PH", "Philippines", ["filipino"], (480, 480)),
    country!("VN", "Vietnam", ["viet nam"], (420, 420)),
    country!("TH", "Thailand", [], (420, 420)),
    country!("JP", "Japan", ["japanese"], (540, 540)),
    country!(
        "KR",
        "South Korea",
        ["korea", "republic of korea"],
        (540, 540)
    ),
    country!("CN", "China", ["chinese", "mainland china"], (480, 480)),
    country!("HK", "Hong Kong", [], (480, 480), "HKD"),
    country!("TW", "Taiwan", [], (480, 480)),
    country!("AU", "Australia", ["australian"], (480, 600), "AUD"),
    country!("NZ", "New Zealand", ["aotearoa"], (720, 720), "NZD"),
];

pub fn country(code: &str) -> Option<&'static Country> {
    COUNTRIES.iter().find(|c| c.code.eq_ignore_ascii_case(code))
}

/// Business regions job postings name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Region {
    NorthAmerica,
    LatinAmerica,
    SouthAmerica,
    CentralAmerica,
    Americas,
    Europe,
    EuropeanUnion,
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
    /// Postings disagree (Mexico in "North America", Turkey in "Europe").
    Maybe,
    No,
}

const EU: &[&str] = &[
    "AT", "BE", "BG", "HR", "CY", "CZ", "DK", "EE", "FI", "FR", "DE", "GR", "HU", "IE", "IT", "LV",
    "LT", "LU", "MT", "NL", "PL", "PT", "RO", "SK", "SI", "ES", "SE",
];
const EUROPE_EXTRA: &[&str] = &["GB", "CH", "NO", "IS", "RS", "UA"];
const SOUTH_AMERICA: &[&str] = &["BR", "AR", "CL", "CO", "PE", "UY", "PY", "BO", "EC", "VE"];
const CENTRAL_AMERICA: &[&str] = &["CR", "GT", "PA"];
const MIDDLE_EAST: &[&str] = &["IL", "AE", "SA", "TR", "EG"];
const AFRICA: &[&str] = &["ZA", "NG", "KE", "GH", "MA", "EG"];
const ASIA: &[&str] = &[
    "IN", "PK", "BD", "LK", "SG", "MY", "ID", "PH", "VN", "TH", "JP", "KR", "CN", "HK", "TW",
];
const OCEANIA: &[&str] = &["AU", "NZ"];

impl Region {
    pub const ALL: [Region; 15] = [
        Self::NorthAmerica,
        Self::LatinAmerica,
        Self::SouthAmerica,
        Self::CentralAmerica,
        Self::Americas,
        Self::Europe,
        Self::EuropeanUnion,
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
            Self::Americas => "the Americas",
            Self::Europe => "Europe",
            Self::EuropeanUnion => "the EU",
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
            Self::Americas => &["americas", "the americas", "amer"],
            Self::Europe => &["europe", "european", "eu uk", "uk eu"],
            Self::EuropeanUnion => &["eu", "european union", "eea", "eu eea"],
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

    /// Whether the region includes `country`.
    pub fn contains(self, country: &Country) -> Membership {
        let c = country.code;
        let any = |list: &[&str]| list.contains(&c);
        let yes = |b: bool| if b { Membership::Yes } else { Membership::No };
        match self {
            Self::NorthAmerica => match c {
                "US" | "CA" => Membership::Yes,
                "MX" => Membership::Maybe,
                _ => Membership::No,
            },
            Self::LatinAmerica => {
                yes(c == "MX" || c == "DO" || any(SOUTH_AMERICA) || any(CENTRAL_AMERICA))
            }
            Self::SouthAmerica => yes(any(SOUTH_AMERICA)),
            Self::CentralAmerica => match c {
                _ if any(CENTRAL_AMERICA) => Membership::Yes,
                "MX" => Membership::Maybe,
                _ => Membership::No,
            },
            Self::Americas => yes(matches!(c, "US" | "CA" | "MX" | "DO")
                || any(SOUTH_AMERICA)
                || any(CENTRAL_AMERICA)),
            Self::Europe => match c {
                _ if any(EU) || any(EUROPE_EXTRA) => Membership::Yes,
                "TR" | "CY" => Membership::Maybe,
                _ => Membership::No,
            },
            Self::EuropeanUnion => match c {
                _ if any(EU) => Membership::Yes,
                // "EU" in postings often loosely means Europe.
                _ if any(EUROPE_EXTRA) => Membership::Maybe,
                _ => Membership::No,
            },
            Self::Nordics => yes(matches!(c, "SE" | "NO" | "DK" | "FI" | "IS")),
            Self::Dach => yes(matches!(c, "DE" | "AT" | "CH")),
            Self::Emea => match c {
                _ if any(EU) || any(EUROPE_EXTRA) || any(MIDDLE_EAST) || any(AFRICA) => {
                    Membership::Yes
                }
                _ => Membership::No,
            },
            Self::MiddleEast => yes(any(MIDDLE_EAST)),
            Self::Africa => yes(any(AFRICA)),
            Self::Apac => match c {
                _ if any(ASIA) || any(OCEANIA) => Membership::Yes,
                "PK" | "BD" | "LK" => Membership::Maybe,
                _ => Membership::No,
            },
            Self::Asia => yes(any(ASIA)),
            Self::Oceania => yes(any(OCEANIA)),
        }
    }

    /// Standard-time UTC offsets the region spans, in minutes.
    pub fn utc_offsets(self) -> (i16, i16) {
        let mut range: Option<(i16, i16)> = None;
        for c in COUNTRIES {
            if self.contains(c) == Membership::Yes {
                range = Some(match range {
                    None => c.utc_offsets,
                    Some((lo, hi)) => (lo.min(c.utc_offsets.0), hi.max(c.utc_offsets.1)),
                });
            }
        }
        range.unwrap_or((0, 0))
    }
}

/// First-level subdivisions that postings use to qualify a city.
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
// Brazilian states whose codes do not collide with US states.
static BR_STATES: &[Subdivision] = subdivisions!("BR":
    ("SP", "São Paulo State"), ("RJ", "Rio de Janeiro State"), ("RS", "Rio Grande do Sul"),
    ("MG", "Minas Gerais"), ("PR", "Paraná"), ("SC", "Santa Catarina"), ("BA", "Bahia"),
    ("PE", "Pernambuco"), ("CE", "Ceará"), ("DF", "Distrito Federal"), ("GO", "Goiás"),
    ("ES", "Espírito Santo"),
);

/// A city and its country.
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
    ("VN", "Ho Chi Minh City"),
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

/// What a place names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    /// Anywhere in the world.
    Worldwide,
    Region(Region),
    Country(&'static Country),
    /// A state or province.
    Subdivision {
        country: &'static Country,
        name: &'static str,
    },
    City {
        name: &'static str,
        country: &'static Country,
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

    /// Standard-time UTC offsets, in minutes.
    pub fn utc_offsets(&self) -> Option<(i16, i16)> {
        match self {
            Self::Worldwide => None,
            Self::Region(r) => Some(r.utc_offsets()),
            Self::Country(c)
            | Self::Subdivision { country: c, .. }
            | Self::City { country: c, .. } => Some(c.utc_offsets),
        }
    }
}

impl fmt::Display for Area {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Worldwide => f.write_str("anywhere"),
            Self::Region(r) => f.write_str(r.name()),
            Self::Country(c) => f.write_str(c.name),
            Self::Subdivision { country, name } => write!(f, "{name}, {}", country.name),
            Self::City { name, country } => write!(f, "{name}, {}", country.name),
        }
    }
}

/// Every name JobHunt knows, normalized with [`search_key`], in order of
/// precedence: "Singapore" is the country, "Washington" the state.
static NAMES: LazyLock<HashMap<String, Area>> = LazyLock::new(|| {
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
    for c in COUNTRIES {
        add(c.name, Area::Country(c));
        for alias in c.aliases {
            add(alias, Area::Country(c));
        }
    }
    for city in CITIES {
        if let Some(country) = country(city.country) {
            for name in city.names {
                add(
                    name,
                    Area::City {
                        name: city.display,
                        country,
                    },
                );
            }
        }
    }
    for list in [US_STATES, CA_PROVINCES, AU_STATES, BR_STATES] {
        for sub in list {
            let Some(country) = country(sub.country) else {
                continue;
            };
            let area = Area::Subdivision {
                country,
                name: sub.name,
            };
            add(sub.name, area);
            let key = search_key(sub.name);
            if let Some(short) = key.strip_suffix(" state")
                && sub.country != "BR"
            {
                add(short, area);
            }
        }
    }
    index
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

/// Looks up a single name: a region, country, city or subdivision name.
/// Two- and three-letter codes are handled by [`lookup_code`].
pub fn lookup_name(text: &str) -> Option<Area> {
    let key = search_key(text);
    if key.is_empty() {
        return None;
    }
    NAMES.get(&key).copied().or_else(|| {
        let trimmed: Vec<&str> = key
            .split(' ')
            .filter(|w| !PLACE_NOISE.contains(w))
            .collect();
        (trimmed.len() < key.split(' ').count() && !trimmed.is_empty())
            .then(|| NAMES.get(&trimmed.join(" ")).copied())
            .flatten()
    })
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
        let own = [US_STATES, CA_PROVINCES, AU_STATES, BR_STATES]
            .iter()
            .flat_map(|list| list.iter())
            .find(|s| s.code == code && s.country == near.code);
        if let Some(s) = own {
            return Some(Area::Subdivision {
                country: country(s.country)?,
                name: s.name,
            });
        }
    }
    lookup_code(text, after_city)
}

/// [`lookup_code_near`] without a nearby country.
pub fn lookup_code(text: &str, after_city: bool) -> Option<Area> {
    let code = text.trim().trim_end_matches('.').replace('.', "");
    if !(2..=3).contains(&code.len()) || !code.chars().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let subdivision = || {
        [US_STATES, CA_PROVINCES, AU_STATES, BR_STATES]
            .iter()
            .flat_map(|list| list.iter())
            .find(|s| s.code == code)
            .and_then(|s| {
                country(s.country).map(|country| Area::Subdivision {
                    country,
                    name: s.name,
                })
            })
    };
    let by_country = || match code.as_str() {
        "UK" => country("GB").map(Area::Country),
        "USA" => country("US").map(Area::Country),
        _ => country(&code).map(Area::Country),
    };
    if code == "EU" {
        return Some(Area::Region(Region::EuropeanUnion));
    }
    if after_city {
        subdivision().or_else(by_country)
    } else {
        by_country().or_else(subdivision)
    }
}

/// A place named in a location string or a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The text it came from, as written.
    pub raw: String,
    /// What JobHunt recognized; `None` when it did not.
    pub area: Option<Area>,
    /// Marked as remote ("Remote (Canada)", "US Remote").
    pub remote: bool,
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

/// Parses a location string as sources write them: "San Francisco, CA",
/// "Remote (Canada)", "US Remote", "New York City, NY; San Francisco, CA |
/// Seattle, WA", "Chicago, Seattle, NYC, San Francisco, Remote".
pub fn parse_places(text: &str) -> Vec<Place> {
    let mut out = Vec::new();
    for option in split_options(text) {
        out.extend(parse_option(&option));
    }
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
        places.push(Place {
            raw: original.to_owned(),
            area: None,
            remote: remote || main_remote,
        });
    }
    places
}

/// "San Francisco, CA, US" → one place; "Chicago, Seattle, NYC" → three.
fn parse_list(text: &str, remote: bool) -> Vec<Place> {
    let parts: Vec<&str> = text
        .split([',', '-', '–', '—'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    // A whole-string match first ("Washington, D.C.", "Rio de Janeiro").
    let whole = text.trim().trim_matches(|c: char| c == ',' || c == '-');
    if let Some(area) = lookup_name(whole) {
        return vec![Place {
            raw: whole.to_owned(),
            area: Some(area),
            remote,
        }];
    }
    let mut out: Vec<Place> = Vec::new();
    let mut unknown: Vec<String> = Vec::new();
    for part in parts {
        let (stripped, part_remote) = strip_remote(part);
        if stripped.is_empty() {
            if part_remote && !remote {
                // "Remote" as an option of its own: no known area.
                for name in unknown.drain(..) {
                    out.push(Place {
                        raw: name,
                        area: None,
                        remote,
                    });
                }
                out.push(Place {
                    raw: part.to_owned(),
                    area: None,
                    remote: true,
                });
            }
            continue;
        }
        let near = out.last().and_then(|p| p.area).and_then(|a| a.country());
        let after_city = out
            .last()
            .is_some_and(|p| matches!(p.area, Some(Area::City { .. }) | None) && !p.remote)
            || !unknown.is_empty();
        // In a location field a short leftover is a code ("US Remote").
        let code = (stripped.len() <= 3).then(|| stripped.to_uppercase());
        let area = lookup_code_near(part.trim(), after_city, near)
            .or_else(|| {
                code.as_deref()
                    .and_then(|c| lookup_code_near(c, after_city, near))
            })
            .or_else(|| lookup_name(&stripped))
            .or_else(|| lookup_name(part));
        match area {
            Some(Area::Country(c)) | Some(Area::Subdivision { country: c, .. })
                if qualifies(out.last(), &unknown, c) =>
            {
                // A qualifier: attach to the place before it.
                if let Some(name) = unknown.pop() {
                    out.push(Place {
                        raw: format!("{name}, {part}"),
                        area: Some(area.unwrap_or(Area::Country(c))),
                        remote,
                    });
                    // Keep the unrecognized city name visible.
                    if let Some(last) = out.last_mut()
                        && let Some(Area::Subdivision { .. } | Area::Country(_)) = last.area
                    {
                        last.raw = format!("{name}, {part}");
                    }
                } else if let Some(last) = out.last_mut() {
                    last.raw = format!("{}, {part}", last.raw);
                    if last.area.is_none() {
                        last.area = area;
                    }
                }
            }
            Some(area) => {
                for name in unknown.drain(..) {
                    out.push(Place {
                        raw: name,
                        area: None,
                        remote,
                    });
                }
                out.push(Place {
                    raw: part.to_owned(),
                    area: Some(area),
                    remote: remote || part_remote,
                });
            }
            None => unknown.push(part.to_owned()),
        }
    }
    for name in unknown {
        out.push(Place {
            raw: name,
            area: None,
            remote,
        });
    }
    out
}

/// Whether a country or state after a comma qualifies the previous place
/// rather than starting a new one.
fn qualifies(previous: Option<&Place>, unknown: &[String], country: &Country) -> bool {
    if !unknown.is_empty() {
        return true;
    }
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
                    )
                {
                    lookup_code(&phrase, false)
                } else {
                    None
                }
            } else {
                lookup_name(&phrase).filter(|a| {
                    // Demonyms and ordinary words are too loose alone
                    // ("American Express", "Asian cuisine").
                    !(len == 1
                        && matches!(a, Area::Region(_) | Area::Country(_))
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
        assert_eq!(
            areas("Hayward, CA, US"),
            s(&[("California, United States", false)])
        );
        assert_eq!(areas("Remote - LATAM"), s(&[("Latin America", true)]));
        assert_eq!(areas("US Remote"), s(&[("United States", true)]));
        assert_eq!(areas("Anywhere"), s(&[("anywhere", false)]));
        assert_eq!(areas("Atlantis"), s(&[("? Atlantis", false)]));
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
        assert_eq!(Region::Europe.utc_offsets(), (-60, 120));
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
}
