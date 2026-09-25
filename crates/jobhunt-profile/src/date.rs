//! Partial calendar dates, as resumes write them.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A year, optionally with a month (`2021` or `2021-03`). Resumes rarely
/// give days, and a missing month is kept missing rather than guessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PartialDate {
    year: i32,
    month: Option<u8>,
}

impl PartialDate {
    pub fn new(year: i32, month: Option<u8>) -> Option<Self> {
        let year_ok = (1900..=2200).contains(&year);
        let month_ok = month.is_none_or(|m| (1..=12).contains(&m));
        (year_ok && month_ok).then_some(Self { year, month })
    }

    pub fn year(&self) -> i32 {
        self.year
    }

    pub fn month(&self) -> Option<u8> {
        self.month
    }

    /// The month this date denotes, or January for a bare year. Only for
    /// ordering and overlap checks; never displayed.
    fn month_index(&self) -> i32 {
        self.year * 12 + i32::from(self.month.unwrap_or(1)) - 1
    }

    /// The date of `at`, to month precision.
    pub fn of(at: DateTime<Utc>) -> Self {
        Self {
            year: at.year(),
            month: u8::try_from(at.month()).ok(),
        }
    }

    /// "Mar 2021" or "2021".
    pub fn display_short(&self) -> String {
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        match self.month {
            Some(m) => format!("{} {}", MONTHS[usize::from(m - 1)], self.year),
            None => self.year.to_string(),
        }
    }
}

impl Ord for PartialDate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.month_index()
            .cmp(&other.month_index())
            .then(self.month.is_some().cmp(&other.month.is_some()))
    }
}

impl PartialOrd for PartialDate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for PartialDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.month {
            Some(m) => write!(f, "{:04}-{m:02}", self.year),
            None => write!(f, "{:04}", self.year),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a date; use YYYY or YYYY-MM (for example 2021 or 2021-03)")]
pub struct ParseDateError(pub String);

impl FromStr for PartialDate {
    type Err = ParseDateError;

    /// Accepts `YYYY`, `YYYY-MM`, `YYYY/MM`, `MM/YYYY` and `Mon YYYY`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseDateError(s.to_owned());
        let t = s.trim();
        let number = |v: &str| -> Option<i32> {
            (!v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
                .then(|| v.parse().ok())
                .flatten()
        };
        if let Some(year) = number(t).filter(|_| t.len() == 4) {
            return Self::new(year, None).ok_or_else(err);
        }
        for sep in ['-', '/', '.'] {
            if let Some((a, b)) = t.split_once(sep) {
                let (year, month) = if a.len() == 4 { (a, b) } else { (b, a) };
                if year.len() == 4 && (1..=2).contains(&month.len()) {
                    let year = number(year).ok_or_else(err)?;
                    let month = number(month)
                        .and_then(|m| u8::try_from(m).ok())
                        .ok_or_else(err)?;
                    return Self::new(year, Some(month)).ok_or_else(err);
                }
            }
        }
        if let Some((name, year)) = t.split_once(char::is_whitespace)
            && let Some(month) = month_from_name(name)
            && let Some(year) = number(year.trim()).filter(|_| year.trim().len() == 4)
        {
            return Self::new(year, Some(month)).ok_or_else(err);
        }
        Err(err())
    }
}

/// Month number for an English or Portuguese/Spanish month name or its
/// usual abbreviation (`Jan`, `January`, `Sept.`, `Fev`, `Ago`).
pub fn month_from_name(name: &str) -> Option<u8> {
    let lower = name.trim().trim_end_matches('.').to_lowercase();
    const NAMES: [(&[&str], u8); 12] = [
        (&["jan", "january", "janeiro", "enero", "ene"], 1),
        (&["feb", "february", "fev", "fevereiro", "febrero"], 2),
        (&["mar", "march", "março", "marco", "marzo"], 3),
        (&["apr", "april", "abr", "abril"], 4),
        (&["may", "mai", "maio", "mayo"], 5),
        (&["jun", "june", "junho", "junio"], 6),
        (&["jul", "july", "julho", "julio"], 7),
        (&["aug", "august", "ago", "agosto"], 8),
        (
            &["sep", "sept", "september", "set", "setembro", "septiembre"],
            9,
        ),
        (&["oct", "october", "out", "outubro", "octubre"], 10),
        (&["nov", "november", "novembro", "noviembre"], 11),
        (
            &["dec", "december", "dez", "dezembro", "dic", "diciembre"],
            12,
        ),
    ];
    NAMES
        .iter()
        .find(|(names, _)| names.contains(&lower.as_str()))
        .map(|(_, month)| *month)
}

impl Serialize for PartialDate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PartialDate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

/// A span of time with possibly unknown ends. `current` means "until now"
/// as the source said it (`Present`), which is different from an unknown end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Period {
    pub start: Option<PartialDate>,
    pub end: Option<PartialDate>,
    pub current: bool,
}

impl Period {
    /// "Mar 2022 – Present", "2014 – 2018", "until Dec 2019", or `None` when
    /// nothing is known.
    pub fn display(&self) -> Option<String> {
        let end = if self.current {
            Some("Present".to_owned())
        } else {
            self.end.map(|d| d.display_short())
        };
        match (self.start.map(|d| d.display_short()), end) {
            (Some(start), Some(end)) if start == end => Some(start),
            (Some(start), Some(end)) => Some(format!("{start} – {end}")),
            (Some(start), None) => Some(format!("from {start}")),
            (None, Some(end)) => Some(format!("until {end}")),
            (None, None) => None,
        }
    }

    /// Whether two periods share at least one month. Unknown bounds never
    /// overlap anything (they are not guessed).
    pub fn overlaps(&self, other: &Period, today: PartialDate) -> bool {
        let bounds = |p: &Period| -> Option<(PartialDate, PartialDate)> {
            let start = p.start?;
            let end = if p.current { Some(today) } else { p.end };
            Some((start, end?))
        };
        match (bounds(self), bounds(other)) {
            (Some((a0, a1)), Some((b0, b1))) => {
                a0.month_index() <= b1.month_index() && b0.month_index() <= a1.month_index()
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> PartialDate {
        s.parse().unwrap()
    }

    #[test]
    fn parses_common_forms() {
        assert_eq!(d("2021"), PartialDate::new(2021, None).unwrap());
        assert_eq!(d("2021-03"), PartialDate::new(2021, Some(3)).unwrap());
        assert_eq!(d("03/2021"), PartialDate::new(2021, Some(3)).unwrap());
        assert_eq!(d("Mar 2021"), PartialDate::new(2021, Some(3)).unwrap());
        assert_eq!(
            d("September 2019"),
            PartialDate::new(2019, Some(9)).unwrap()
        );
        assert_eq!(d("Fev 2020"), PartialDate::new(2020, Some(2)).unwrap());
        assert!("2021-13".parse::<PartialDate>().is_err());
        assert!("21".parse::<PartialDate>().is_err());
        assert!("soon".parse::<PartialDate>().is_err());
    }

    #[test]
    fn displays_and_orders() {
        assert_eq!(d("2021-03").to_string(), "2021-03");
        assert_eq!(d("2021").to_string(), "2021");
        assert_eq!(d("2021-03").display_short(), "Mar 2021");
        assert!(d("2020-12") < d("2021"));
        assert!(d("2021") < d("2021-02"));
    }

    #[test]
    fn periods_display_and_overlap() {
        let today = d("2026-09");
        let a = Period {
            start: Some(d("2020-01")),
            end: Some(d("2022-04")),
            current: false,
        };
        let b = Period {
            start: Some(d("2022-03")),
            end: None,
            current: true,
        };
        let c = Period {
            start: None,
            end: None,
            current: false,
        };
        assert_eq!(a.display().unwrap(), "Jan 2020 – Apr 2022");
        assert_eq!(b.display().unwrap(), "Mar 2022 – Present");
        assert_eq!(c.display(), None);
        assert!(a.overlaps(&b, today));
        assert!(!a.overlaps(&c, today));
    }
}
