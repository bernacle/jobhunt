//! Finding date ranges in resume lines: `Mar 2022 – Present`,
//! `2014 - 2018`, `01/2020 to 04/2022`, `Since 2021`, `2019`.

use std::ops::Range;

use jobhunt_profile::PartialDate;
use jobhunt_profile::date::month_from_name;

/// A date range found in a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateRange {
    pub start: Option<PartialDate>,
    pub end: Option<PartialDate>,
    /// Ends in "Present" (or "since …").
    pub current: bool,
    /// Only one date was written (`2019`), stored in `start` and `end`.
    pub single: bool,
    /// Byte range of the dates in the line.
    pub span: Range<usize>,
}

#[derive(Debug, Clone)]
struct Token {
    text: String,
    lower: String,
    span: Range<usize>,
}

/// Words, numbers (with inner `/`, `.` or `-` between digits, as in
/// `03/2020` or `2020-03`) and single dash characters.
fn tokens(line: &str) -> Vec<Token> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let end_of = |i: usize| chars.get(i).map_or(line.len(), |(b, _)| *b);
    while i < chars.len() {
        let (start, c) = chars[i];
        if c.is_alphanumeric() {
            let mut j = i + 1;
            while j < chars.len() {
                let ch = chars[j].1;
                let joins_digits = matches!(ch, '/' | '.' | '-')
                    && chars[j - 1].1.is_ascii_digit()
                    && chars.get(j + 1).is_some_and(|(_, n)| n.is_ascii_digit())
                    // "2014-2018" is two years, not one token.
                    && !(ch == '-' && is_year_ahead(&chars, j + 1) && digits_before(&chars, j) == 4);
                if chars[j].1.is_alphanumeric() || joins_digits {
                    j += 1;
                } else {
                    break;
                }
            }
            let text = line[start..end_of(j)].to_owned();
            out.push(Token {
                lower: text.to_lowercase(),
                text,
                span: start..end_of(j),
            });
            i = j;
        } else if matches!(c, '-' | '–' | '—' | '~' | '→') {
            out.push(Token {
                text: c.to_string(),
                lower: c.to_string(),
                span: start..end_of(i + 1),
            });
            i += 1;
        } else {
            i += 1;
        }
    }
    out
}

fn digits_before(chars: &[(usize, char)], at: usize) -> usize {
    chars[..at]
        .iter()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit())
        .count()
}

fn is_year_ahead(chars: &[(usize, char)], at: usize) -> bool {
    let digits = chars[at..]
        .iter()
        .take_while(|(_, c)| c.is_ascii_digit())
        .count();
    digits == 4
}

fn year(text: &str) -> Option<i32> {
    (text.len() == 4 && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
        .filter(|y| (1950..=2100).contains(y))
}

/// Parses a date starting at token `i`; returns it and the tokens used.
fn date_at(tokens: &[Token], i: usize) -> Option<(PartialDate, usize)> {
    let t = tokens.get(i)?;
    if let Some(y) = year(&t.text) {
        return Some((PartialDate::new(y, None)?, 1));
    }
    if let Some(month) = month_from_name(&t.lower)
        && let Some(next) = tokens.get(i + 1)
        && let Some(y) = year(&next.text)
    {
        return Some((PartialDate::new(y, Some(month))?, 2));
    }
    // Seasons and quarters give the year only.
    if matches!(
        t.lower.as_str(),
        "spring" | "summer" | "fall" | "autumn" | "winter" | "q1" | "q2" | "q3" | "q4"
    ) && let Some(next) = tokens.get(i + 1)
        && let Some(y) = year(&next.text)
    {
        return Some((PartialDate::new(y, None)?, 2));
    }
    if t.text.contains(['/', '.', '-'])
        && let Ok(date) = t.text.parse::<PartialDate>()
        && date.month().is_some()
        && (1950..=2100).contains(&date.year())
    {
        return Some((date, 1));
    }
    None
}

const PRESENT: [&str; 12] = [
    "present",
    "current",
    "currently",
    "now",
    "today",
    "ongoing",
    "atual",
    "atualmente",
    "presente",
    "hoje",
    "actualidad",
    "actual",
];
const RANGE_WORDS: [&str; 8] = [
    "to", "until", "till", "through", "thru", "até", "a", "hasta",
];

fn is_separator(t: &Token) -> bool {
    matches!(t.lower.as_str(), "-" | "–" | "—" | "~" | "→")
        || RANGE_WORDS.contains(&t.lower.as_str())
}

/// The first date range in `line`, or else the last single date.
pub fn find(line: &str) -> Option<DateRange> {
    let toks = tokens(line);
    let mut single: Option<DateRange> = None;
    let mut i = 0;
    while i < toks.len() {
        // "Since 2021" / "From 2021 onwards"
        let since = matches!(toks[i].lower.as_str(), "since" | "desde");
        let lead = usize::from(since || toks[i].lower == "from");
        if let Some((start, used)) = date_at(&toks, i + lead) {
            let after = i + lead + used;
            let span_start = toks[i].span.start;
            // Separator, then an end date or "present".
            if let Some(sep) = toks.get(after).filter(|t| is_separator(t)) {
                let _ = sep;
                let mut j = after + 1;
                if toks.get(j).is_some_and(|t| t.lower == "the") {
                    j += 1;
                }
                if let Some(t) = toks.get(j)
                    && PRESENT.contains(&t.lower.as_str())
                {
                    return Some(DateRange {
                        start: Some(start),
                        end: None,
                        current: true,
                        single: false,
                        span: span_start..t.span.end,
                    });
                }
                if let Some((end, used_end)) = date_at(&toks, j) {
                    let end_tok = &toks[j + used_end - 1];
                    return Some(DateRange {
                        start: Some(start),
                        end: Some(end),
                        current: false,
                        single: false,
                        span: span_start..end_tok.span.end,
                    });
                }
            }
            let last = &toks[after - 1];
            if since {
                return Some(DateRange {
                    start: Some(start),
                    end: None,
                    current: true,
                    single: false,
                    span: span_start..last.span.end,
                });
            }
            single = Some(DateRange {
                start: Some(start),
                end: Some(start),
                current: false,
                single: true,
                span: span_start..last.span.end,
            });
            i = after;
            continue;
        }
        i += 1;
    }
    single
}

/// `line` without the dates in `span`, and without separators left
/// dangling around them.
pub fn remove(line: &str, span: &Range<usize>) -> String {
    let mut out = String::new();
    out.push_str(&line[..span.start]);
    out.push(' ');
    out.push_str(&line[span.end..]);
    let trimmed = out
        .trim()
        .trim_matches(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    '|' | '·' | '•' | ',' | '-' | '–' | '—' | '(' | ')' | '\t'
                )
        })
        .to_owned();
    // "Acme (  )" leftovers.
    trimmed.replace("()", "").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Option<PartialDate> {
        Some(s.parse().unwrap())
    }

    #[test]
    fn finds_ranges() {
        let r = find("Ledgerly — Senior Software Engineer\tMar 2022 – Present").unwrap();
        assert_eq!(
            (r.start, r.end, r.current, r.single),
            (d("2022-03"), None, true, false)
        );
        assert_eq!(
            remove(
                "Ledgerly — Senior Software Engineer\tMar 2022 – Present",
                &r.span
            ),
            "Ledgerly — Senior Software Engineer"
        );

        let r = find("Universidade de São Paulo — B.Sc. 2014 - 2018").unwrap();
        assert_eq!((r.start, r.end), (d("2014"), d("2018")));
        let r = find("2014-2018").unwrap();
        assert_eq!((r.start, r.end), (d("2014"), d("2018")));
        let r = find("Engineer (01/2020 to 04/2022)").unwrap();
        assert_eq!((r.start, r.end), (d("2020-01"), d("2022-04")));
        assert_eq!(remove("Engineer (01/2020 to 04/2022)", &r.span), "Engineer");
        let r = find("Jan. 2019 — Sept 2020").unwrap();
        assert_eq!((r.start, r.end), (d("2019-01"), d("2020-09")));
        let r = find("2020-03 – now").unwrap();
        assert_eq!((r.start, r.current), (d("2020-03"), true));
        let r = find("Since 2021").unwrap();
        assert_eq!((r.start, r.current), (d("2021"), true));
        let r = find("Budgetly — personal finance app\t2019").unwrap();
        assert!(r.single);
        assert_eq!((r.start, r.end), (d("2019"), d("2019")));
        let r = find("Fev 2020 - Atual").unwrap();
        assert_eq!((r.start, r.current), (d("2020-02"), true));
    }

    #[test]
    fn ignores_numbers_that_are_not_dates() {
        assert_eq!(
            find("Handled 2 million transactions with 99.99% availability"),
            None
        );
        assert_eq!(find("1,200 GitHub stars"), None);
        assert_eq!(find("Cut latency from 800 ms to 120 ms"), None);
        assert_eq!(find("Series C, 250 people"), None);
    }
}
