//! Time zones as postings and people write them ("Pacific time", "EST",
//! "UTC-3", "GMT+1", "European time zones", "US hours",
//! "America/Sao_Paulo"), and as places keep them.
//!
//! A place keeps time on a [`Clock`]: an IANA zone, with its daylight
//! saving rules (from the IANA database compiled into `chrono-tz`), or a
//! fixed UTC offset when that is what was written ("UTC-3"). A [`Zone`] is
//! one or more clocks: the one a city keeps, the several a country or a
//! region spans, or the ends of a stated range ("between UTC-5 and UTC+1").
//!
//! Offsets are never compared as permanent numbers. São Paulo is two hours
//! from New York in January and one in July; London and New York are five
//! hours apart except for the weeks when only one of them has changed its
//! clocks. Every comparison is made day by day across a reference year
//! ([`REFERENCE_YEAR`], each day at 12:00 UTC), so an answer holds all
//! year, or says for which part of the year it doesn't. [`Zone::at`] and
//! [`Clock::offset_at`] answer for one instant.
//!
//! Named zones mean what postings mean by them: "EST" and "PST" are US
//! Eastern and Pacific time, daylight saving included, not the standard
//! offsets alone; "UTC-3" is exactly UTC-3.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use chrono::{DateTime, Datelike, Duration, NaiveDate, Offset, TimeZone, Utc};
use chrono_tz::{OffsetComponents, Tz};
use jobhunt_profile::words::{Pattern, words};

use crate::geo::{Area, Region, format_offset, lookup_name, places_in_text};

/// The calendar year time-zone compatibility is judged over: a decision
/// holds on every day of it, or says it doesn't. A fixed year keeps
/// decisions deterministic and cacheable; moving it to a new year (or
/// updating the IANA database, [`chrono_tz::IANA_TZDB_VERSION`]) can
/// change decisions, so it is part of every stored decision's key (see
/// [`crate::cache`]).
pub const REFERENCE_YEAR: i32 = 2026;

/// A range of UTC offsets, in minutes east of UTC.
pub type Offsets = (i16, i16);

/// How a place keeps time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Clock {
    /// An IANA zone ("America/Sao_Paulo"), daylight saving included.
    Iana(Tz),
    /// A fixed offset, in minutes east of UTC, as written ("UTC-3").
    Fixed(i16),
}

impl Clock {
    /// Parses an IANA zone name ("Europe/Lisbon").
    pub fn iana(name: &str) -> Option<Self> {
        name.parse::<Tz>().ok().map(Self::Iana)
    }

    /// The UTC offset at an instant, in minutes.
    pub fn offset_at(self, at: DateTime<Utc>) -> i16 {
        match self {
            Self::Iana(tz) => {
                let seconds = tz
                    .offset_from_utc_datetime(&at.naive_utc())
                    .fix()
                    .local_minus_utc();
                i16::try_from(seconds / 60).unwrap_or(0)
            }
            Self::Fixed(minutes) => minutes,
        }
    }

    /// "America/Sao_Paulo", "UTC-3".
    pub fn name(self) -> String {
        match self {
            Self::Iana(tz) => tz.name().to_owned(),
            Self::Fixed(minutes) => format_offset(minutes),
        }
    }

    /// The offset on every day of the reference year (remembered).
    fn year(self) -> Arc<[i16]> {
        static YEARS: LazyLock<Mutex<HashMap<Clock, Arc<[i16]>>>> = LazyLock::new(Mutex::default);
        if let Some(found) = YEARS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&self)
        {
            return Arc::clone(found);
        }
        let year: Arc<[i16]> = reference_days()
            .iter()
            .map(|day| self.offset_at(*day))
            .collect();
        YEARS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(self, Arc::clone(&year));
        year
    }
}

impl Ord for Clock {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Iana(a), Self::Iana(b)) => a.name().cmp(b.name()),
            (Self::Fixed(a), Self::Fixed(b)) => a.cmp(b),
            (Self::Iana(_), Self::Fixed(_)) => Ordering::Less,
            (Self::Fixed(_), Self::Iana(_)) => Ordering::Greater,
        }
    }
}

impl PartialOrd for Clock {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Every day of [`REFERENCE_YEAR`], at 12:00 UTC.
pub fn reference_days() -> &'static [DateTime<Utc>] {
    static DAYS: LazyLock<Vec<DateTime<Utc>>> = LazyLock::new(|| {
        let mut out = Vec::with_capacity(366);
        let Some(mut day) = Utc
            .with_ymd_and_hms(REFERENCE_YEAR, 1, 1, 12, 0, 0)
            .single()
        else {
            return out;
        };
        while day.year() == REFERENCE_YEAR {
            out.push(day);
            day += Duration::days(1);
        }
        out
    });
    &DAYS
}

/// "Mar 8" for a day of the reference year.
fn day_label(index: usize) -> String {
    reference_days()
        .get(index)
        .map(|d| d.format("%b %-d").to_string())
        .unwrap_or_default()
}

/// One or more clocks: the one a city keeps, the ones a country or region
/// spans, or the ends of a stated range. With several, where exactly the
/// person (or the requirement) is between them is not known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zone {
    /// Sorted, without duplicates.
    clocks: Vec<Clock>,
    /// For each day of the reference year, the lowest and highest offset
    /// among the clocks.
    days: Arc<[Offsets]>,
}

impl Zone {
    /// The zone of these clocks; `None` when there are none.
    pub fn new(clocks: impl IntoIterator<Item = Clock>) -> Option<Self> {
        let mut clocks: Vec<Clock> = clocks.into_iter().collect();
        clocks.sort();
        clocks.dedup();
        let years: Vec<Arc<[i16]>> = clocks.iter().map(|c| c.year()).collect();
        let first = years.first()?;
        let days: Arc<[Offsets]> = (0..first.len())
            .map(|d| {
                years.iter().fold((i16::MAX, i16::MIN), |(lo, hi), y| {
                    (lo.min(y[d]), hi.max(y[d]))
                })
            })
            .collect();
        Some(Self { clocks, days })
    }

    pub fn fixed(minutes: i16) -> Self {
        Self::of(Clock::Fixed(minutes))
    }

    pub fn of(clock: Clock) -> Self {
        let year = clock.year();
        Self {
            clocks: vec![clock],
            days: year.iter().map(|o| (*o, *o)).collect(),
        }
    }

    /// An IANA zone by name ("Asia/Kolkata").
    pub fn iana(name: &str) -> Option<Self> {
        Clock::iana(name).map(Self::of)
    }

    /// Both zones' clocks.
    pub fn union(&self, other: &Zone) -> Zone {
        Self::new(self.clocks.iter().chain(&other.clocks).copied()).unwrap_or_else(|| self.clone())
    }

    pub fn clocks(&self) -> &[Clock] {
        &self.clocks
    }

    /// The lowest and highest offset among the clocks at an instant.
    pub fn at(&self, at: DateTime<Utc>) -> Offsets {
        self.clocks
            .iter()
            .map(|c| c.offset_at(at))
            .fold((i16::MAX, i16::MIN), |(lo, hi), o| (lo.min(o), hi.max(o)))
    }

    /// The lowest and highest offset on each day of the reference year.
    pub fn days(&self) -> &[Offsets] {
        &self.days
    }

    /// The lowest and highest offset over the whole reference year.
    pub fn year_range(&self) -> Offsets {
        self.days
            .iter()
            .fold((i16::MAX, i16::MIN), |(lo, hi), (a, b)| {
                (lo.min(*a), hi.max(*b))
            })
    }

    /// Whether the clocks disagree on some day: where exactly within the
    /// zone someone is matters.
    pub fn is_spread(&self) -> bool {
        self.days.iter().any(|(lo, hi)| lo != hi)
    }

    /// Whether its offsets change during the year (daylight saving time).
    pub fn is_seasonal(&self) -> bool {
        self.days.windows(2).any(|w| w[0] != w[1])
    }

    /// "America/Sao_Paulo (UTC-3)", "Europe/Lisbon (UTC+0; UTC+1 with
    /// daylight saving time)", "UTC-3", "UTC-10 to UTC-4".
    pub fn label(&self) -> String {
        let (lo, hi) = self.year_range();
        match self.clocks.as_slice() {
            [Clock::Fixed(m)] => format_offset(*m),
            [clock @ Clock::Iana(_)] if lo == hi => {
                format!("{} ({})", clock.name(), format_offset(lo))
            }
            [clock @ Clock::Iana(tz)] => {
                // The IANA database says which offset is standard time.
                let standard = reference_days().first().map_or(lo, |day| {
                    let offset = tz.offset_from_utc_datetime(&day.naive_utc());
                    i16::try_from(offset.base_utc_offset().num_minutes()).unwrap_or(lo)
                });
                let dst = if standard == lo { hi } else { lo };
                format!(
                    "{} ({}; {} with daylight saving time)",
                    clock.name(),
                    format_offset(standard),
                    format_offset(dst)
                )
            }
            _ => offsets_text((lo, hi)),
        }
    }
}

/// "UTC-3", "UTC-5 to UTC-2".
pub fn offsets_text((lo, hi): Offsets) -> String {
    if lo == hi {
        format_offset(lo)
    } else {
        format!("{} to {}", format_offset(lo), format_offset(hi))
    }
}

/// Hours between two offset ranges (0 when they overlap).
pub fn distance_hours(a: Offsets, b: Offsets) -> f32 {
    let gap = if a.1 < b.0 {
        b.0 - a.1
    } else if b.1 < a.0 {
        a.0 - b.1
    } else {
        0
    };
    f32::from(gap) / 60.0
}

/// The hours between two clocks at an instant (positive when `b` is ahead
/// of `a`).
pub fn difference_hours_at(a: Clock, b: Clock, at: DateTime<Utc>) -> f32 {
    f32::from(b.offset_at(at) - a.offset_at(at)) / 60.0
}

/// Days of the reference year grouped by how two zones stand: each
/// distinct pair of daily offset ranges, with the days it holds on.
pub fn day_pairs(mine: &Zone, theirs: &Zone) -> Vec<((Offsets, Offsets), Vec<usize>)> {
    let mut out: Vec<((Offsets, Offsets), Vec<usize>)> = Vec::new();
    for (d, pair) in mine
        .days
        .iter()
        .copied()
        .zip(theirs.days.iter().copied())
        .enumerate()
    {
        match out.iter_mut().find(|(p, _)| *p == pair) {
            Some((_, days)) => days.push(d),
            None => out.push((pair, vec![d])),
        }
    }
    out
}

/// "Mar 8–Oct 31", "Oct 4–Apr 4" for days of the reference year (runs
/// that wrap the year's end are joined).
pub fn days_text(days: &[usize]) -> String {
    let total = reference_days().len();
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for &d in days {
        match runs.last_mut() {
            Some((_, end)) if *end + 1 == d => *end = d,
            _ => runs.push((d, d)),
        }
    }
    if runs.len() > 1
        && let (Some(&(0, first_end)), Some(&(last_start, last_end))) = (runs.first(), runs.last())
        && last_end + 1 == total
    {
        runs.remove(0);
        runs.pop();
        runs.push((last_start, first_end));
    }
    let mut out = String::new();
    for (i, (start, end)) in runs.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        if start == end {
            out.push_str(&day_label(*start));
        } else {
            let _ = write!(out, "{}–{}", day_label(*start), day_label(*end));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Reading zones from text

const ABBREVIATIONS: [(&str, &str); 22] = [
    ("PT", "America/Los_Angeles"),
    ("PST", "America/Los_Angeles"),
    ("PDT", "America/Los_Angeles"),
    ("MT", "America/Denver"),
    ("MST", "America/Denver"),
    ("MDT", "America/Denver"),
    ("CT", "America/Chicago"),
    ("CST", "America/Chicago"),
    ("CDT", "America/Chicago"),
    ("ET", "America/New_York"),
    ("EST", "America/New_York"),
    ("EDT", "America/New_York"),
    ("BRT", "America/Sao_Paulo"),
    ("GMT", "UTC"),
    ("UTC", "UTC"),
    ("WET", "Europe/Lisbon"),
    ("BST", "Europe/London"),
    ("CET", "Europe/Paris"),
    ("CEST", "Europe/Paris"),
    ("EET", "Europe/Athens"),
    // India, as postings use it (also Israel's and Ireland's abbreviation).
    ("IST", "Asia/Kolkata"),
    ("AEST", "Australia/Sydney"),
];

const NAMED: [(&str, &str); 9] = [
    ("pacific time*", "America/Los_Angeles"),
    ("pacific", "America/Los_Angeles"),
    ("mountain time*", "America/Denver"),
    ("central time*", "America/Chicago"),
    ("central standard", "America/Chicago"),
    ("eastern time*", "America/New_York"),
    ("eastern standard", "America/New_York"),
    ("atlantic time*", "America/Halifax"),
    ("brasília time", "America/Sao_Paulo"),
];

const EUROPEAN: [(&str, &str); 3] = [
    ("western european", "Europe/Lisbon"),
    ("central european", "Europe/Paris"),
    ("eastern european", "Europe/Athens"),
];

/// The zones US working hours mean: the continental ones.
const US_HOURS: [&str; 5] = [
    "America/Los_Angeles",
    "America/Phoenix",
    "America/Denver",
    "America/Chicago",
    "America/New_York",
];

fn named(name: &str) -> Zone {
    if name == "UTC" {
        return Zone::fixed(0);
    }
    Zone::iana(name).unwrap_or_else(|| Zone::fixed(0))
}

/// The zones a sentence or preference names, each with the words that
/// named it.
pub fn zones_in(text: &str) -> Vec<(String, Zone)> {
    let ws = words(text);
    let mut out: Vec<(String, Zone)> = Vec::new();
    let push = |label: String, zone: Zone, out: &mut Vec<(String, Zone)>| {
        if !out.iter().any(|(_, z)| *z == zone) {
            out.push((label, zone));
        }
    };
    // IANA names as written ("America/Sao_Paulo").
    for token in text.split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')')) {
        let token = token.trim_matches(|c: char| matches!(c, '.' | ':' | '"' | '\''));
        if token.contains('/')
            && let Some(zone) = Zone::iana(token)
        {
            push(token.to_owned(), zone, &mut out);
        }
    }
    let mut european = false;
    for (pattern, name) in EUROPEAN {
        if let Some(r) = Pattern::new(pattern).find(&ws) {
            european = true;
            push(
                ws[r.start..r.end]
                    .iter()
                    .map(|w| w.original.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                named(name),
                &mut out,
            );
        }
    }
    for (pattern, name) in NAMED {
        // "Eastern European" is not US Eastern time.
        if european && (pattern.starts_with("eastern") || pattern.starts_with("central")) {
            continue;
        }
        if let Some(r) = Pattern::new(pattern).find(&ws) {
            let label = ws[r.start..r.end]
                .iter()
                .map(|w| w.original.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            push(label, named(name), &mut out);
        }
    }
    // "Eastern, Pacific and Western European time zones": bare
    // "Eastern"/"Central" next to a time-zone word.
    if has_zone_word(text) {
        for (word, name) in [
            ("Eastern", "America/New_York"),
            ("Central", "America/Chicago"),
            ("Mountain", "America/Denver"),
        ] {
            let followed_by_european = ws
                .windows(2)
                .any(|w| w[0].original == word && w[1].lower == "european");
            if !followed_by_european && ws.iter().any(|w| w.original == word) {
                push(word.to_owned(), named(name), &mut out);
            }
        }
    }
    for w in &ws {
        // "UTC+3" is one word; "UTC-3" is "UTC" followed by "-3".
        for prefix in ["UTC", "GMT"] {
            if w.original.len() > prefix.len()
                && w.original.starts_with(prefix)
                && let Some(rest) = text.get(w.span.start + prefix.len()..)
                && let Some(minutes) = parse_signed_offset(rest)
            {
                let label = format!("{prefix}{}", &rest[..offset_len(rest)]);
                push(label, Zone::fixed(minutes), &mut out);
            }
        }
        if let Some((_, name)) = ABBREVIATIONS.iter().find(|(a, _)| *a == w.original) {
            if matches!(w.original.as_str(), "UTC" | "GMT") {
                let after = &text[w.span.end..];
                if let Some(minutes) = parse_signed_offset(after) {
                    let label = format!("{}{}", w.original, &after[..offset_len(after)]);
                    push(label, Zone::fixed(minutes), &mut out);
                    continue;
                }
            }
            push(w.original.clone(), named(name), &mut out);
        }
    }
    // "US time zones", "European hours", "Americas time zones".
    for (i, w) in ws.iter().enumerate() {
        let next = ws.get(i + 1).map(|n| n.lower.as_str());
        let zoneish = matches!(
            next,
            Some("time" | "timezone" | "timezones" | "hours" | "business")
        );
        if !zoneish {
            continue;
        }
        let area =
            if w.original == "US" || w.lower == "american" && i > 0 && ws[i - 1].lower == "north" {
                Some(Area::Region(Region::NorthAmerica))
            } else if w.lower == "european" {
                if european {
                    None
                } else {
                    Some(Area::Region(Region::Europe))
                }
            } else {
                lookup_name(&w.original).or_else(|| places_in_text(&w.original).into_iter().next())
            };
        if let Some(area) = area
            && let Some(zone) = match area {
                // US working hours mean the continental zones.
                Area::Region(Region::NorthAmerica) => us_hours(),
                Area::Country(c) if c.code == "US" => us_hours(),
                other => other.zone(),
            }
        {
            push(
                format!("{} {}", w.original, next.unwrap_or_default()),
                zone,
                &mut out,
            );
        }
    }
    out
}

fn us_hours() -> Option<Zone> {
    Zone::new(US_HOURS.iter().filter_map(|n| Clock::iana(n)))
}

fn has_zone_word(text: &str) -> bool {
    let ws = words(text);
    ["time zone*", "timezone*", "time", "hours", "overlap"]
        .iter()
        .any(|p| Pattern::new(p).find(&ws).is_some())
}

fn offset_len(after: &str) -> usize {
    after
        .char_indices()
        .take_while(|(i, c)| {
            (*i == 0 && matches!(c, '+' | '-' | '−')) || c.is_ascii_digit() || *c == ':'
        })
        .map(|(i, c)| i + c.len_utf8())
        .last()
        .unwrap_or(0)
}

/// "+5:30" → 330, "-3" → -180.
fn parse_signed_offset(after: &str) -> Option<i16> {
    let len = offset_len(after);
    let text = after.get(..len)?;
    let sign = match text.chars().next()? {
        '+' => 1,
        '-' | '−' => -1,
        _ => return None,
    };
    let body = &text[text.chars().next()?.len_utf8()..];
    let (hours, minutes) = match body.split_once(':') {
        Some((h, m)) => (h.parse::<i16>().ok()?, m.parse::<i16>().ok()?),
        None => (body.parse::<i16>().ok()?, 0),
    };
    ((0..=14).contains(&hours) && (0..60).contains(&minutes))
        .then_some(sign * (hours * 60 + minutes))
}

/// A day of the reference year as a date (tests, labels).
pub fn reference_date(month: u32, day: u32) -> Option<DateTime<Utc>> {
    NaiveDate::from_ymd_opt(REFERENCE_YEAR, month, day)
        .and_then(|d| d.and_hms_opt(12, 0, 0))
        .map(|d| d.and_utc())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(text: &str) -> Vec<String> {
        zones_in(text).into_iter().map(|(l, _)| l).collect()
    }

    fn at(month: u32, day: u32) -> DateTime<Utc> {
        reference_date(month, day).unwrap()
    }

    fn clock(name: &str) -> Clock {
        Clock::iana(name).unwrap()
    }

    #[test]
    fn reads_zones_from_postings() {
        assert_eq!(
            labels("people (in the US - Pacific timezone)"),
            ["Pacific timezone"]
        );
        assert_eq!(
            zones_in("people (in the US - Pacific timezone)")[0].1,
            Zone::iana("America/Los_Angeles").unwrap()
        );
        assert_eq!(labels("Overlap with EST until 2pm"), ["EST"]);
        assert_eq!(
            labels("anywhere between UTC-3 and UTC+3"),
            ["UTC-3", "UTC+3"]
        );
        assert_eq!(zones_in("UTC+5:30 preferred")[0].1, Zone::fixed(330));
        assert_eq!(
            labels("Operating primarily across Eastern, Pacific and Western European time zones"),
            ["Western European", "Pacific", "Eastern"]
        );
        assert_eq!(labels("European time zones"), ["European time"]);
        let us = &zones_in("must work US hours")[0].1;
        assert_eq!(us.clocks().len(), 5);
        assert_eq!(us.at(at(1, 15)), (-480, -300));
        assert_eq!(us.at(at(7, 15)), (-420, -240));
        assert!(labels("You can work from most timezones within these regions").is_empty());
        assert!(labels("We use GitHub and Slack").is_empty());
        // IANA names as written.
        assert_eq!(
            zones_in("I work on America/Sao_Paulo time")[0].1,
            Zone::iana("America/Sao_Paulo").unwrap()
        );
    }

    #[test]
    fn every_named_zone_is_an_iana_zone() {
        let names = ABBREVIATIONS
            .iter()
            .chain(NAMED.iter())
            .chain(EUROPEAN.iter())
            .map(|(_, n)| *n)
            .chain(US_HOURS);
        for name in names {
            assert!(name == "UTC" || Clock::iana(name).is_some(), "{name}");
        }
    }

    #[test]
    fn offsets_follow_daylight_saving_rules() {
        let sao_paulo = clock("America/Sao_Paulo");
        let new_york = clock("America/New_York");
        let london = clock("Europe/London");
        // Brazil keeps no daylight saving time (since 2019); New York does.
        assert_eq!(sao_paulo.offset_at(at(1, 15)), -180);
        assert_eq!(sao_paulo.offset_at(at(7, 15)), -180);
        assert_eq!(new_york.offset_at(at(1, 15)), -300);
        assert_eq!(new_york.offset_at(at(7, 15)), -240);
        // So the gap between them depends on the date.
        assert_eq!(difference_hours_at(sao_paulo, new_york, at(1, 15)), -2.0);
        assert_eq!(difference_hours_at(sao_paulo, new_york, at(7, 15)), -1.0);
        // London and New York change clocks on different weeks: five hours
        // apart most of the year, four in between.
        assert_eq!(difference_hours_at(new_york, london, at(1, 15)), 5.0);
        assert_eq!(difference_hours_at(new_york, london, at(7, 15)), 5.0);
        assert_eq!(difference_hours_at(new_york, london, at(3, 20)), 4.0);
        assert_eq!(difference_hours_at(new_york, london, at(10, 28)), 4.0);
        // The southern hemisphere changes the other way round.
        let sydney = clock("Australia/Sydney");
        assert_eq!(sydney.offset_at(at(1, 15)), 660);
        assert_eq!(sydney.offset_at(at(7, 15)), 600);
        // A fixed offset is fixed.
        assert_eq!(Clock::Fixed(-180).offset_at(at(7, 15)), -180);
    }

    #[test]
    fn zones_know_when_they_change() {
        let sao_paulo = Zone::iana("America/Sao_Paulo").unwrap();
        assert!(!sao_paulo.is_seasonal());
        assert!(!sao_paulo.is_spread());
        assert_eq!(sao_paulo.label(), "America/Sao_Paulo (UTC-3)");
        let lisbon = Zone::iana("Europe/Lisbon").unwrap();
        assert!(lisbon.is_seasonal());
        assert_eq!(lisbon.year_range(), (0, 60));
        assert_eq!(
            lisbon.label(),
            "Europe/Lisbon (UTC+0; UTC+1 with daylight saving time)"
        );
        let sydney = Zone::iana("Australia/Sydney").unwrap();
        assert_eq!(
            sydney.label(),
            "Australia/Sydney (UTC+10; UTC+11 with daylight saving time)"
        );
        let range = Zone::new([Clock::Fixed(-300), Clock::Fixed(60)]).unwrap();
        assert!(range.is_spread());
        assert!(!range.is_seasonal());
        assert_eq!(range.label(), "UTC-5 to UTC+1");
        assert_eq!(Zone::fixed(-180).label(), "UTC-3");
        assert_eq!(reference_days().len(), 365);
    }

    #[test]
    fn pairs_group_the_days_of_the_year() {
        let sao_paulo = Zone::iana("America/Sao_Paulo").unwrap();
        let new_york = Zone::iana("America/New_York").unwrap();
        let pairs = day_pairs(&sao_paulo, &new_york);
        assert_eq!(pairs.len(), 2);
        let summer = pairs
            .iter()
            .find(|((_, ny), _)| *ny == (-240, -240))
            .unwrap();
        // US daylight saving time, 2026: March 8 to November 1.
        assert_eq!(days_text(&summer.1), "Mar 8–Oct 31");
        let winter = pairs
            .iter()
            .find(|((_, ny), _)| *ny == (-300, -300))
            .unwrap();
        assert_eq!(days_text(&winter.1), "Nov 1–Mar 7");
    }

    #[test]
    fn distances() {
        assert_eq!(distance_hours((-480, -480), (-180, -180)), 5.0);
        assert_eq!(distance_hours((-480, -300), (-360, -360)), 0.0);
        assert_eq!(offsets_text((-300, -120)), "UTC-5 to UTC-2");
    }
}
