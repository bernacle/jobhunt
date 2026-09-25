//! Time zones as postings and people write them: "Pacific time", "EST",
//! "UTC-3", "GMT+1", "European time zones", "US hours".
//!
//! A zone is a range of standard-time UTC offsets in minutes. Daylight
//! saving is ignored: it moves everyone by an hour at most, which is below
//! the precision of these statements.

use jobhunt_profile::words::{Pattern, words};

use crate::geo::{Area, Region, lookup_name, places_in_text};

/// A range of UTC offsets, in minutes east of UTC.
pub type Offsets = (i16, i16);

const ABBREVIATIONS: [(&str, i16); 22] = [
    ("PT", -480),
    ("PST", -480),
    ("PDT", -480),
    ("MT", -420),
    ("MST", -420),
    ("MDT", -420),
    ("CT", -360),
    ("CST", -360),
    ("CDT", -360),
    ("ET", -300),
    ("EST", -300),
    ("EDT", -300),
    ("BRT", -180),
    ("GMT", 0),
    ("UTC", 0),
    ("WET", 0),
    ("BST", 0),
    ("CET", 60),
    ("CEST", 60),
    ("EET", 120),
    ("IST", 330),
    ("AEST", 600),
];

const NAMED: [(&str, i16); 9] = [
    ("pacific time*", -480),
    ("pacific", -480),
    ("mountain time*", -420),
    ("central time*", -360),
    ("central standard", -360),
    ("eastern time*", -300),
    ("eastern standard", -300),
    ("atlantic time*", -240),
    ("brasília time", -180),
];

const EUROPEAN: [(&str, i16); 3] = [
    ("western european", 0),
    ("central european", 60),
    ("eastern european", 120),
];

/// The zones a sentence or preference names, each with the words that
/// named it.
pub fn zones_in(text: &str) -> Vec<(String, Offsets)> {
    let ws = words(text);
    let mut out: Vec<(String, Offsets)> = Vec::new();
    let push = |label: String, range: Offsets, out: &mut Vec<(String, Offsets)>| {
        if !out.iter().any(|(_, r)| *r == range) {
            out.push((label, range));
        }
    };
    let mut european = false;
    for (pattern, offset) in EUROPEAN {
        if let Some(r) = Pattern::new(pattern).find(&ws) {
            european = true;
            push(
                ws[r.start..r.end]
                    .iter()
                    .map(|w| w.original.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                (offset, offset),
                &mut out,
            );
        }
    }
    for (pattern, offset) in NAMED {
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
            push(label, (offset, offset), &mut out);
        }
    }
    // "Eastern, Pacific and Western European time zones": bare
    // "Eastern"/"Central" next to a time-zone word.
    if has_zone_word(text) {
        for (word, offset) in [("Eastern", -300), ("Central", -360), ("Mountain", -420)] {
            let followed_by_european = ws
                .windows(2)
                .any(|w| w[0].original == word && w[1].lower == "european");
            if !followed_by_european && ws.iter().any(|w| w.original == word) {
                push(word.to_owned(), (offset, offset), &mut out);
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
                push(label, (minutes, minutes), &mut out);
            }
        }
        if let Some((_, offset)) = ABBREVIATIONS.iter().find(|(a, _)| *a == w.original) {
            if matches!(w.original.as_str(), "UTC" | "GMT") {
                let after = &text[w.span.end..];
                if let Some(minutes) = parse_signed_offset(after) {
                    let label = format!("{}{}", w.original, &after[..offset_len(after)]);
                    push(label, (minutes, minutes), &mut out);
                    continue;
                }
            }
            push(w.original.clone(), (*offset, *offset), &mut out);
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
            && let Some(range) = match area {
                // US working hours mean the continental zones.
                Area::Region(Region::NorthAmerica) => Some((-480, -300)),
                Area::Country(c) if c.code == "US" => Some((-480, -300)),
                other => other.utc_offsets(),
            }
        {
            push(
                format!("{} {}", w.original, next.unwrap_or_default()),
                range,
                &mut out,
            );
        }
    }
    out
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

/// Joins ranges into the smallest range covering them.
pub fn span(ranges: &[Offsets]) -> Option<Offsets> {
    ranges
        .iter()
        .copied()
        .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranges(text: &str) -> Vec<Offsets> {
        zones_in(text).into_iter().map(|(_, r)| r).collect()
    }

    #[test]
    fn reads_zones_from_postings() {
        assert_eq!(
            ranges("people (in the US - Pacific timezone)"),
            [(-480, -480)]
        );
        assert_eq!(ranges("Overlap with EST until 2pm"), [(-300, -300)]);
        assert_eq!(
            ranges("anywhere between UTC-3 and UTC+3"),
            [(-180, -180), (180, 180)]
        );
        assert_eq!(ranges("UTC+5:30 preferred"), [(330, 330)]);
        assert_eq!(
            ranges("Operating primarily across Eastern, Pacific and Western European time zones"),
            [(0, 0), (-480, -480), (-300, -300)]
        );
        assert_eq!(ranges("European time zones"), [(-60, 120)]);
        assert_eq!(ranges("must work US hours"), [(-480, -300)]);
        assert_eq!(
            ranges("You can work from most timezones within these regions"),
            Vec::<Offsets>::new()
        );
        assert_eq!(ranges("We use GitHub and Slack"), Vec::<Offsets>::new());
    }

    #[test]
    fn distances() {
        assert_eq!(distance_hours((-480, -480), (-180, -180)), 5.0);
        assert_eq!(distance_hours((-480, -300), (-360, -360)), 0.0);
        assert_eq!(span(&[(-480, -480), (0, 0)]), Some((-480, 0)));
    }
}
