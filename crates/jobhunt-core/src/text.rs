//! Conservative text cleanup for values coming from external sources.

/// Trims a single-line value and collapses internal runs of whitespace into a
/// single space. Returns `None` when nothing meaningful remains, so that empty
/// source values stay "unknown" instead of becoming empty strings.
pub fn clean_line(value: &str) -> Option<String> {
    let mut out = String::with_capacity(value.len());
    for word in value.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    (!out.is_empty()).then_some(out)
}

/// [`clean_line`] for optional values.
pub fn clean_line_opt(value: Option<&str>) -> Option<String> {
    value.and_then(clean_line)
}

/// Trims a multi-line value (such as a description), normalizing line endings
/// and stripping trailing whitespace from each line while keeping paragraph
/// structure. Returns `None` when the value is blank.
pub fn clean_block(value: &str) -> Option<String> {
    let normalized = value.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.lines().map(str::trim_end).collect();
    let joined = lines.join("\n");
    let trimmed = joined.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// [`clean_block`] for optional values.
pub fn clean_block_opt(value: Option<&str>) -> Option<String> {
    value.and_then(clean_block)
}

/// Normalizes text for word-based matching: lowercases (Unicode-aware) and
/// turns every run of non-alphanumeric characters into a single space.
///
/// `"Senior Engineer (Rust/Go)"` becomes `"senior engineer rust go"`. Two
/// values normalized this way can be compared with plain substring checks
/// in any storage backend.
pub fn search_key(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut pending_space = false;
    for c in value.chars() {
        if c.is_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.extend(c.to_lowercase());
        } else {
            pending_space = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_line_collapses_whitespace() {
        assert_eq!(
            clean_line("  Security\tEngineer,\n Cloud "),
            Some("Security Engineer, Cloud".to_owned())
        );
    }

    #[test]
    fn blank_values_become_unknown() {
        assert_eq!(clean_line("   \t\n"), None);
        assert_eq!(clean_line_opt(None), None);
        assert_eq!(clean_block(" \r\n "), None);
    }

    #[test]
    fn clean_block_keeps_paragraphs() {
        assert_eq!(
            clean_block("\n  First line  \r\n\r\nSecond   \n"),
            Some("First line\n\nSecond".to_owned())
        );
    }

    #[test]
    fn search_key_normalizes_words() {
        assert_eq!(
            search_key("Senior Engineer (Rust/Go)"),
            "senior engineer rust go"
        );
        assert_eq!(search_key("  Trust & Safety "), "trust safety");
        assert_eq!(search_key("MÜNCHEN, Deutschland"), "münchen deutschland");
        assert_eq!(search_key("--"), "");
    }
}
