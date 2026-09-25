//! Conversion of HTML fragments (job descriptions, rich-text fields) into
//! readable plain text.
//!
//! Sources publish descriptions written in rich-text editors, so the markup
//! is a small, regular subset of HTML: paragraphs, headings, lists, links,
//! emphasis, line breaks and the odd table. [`html_to_text`] keeps the
//! structure a reader needs (paragraphs, list items, line breaks) and drops
//! everything else (styles, attributes, scripts). It is tolerant of broken
//! markup: anything that does not parse as a tag is kept as text.

use std::borrow::Cow;

use crate::text::clean_block;

/// Elements whose content is never visible text.
const SKIPPED: &[&str] = &["script", "style", "head", "title", "noscript", "template"];

/// Elements that start a new paragraph (blank line before and after).
const PARAGRAPHS: &[&str] = &[
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "dl",
    "table",
    "blockquote",
    "pre",
    "figure",
    "section",
    "article",
    "header",
    "footer",
    "hr",
];

/// Elements that start a new line.
const LINES: &[&str] = &["div", "tr", "dt", "dd", "li", "figcaption", "address"];

/// Converts an HTML fragment into plain text.
///
/// Paragraph-level elements are separated by a blank line, list items become
/// `• item` (or `1. item` in ordered lists) on their own line, `<br>` becomes
/// a line break, and character references are decoded. Whitespace is
/// collapsed as a browser would. Returns `None` when no text remains.
pub fn html_to_text(input: &str) -> Option<String> {
    let mut writer = Writer::default();
    let mut rest = input;
    let mut skipping: Option<String> = None;

    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            if skipping.is_none() {
                writer.text(rest);
            }
            break;
        };
        if lt > 0 && skipping.is_none() {
            writer.text(&rest[..lt]);
        }
        rest = &rest[lt..];

        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").map_or("", |end| &after[end + 3..]);
            continue;
        }
        match parse_tag(rest) {
            Some((tag, consumed)) => {
                rest = &rest[consumed..];
                if let Some(skipped) = &skipping {
                    if tag.closing && tag.name == *skipped {
                        skipping = None;
                    }
                    continue;
                }
                if !tag.closing && !tag.self_closing && SKIPPED.contains(&tag.name.as_str()) {
                    skipping = Some(tag.name);
                    continue;
                }
                writer.tag(&tag);
            }
            None => {
                // A lone '<' ("a < b"): keep it as text.
                if skipping.is_none() {
                    writer.text("<");
                }
                rest = &rest[1..];
            }
        }
    }
    writer.finish()
}

#[derive(Debug)]
struct Tag {
    name: String,
    closing: bool,
    self_closing: bool,
}

/// Parses a tag at the start of `input` (which begins with `<`). Returns the
/// tag and the number of bytes it spans, or `None` if this is not a tag.
fn parse_tag(input: &str) -> Option<(Tag, usize)> {
    let bytes = input.as_bytes();
    let mut i = 1;
    let closing = bytes.get(i) == Some(&b'/');
    if closing {
        i += 1;
    }
    // Declarations and processing instructions (<!DOCTYPE>, <?xml?>).
    if !closing && matches!(bytes.get(i), Some(b'!' | b'?')) {
        let end = input[i..].find('>')?;
        return Some((
            Tag {
                name: String::new(),
                closing: false,
                self_closing: true,
            },
            i + end + 1,
        ));
    }
    let name_start = i;
    while bytes.get(i).is_some_and(u8::is_ascii_alphanumeric) {
        i += 1;
    }
    if i == name_start || !bytes[name_start].is_ascii_alphabetic() {
        return None;
    }
    let name = input[name_start..i].to_ascii_lowercase();

    // Skip attributes, honoring quotes so that '>' inside a value is ignored.
    let mut quote: Option<u8> = None;
    let mut last_significant = b' ';
    while let Some(&b) = bytes.get(i) {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if b == b'>' => {
                return Some((
                    Tag {
                        name,
                        closing,
                        self_closing: last_significant == b'/',
                    },
                    i + 1,
                ));
            }
            None => {}
        }
        if !b.is_ascii_whitespace() {
            last_significant = b;
        }
        i += 1;
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Break {
    Space,
    /// Between table cells.
    Cell,
    Line,
    Paragraph,
}

#[derive(Debug)]
enum ListKind {
    Unordered,
    Ordered(u32),
}

#[derive(Default)]
struct Writer {
    out: String,
    pending: Option<Break>,
    lists: Vec<ListKind>,
    /// A list marker waiting for the list item's first text.
    marker: Option<String>,
    /// Nesting depth of `<pre>`, inside which whitespace is kept.
    pre: usize,
}

impl Writer {
    fn request(&mut self, kind: Break) {
        // Inside a list item, paragraphs only start a new line: `<li><p>..`
        // must not leave blank lines between items.
        let kind = if kind == Break::Paragraph && !self.lists.is_empty() {
            Break::Line
        } else {
            kind
        };
        self.pending = Some(self.pending.map_or(kind, |p| p.max(kind)));
    }

    fn tag(&mut self, tag: &Tag) {
        let name = tag.name.as_str();
        match name {
            "br" => {
                // A forced line break. Pending breaks come first, so a `<br>`
                // alone in a block (`<div><br></div>`) renders as the blank
                // line a browser shows.
                if !self.out.is_empty() {
                    self.emit_pending();
                    self.flush_marker();
                    self.out.push('\n');
                }
                self.pending = None;
                return;
            }
            "ul" | "ol" if !tag.closing => {
                self.request(Break::Paragraph);
                self.lists.push(if name == "ol" {
                    ListKind::Ordered(0)
                } else {
                    ListKind::Unordered
                });
                return;
            }
            "ul" | "ol" => {
                self.lists.pop();
                self.marker = None;
                self.request(Break::Paragraph);
                return;
            }
            "li" if !tag.closing => {
                self.request(Break::Line);
                let depth = self.lists.len().saturating_sub(1);
                let bullet = match self.lists.last_mut() {
                    Some(ListKind::Ordered(n)) => {
                        *n += 1;
                        format!("{n}. ")
                    }
                    _ => "• ".to_owned(),
                };
                self.marker = Some(format!("{}{bullet}", "  ".repeat(depth)));
                return;
            }
            "pre" if !tag.closing => self.pre += 1,
            "pre" => self.pre = self.pre.saturating_sub(1),
            "td" | "th" if !tag.closing => {
                self.request(Break::Cell);
                return;
            }
            _ => {}
        }
        if PARAGRAPHS.contains(&name) {
            self.request(Break::Paragraph);
        } else if LINES.contains(&name) {
            self.request(Break::Line);
        }
    }

    fn text(&mut self, raw: &str) {
        let decoded = decode_entities(raw);
        let blank_line = decoded.contains('\u{a0}')
            && decoded.chars().all(|c| c.is_whitespace() || c == '\u{a0}');
        if blank_line
            && self.pre == 0
            && (self.at_line_start() || self.pending.is_some_and(|p| p >= Break::Line))
        {
            // A line holding only `&nbsp;` (`<div>&nbsp;</div>`) is how rich
            // text editors write an empty paragraph: keep it as a blank line.
            if !self.out.is_empty() {
                self.emit_pending();
                self.out.push('\n');
            }
            return;
        }
        if self.pre > 0 {
            if !decoded.is_empty() {
                self.emit_pending();
                self.out.push_str(&decoded);
            }
            return;
        }
        let words = decoded.split(|c: char| c.is_whitespace() || c == '\u{a0}');
        let starts_with_space = decoded
            .chars()
            .next()
            .is_some_and(|c| c.is_whitespace() || c == '\u{a0}');
        let mut first = true;
        for word in words {
            if word.is_empty() {
                continue;
            }
            if first {
                if starts_with_space {
                    self.request(Break::Space);
                }
                first = false;
            } else {
                self.request(Break::Space);
            }
            self.emit_pending();
            self.flush_marker();
            self.out.push_str(word);
        }
        let ends_with_space = decoded
            .chars()
            .last()
            .is_some_and(|c| c.is_whitespace() || c == '\u{a0}');
        if ends_with_space && !first {
            self.request(Break::Space);
        } else if first && !decoded.is_empty() {
            // Whitespace-only text between inline elements.
            self.request(Break::Space);
        }
    }

    fn at_line_start(&self) -> bool {
        self.out.is_empty() || self.out.ends_with('\n')
    }

    fn emit_pending(&mut self) {
        let Some(kind) = self.pending.take() else {
            return;
        };
        if self.out.is_empty() {
            return;
        }
        match kind {
            Break::Space => {
                if !self.at_line_start() && !self.out.ends_with(' ') {
                    self.out.push(' ');
                }
            }
            Break::Cell => {
                trim_trailing_spaces(&mut self.out);
                if !self.at_line_start() {
                    self.out.push_str(" | ");
                }
            }
            Break::Line => {
                trim_trailing_spaces(&mut self.out);
                if !self.out.ends_with('\n') {
                    self.out.push('\n');
                }
            }
            Break::Paragraph => {
                trim_trailing_spaces(&mut self.out);
                while !self.out.ends_with("\n\n") {
                    self.out.push('\n');
                }
            }
        }
    }

    fn flush_marker(&mut self) {
        if let Some(marker) = self.marker.take() {
            if !self.at_line_start() {
                trim_trailing_spaces(&mut self.out);
                self.out.push('\n');
            }
            self.out.push_str(&marker);
        }
    }

    fn finish(self) -> Option<String> {
        // clean_block trims every line; collapse runs of blank lines too.
        let cleaned = clean_block(&self.out)?;
        let mut out = String::with_capacity(cleaned.len());
        let mut blank_run = 0;
        for line in cleaned.lines() {
            if line.is_empty() {
                blank_run += 1;
                if blank_run > 1 {
                    continue;
                }
            } else {
                blank_run = 0;
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(line);
        }
        Some(out)
    }
}

fn trim_trailing_spaces(out: &mut String) {
    let trimmed = out.trim_end_matches([' ', '\t']).len();
    out.truncate(trimmed);
}

/// Decodes HTML character references: named ones in common use
/// (`&amp;`, `&nbsp;`, `&rsquo;`, `&eacute;`, ...) and numeric ones
/// (`&#39;`, `&#x2014;`). Unknown or malformed references are kept verbatim.
/// Soft hyphens and zero-width spaces are removed.
pub fn decode_entities(input: &str) -> Cow<'_, str> {
    if !input.contains('&') && !input.contains(['\u{ad}', '\u{200b}']) {
        return Cow::Borrowed(input);
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        match decode_reference(rest) {
            Some((decoded, consumed)) => {
                out.push(decoded);
                rest = &rest[consumed..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out.retain(|c| c != '\u{ad}' && c != '\u{200b}');
    Cow::Owned(out)
}

/// Decodes the reference at the start of `input` (which begins with `&`).
fn decode_reference(input: &str) -> Option<(char, usize)> {
    let end = input[1..].find(';').map(|i| i + 1)?;
    // References are short; a far-away ';' means this '&' is plain text.
    if end > 12 {
        return None;
    }
    let body = &input[1..end];
    let decoded = if let Some(num) = body.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse::<u32>().ok()?,
        };
        char::from_u32(code).filter(|c| *c != '\0')?
    } else {
        named_entity(body)?
    };
    Some((decoded, end + 1))
}

fn named_entity(name: &str) -> Option<char> {
    let c = match name {
        "amp" | "AMP" => '&',
        "lt" | "LT" => '<',
        "gt" | "GT" => '>',
        "quot" | "QUOT" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "ensp" | "emsp" | "thinsp" => ' ',
        "shy" => '\u{ad}',
        "zwsp" | "ZeroWidthSpace" => '\u{200b}',
        "ndash" => '–',
        "mdash" => '—',
        "minus" => '−',
        "hyphen" | "dash" => '‐',
        "lsquo" => '‘',
        "rsquo" => '’',
        "sbquo" => '‚',
        "ldquo" => '“',
        "rdquo" => '”',
        "bdquo" => '„',
        "laquo" => '«',
        "raquo" => '»',
        "lsaquo" => '‹',
        "rsaquo" => '›',
        "hellip" => '…',
        "bull" => '•',
        "middot" => '·',
        "prime" => '′',
        "Prime" => '″',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "deg" => '°',
        "times" => '×',
        "divide" => '÷',
        "plusmn" => '±',
        "frac12" => '½',
        "frac14" => '¼',
        "frac34" => '¾',
        "sup1" => '¹',
        "sup2" => '²',
        "sup3" => '³',
        "micro" => 'µ',
        "para" => '¶',
        "sect" => '§',
        "dagger" => '†',
        "Dagger" => '‡',
        "permil" => '‰',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "harr" => '↔',
        "check" | "checkmark" => '✓',
        "star" => '☆',
        "starf" => '★',
        "euro" => '€',
        "pound" => '£',
        "yen" => '¥',
        "cent" => '¢',
        "curren" => '¤',
        "iexcl" => '¡',
        "iquest" => '¿',
        "ordf" => 'ª',
        "ordm" => 'º',
        "szlig" => 'ß',
        "agrave" => 'à',
        "aacute" => 'á',
        "acirc" => 'â',
        "atilde" => 'ã',
        "auml" => 'ä',
        "aring" => 'å',
        "aelig" => 'æ',
        "ccedil" => 'ç',
        "egrave" => 'è',
        "eacute" => 'é',
        "ecirc" => 'ê',
        "euml" => 'ë',
        "igrave" => 'ì',
        "iacute" => 'í',
        "icirc" => 'î',
        "iuml" => 'ï',
        "ntilde" => 'ñ',
        "ograve" => 'ò',
        "oacute" => 'ó',
        "ocirc" => 'ô',
        "otilde" => 'õ',
        "ouml" => 'ö',
        "oslash" => 'ø',
        "oelig" => 'œ',
        "ugrave" => 'ù',
        "uacute" => 'ú',
        "ucirc" => 'û',
        "uuml" => 'ü',
        "yacute" => 'ý',
        "yuml" => 'ÿ',
        "Agrave" => 'À',
        "Aacute" => 'Á',
        "Acirc" => 'Â',
        "Atilde" => 'Ã',
        "Auml" => 'Ä',
        "Aring" => 'Å',
        "AElig" => 'Æ',
        "Ccedil" => 'Ç',
        "Egrave" => 'È',
        "Eacute" => 'É',
        "Ecirc" => 'Ê',
        "Euml" => 'Ë',
        "Igrave" => 'Ì',
        "Iacute" => 'Í',
        "Icirc" => 'Î',
        "Iuml" => 'Ï',
        "Ntilde" => 'Ñ',
        "Ograve" => 'Ò',
        "Oacute" => 'Ó',
        "Ocirc" => 'Ô',
        "Otilde" => 'Õ',
        "Ouml" => 'Ö',
        "Oslash" => 'Ø',
        "OElig" => 'Œ',
        "Ugrave" => 'Ù',
        "Uacute" => 'Ú',
        "Ucirc" => 'Û',
        "Uuml" => 'Ü',
        "Yacute" => 'Ý',
        _ => return None,
    };
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(html: &str) -> String {
        html_to_text(html).unwrap_or_default()
    }

    #[test]
    fn paragraphs_and_headings_are_separated_by_blank_lines() {
        assert_eq!(
            text(
                "<h2><strong>About us</strong></h2><p>We build <em>things</em>.</p><p>Join us.</p>"
            ),
            "About us\n\nWe build things.\n\nJoin us."
        );
    }

    #[test]
    fn lists_become_bullets_without_blank_lines_between_items() {
        assert_eq!(
            text(
                "<p>You will:</p><ul><li>Ship code</li><li><p>Review <b>PRs</b></p></li></ul><p>Thanks</p>"
            ),
            "You will:\n\n• Ship code\n• Review PRs\n\nThanks"
        );
        assert_eq!(
            text("<ol><li>One</li><li>Two<ul><li>Nested</li></ul></li></ol>"),
            "1. One\n2. Two\n  • Nested"
        );
    }

    #[test]
    fn line_breaks_and_whitespace_follow_browser_rules() {
        assert_eq!(text("a<br>b<br/><br />c"), "a\nb\n\nc");
        assert_eq!(text("  lots   of\n\t space  "), "lots of space");
        assert_eq!(
            text("<span>inline</span> <span>words</span>"),
            "inline words"
        );
        assert_eq!(text("x<span> y</span>z"), "x yz");
    }

    #[test]
    fn decodes_entities_and_drops_nbsp_runs() {
        assert_eq!(
            text("<p>R&amp;D&nbsp;&mdash; caf&eacute; &#8212; &#x2019;s&nbsp;&nbsp;</p>"),
            "R&D — café — ’s"
        );
    }

    #[test]
    fn skips_scripts_styles_comments_and_attributes() {
        assert_eq!(
            text(
                "<style>p{color:red}</style><!-- note --><p style=\"a>b\" class='x'>Hi</p><script>alert(1)</script>"
            ),
            "Hi"
        );
    }

    #[test]
    fn tolerates_broken_markup() {
        assert_eq!(text("1 < 2 and <p>ok"), "1 < 2 and\n\nok");
        assert_eq!(text("<p>unterminated <b"), "unterminated <b");
        assert_eq!(text("AT&T &unknown; & more"), "AT&T &unknown; & more");
        assert_eq!(html_to_text("<p> </p><div>&nbsp;</div>"), None);
    }

    #[test]
    fn empty_spacer_blocks_become_blank_lines() {
        assert_eq!(
            text(
                "<div><b>Title</b></div>\n<div>&nbsp;</div>\n<div>Body</div><div><br></div><div>More</div>"
            ),
            "Title\n\nBody\n\nMore"
        );
        assert_eq!(text("<p>A</p><p>&nbsp;</p><p>B</p>"), "A\n\nB");
        assert_eq!(text("a&nbsp;b"), "a b");
    }

    #[test]
    fn tables_keep_cells_apart() {
        assert_eq!(
            text(
                "<table><tr><th>Level</th><th>Pay</th></tr><tr><td>L1</td><td>$1</td></tr></table>"
            ),
            "Level | Pay\nL1 | $1"
        );
    }

    #[test]
    fn decode_entities_is_conservative() {
        assert_eq!(decode_entities("plain"), "plain");
        assert_eq!(decode_entities("&lt;p&gt;x&lt;/p&gt;"), "<p>x</p>");
        assert_eq!(decode_entities("&#0; &#xZZ; &;"), "&#0; &#xZZ; &;");
        assert_eq!(decode_entities("a&shy;b\u{200b}c"), "abc");
    }
}
