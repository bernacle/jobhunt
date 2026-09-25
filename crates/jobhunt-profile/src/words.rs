//! Word-level matching used by the vocabularies and parsers.
//!
//! Text is split into [`Word`]s: runs of letters and digits, keeping `+`,
//! `#` and inner `.` so that `C++`, `C#`, `Node.js` and `.NET` survive.
//! Hyphens, slashes, apostrophes and other punctuation separate words.
//! Patterns are space-separated word patterns:
//!
//! * `payment*` matches any word starting with `payment`;
//! * `_` matches any single word;
//! * `=Go` matches the word `Go` with exactly that case (for technology
//!   names that are also English words);
//! * anything else matches one word, ignoring case.

use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    /// Lowercased.
    pub lower: String,
    /// As written.
    pub original: String,
    /// Byte range in the source text.
    pub span: Range<usize>,
}

/// Splits `text` into words.
pub fn words(text: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let is_word_char = |i: usize| -> bool {
        let (_, c) = chars[i];
        if c.is_alphanumeric() || c == '+' || c == '#' {
            return true;
        }
        // A dot joins two word characters ("node.js") or leads one (".net").
        c == '.'
            && chars
                .get(i + 1)
                .is_some_and(|(_, next)| next.is_alphanumeric())
            && (i == 0
                || chars[i - 1].1.is_alphanumeric()
                || chars[i - 1].1.is_whitespace()
                || chars[i - 1].1 == '(')
    };
    for (i, &(offset, _)) in chars.iter().enumerate() {
        if is_word_char(i) {
            if start.is_none() {
                start = Some(offset);
            }
        } else if let Some(s) = start.take() {
            push_word(&mut out, text, s, offset);
        }
    }
    if let Some(s) = start {
        push_word(&mut out, text, s, text.len());
    }
    out
}

fn push_word(out: &mut Vec<Word>, text: &str, start: usize, end: usize) {
    let original = &text[start..end];
    // "+" alone (as in "30+") is not a word.
    if original.chars().all(|c| c == '+' || c == '#' || c == '.') {
        return;
    }
    out.push(Word {
        lower: original.to_lowercase(),
        original: original.to_owned(),
        span: start..end,
    });
}

/// A compiled pattern.
#[derive(Debug, Clone)]
pub struct Pattern {
    parts: Vec<Part>,
}

#[derive(Debug, Clone)]
enum Part {
    Exact(String),
    Prefix(String),
    CaseSensitive(String),
    Any,
}

impl Pattern {
    pub fn new(pattern: &str) -> Self {
        let parts = pattern
            .split_whitespace()
            .map(|p| {
                if p == "_" {
                    Part::Any
                } else if let Some(rest) = p.strip_prefix('=') {
                    Part::CaseSensitive(rest.to_owned())
                } else if let Some(rest) = p.strip_suffix('*') {
                    Part::Prefix(rest.to_lowercase())
                } else {
                    Part::Exact(p.to_lowercase())
                }
            })
            .collect();
        Self { parts }
    }

    pub fn len(&self) -> usize {
        self.parts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    fn matches_at(&self, words: &[Word], at: usize) -> bool {
        if self.parts.is_empty() || at + self.parts.len() > words.len() {
            return false;
        }
        self.parts
            .iter()
            .zip(&words[at..])
            .all(|(part, word)| match part {
                Part::Exact(p) => word.lower == *p,
                Part::Prefix(p) => word.lower.starts_with(p.as_str()),
                Part::CaseSensitive(p) => word.original == *p,
                Part::Any => true,
            })
    }

    /// Index ranges (in words) of every match, left to right, without
    /// overlaps.
    pub fn find_all(&self, words: &[Word]) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < words.len() {
            if self.matches_at(words, i) {
                out.push(i..i + self.parts.len());
                i += self.parts.len();
            } else {
                i += 1;
            }
        }
        out
    }

    pub fn find(&self, words: &[Word]) -> Option<Range<usize>> {
        (0..words.len())
            .find(|i| self.matches_at(words, *i))
            .map(|i| i..i + self.parts.len())
    }
}

/// The text covered by words `range` of `text`.
pub fn span_text<'a>(text: &'a str, words: &[Word], range: &Range<usize>) -> &'a str {
    match (words.get(range.start), words.get(range.end.wrapping_sub(1))) {
        (Some(first), Some(last)) => &text[first.span.start..last.span.end],
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lowers(text: &str) -> Vec<String> {
        words(text).into_iter().map(|w| w.lower).collect()
    }

    #[test]
    fn keeps_technology_punctuation() {
        assert_eq!(
            lowers("Node.js, C++ and C# on .NET; CI/CD (30+ repos)."),
            [
                "node.js", "c++", "and", "c#", "on", ".net", "ci", "cd", "30+", "repos"
            ]
        );
        assert_eq!(
            lowers("don't multi-tenant"),
            ["don", "t", "multi", "tenant"]
        );
    }

    #[test]
    fn patterns_match_words_not_substrings() {
        let w = words("Integrated four payment providers behind one Go service");
        assert!(Pattern::new("payment* provider*").find(&w).is_some());
        assert!(Pattern::new("four _ providers").find(&w).is_some());
        assert!(Pattern::new("=Go").find(&w).is_some());
        assert!(Pattern::new("=Go").find(&words("go to market")).is_none());
        assert!(Pattern::new("pay").find(&w).is_none());
        let r = Pattern::new("payment providers").find(&w).unwrap();
        assert_eq!(
            span_text("Integrated four payment providers behind", &w, &r),
            "payment providers"
        );
    }
}
