//! Reading resume files into lines of text, locally.
//!
//! PDFs are read with `pdf-extract` (pure Rust, no browser, no network,
//! no LLM). Instead of its plain-text output, glyph positions are
//! collected and laid out here: glyphs are grouped into lines by baseline
//! in content-stream order, spaces are inserted from real gaps (so kerning
//! does not split words), wide gaps become column breaks (`\t`, typical of
//! right-aligned dates), and larger vertical gaps become paragraph breaks.
//! Ligatures and other compatibility characters are normalized (NFKC), and
//! lines repeated at the top or bottom of several pages (running headers,
//! "Page 2 of 3") are removed.
//!
//! The PDF library can panic on malformed files; extraction runs on its
//! own thread so a panic becomes an [`ExtractError::Unreadable`].

use std::collections::HashMap;
use std::sync::Once;

use jobhunt_profile::DocumentKind;
use unicode_normalization::UnicodeNormalization;

/// Largest file accepted.
pub const MAX_FILE_BYTES: usize = 20 * 1024 * 1024;

const PDF_THREAD: &str = "jobhunt-pdf-extract";

/// Why a resume could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error("the file is empty")]
    EmptyFile,
    #[error("the file is too large ({size} bytes; the limit is {MAX_FILE_BYTES})")]
    TooLarge { size: usize },
    #[error("the file is not a PDF (it does not start with %PDF)")]
    NotPdf,
    #[error("the PDF could not be read; it may be damaged or truncated ({detail})")]
    Unreadable { detail: String },
    #[error("the PDF is password-protected; export an unprotected copy")]
    Encrypted,
    #[error(
        "no text could be extracted from the PDF ({pages} page(s)); it may be a scanned image. \
         JobHunt does not do OCR: export the resume as a text PDF, or save it as .txt or .md"
    )]
    NoText { pages: usize },
    #[error("the file is not UTF-8 text; save it as UTF-8, or pass a PDF")]
    NotUtf8,
    #[error("{extension} files are not supported; save the resume as PDF, .txt or .md")]
    Unsupported { extension: String },
    #[error("the file contains no text")]
    NoTextInFile,
}

/// One line of a document.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// The line's text. `\t` separates columns (a wide horizontal gap).
    pub text: String,
    /// Font size in points (PDF only).
    pub size: Option<f32>,
    /// A paragraph break (blank line or large vertical gap) precedes it.
    pub gap_before: bool,
    /// Markdown heading level (`## …` is 2); 0 when not a heading.
    pub heading: u8,
    /// Started with a list marker, which was removed from `text`.
    pub bullet: bool,
    /// 1-based page number (1 for text files).
    pub page: usize,
}

/// A document as lines, ready for parsing.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedText {
    pub kind: DocumentKind,
    pub pages: usize,
    pub lines: Vec<Line>,
    /// Repeated header/footer lines that were dropped.
    pub removed: Vec<String>,
}

impl ExtractedText {
    /// The text as stored with the profile: one line per line, a blank line
    /// for paragraph breaks and page boundaries.
    pub fn text(&self) -> String {
        let mut out = String::new();
        let mut page = 1;
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 {
                out.push('\n');
                if line.gap_before || line.page != page {
                    out.push('\n');
                }
            }
            page = line.page;
            if line.bullet {
                out.push_str("• ");
            }
            out.push_str(&line.text);
        }
        out
    }
}

/// Reads a resume from its bytes. `extension` (lowercase, without dot)
/// decides between text and Markdown; PDFs are recognized by content.
pub fn extract(bytes: &[u8], extension: Option<&str>) -> Result<ExtractedText, ExtractError> {
    if bytes.is_empty() {
        return Err(ExtractError::EmptyFile);
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Err(ExtractError::TooLarge { size: bytes.len() });
    }
    let head = &bytes[..bytes.len().min(1024)];
    let is_pdf = head.windows(5).any(|w| w == b"%PDF-");
    match extension {
        _ if is_pdf => extract_pdf(bytes),
        Some("pdf") => Err(ExtractError::NotPdf),
        Some(
            ext @ ("doc" | "docx" | "odt" | "rtf" | "pages" | "png" | "jpg" | "jpeg" | "html"
            | "htm"),
        ) => Err(ExtractError::Unsupported {
            extension: format!(".{ext}"),
        }),
        Some("md" | "markdown") => text_lines(bytes, DocumentKind::Markdown),
        _ => text_lines(bytes, DocumentKind::Text),
    }
}

fn text_lines(bytes: &[u8], kind: DocumentKind) -> Result<ExtractedText, ExtractError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ExtractError::NotUtf8)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = Vec::new();
    let mut gap = false;
    let mut in_code = false;
    for raw in text.lines() {
        let raw: String = raw.nfkc().collect();
        let trimmed = raw.trim();
        if kind == DocumentKind::Markdown && trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if trimmed.is_empty()
            || (kind == DocumentKind::Markdown
                && (trimmed.chars().all(|c| matches!(c, '-' | '*' | '_' | '='))
                    && trimmed.len() >= 3))
        {
            gap = true;
            continue;
        }
        let mut line = Line {
            text: trimmed.to_owned(),
            size: None,
            gap_before: gap,
            heading: 0,
            bullet: false,
            page: 1,
        };
        gap = false;
        if kind == DocumentKind::Markdown && !in_code {
            let hashes = line.text.chars().take_while(|c| *c == '#').count();
            if hashes > 0 && line.text[hashes..].starts_with(' ') {
                line.text = line.text[hashes..].trim().to_owned();
                line.heading = u8::try_from(hashes).unwrap_or(u8::MAX);
            }
            line.text = strip_markdown(&line.text);
        }
        split_bullet(&mut line);
        if !line.text.is_empty() {
            lines.push(line);
        }
    }
    if lines.is_empty() {
        return Err(ExtractError::NoTextInFile);
    }
    Ok(ExtractedText {
        kind,
        pages: 1,
        lines,
        removed: Vec::new(),
    })
}

/// Removes emphasis markers and turns `[text](url)` into `text (url)`.
fn strip_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '['
            && let Some(close) = chars[i..].iter().position(|x| *x == ']').map(|p| p + i)
            && chars.get(close + 1) == Some(&'(')
            && let Some(end) = chars[close..]
                .iter()
                .position(|x| *x == ')')
                .map(|p| p + close)
        {
            let label: String = chars[i + 1..close].iter().collect();
            let url: String = chars[close + 2..end].iter().collect();
            if label.trim() == url.trim() || url.trim().ends_with(label.trim()) {
                out.push_str(url.trim());
            } else {
                out.push_str(&format!("{} ({})", label.trim(), url.trim()));
            }
            i = end + 1;
            continue;
        }
        // Emphasis: ** __ * _ at word boundaries; backticks.
        if c == '`' {
            i += 1;
            continue;
        }
        if c == '*' || c == '_' {
            let prev = i.checked_sub(1).map(|p| chars[p]);
            let next = chars.get(i + 1).copied();
            let boundary = prev.is_none_or(|p| !p.is_alphanumeric())
                || next.is_none_or(|n| !n.is_alphanumeric());
            // Keep a lone leading "* " (a bullet; handled later).
            if boundary && !(i == 0 && next == Some(' ')) {
                i += 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out.trim().to_owned()
}

/// Detects a list marker at the start of a line and removes it.
fn split_bullet(line: &mut Line) {
    let text = line.text.trim_start();
    let mut chars = text.chars();
    let Some(first) = chars.next() else { return };
    let rest = chars.as_str();
    let marker =
        matches!(
            first,
            '•' | '●'
                | '○'
                | '◦'
                | '▪'
                | '■'
                | '□'
                | '‣'
                | '⁃'
                | '∙'
                | '·'
                | '➢'
                | '➤'
                | '►'
                | '▸'
                | '✓'
                | '✔'
        ) || ((first == '-' || first == '*' || first == '+' || first == '–' || first == '—')
            && rest.starts_with(' '));
    if marker {
        line.text = rest.trim().to_owned();
        line.bullet = true;
    }
}

fn quiet_pdf_panics() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if std::thread::current().name() != Some(PDF_THREAD) {
                previous(info);
            }
        }));
    });
}

#[derive(Debug, Clone)]
struct Glyph {
    x: f64,
    y: f64,
    width: f64,
    size: f64,
    text: String,
}

#[derive(Default)]
struct Collector {
    height: f64,
    pages: Vec<Vec<Glyph>>,
}

impl pdf_extract::OutputDev for Collector {
    fn begin_page(
        &mut self,
        _page: u32,
        media_box: &pdf_extract::MediaBox,
        _art_box: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), pdf_extract::OutputError> {
        self.height = media_box.ury - media_box.lly;
        self.pages.push(Vec::new());
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }

    fn output_character(
        &mut self,
        trm: &pdf_extract::Transform,
        width: f64,
        _spacing: f64,
        font_size: f64,
        text: &str,
    ) -> Result<(), pdf_extract::OutputError> {
        let vx = (trm.m11 + trm.m21) * font_size;
        let vy = (trm.m12 + trm.m22) * font_size;
        let size = (vx * vy).abs().sqrt();
        if let Some(page) = self.pages.last_mut()
            && size.is_finite()
            && size > 0.0
        {
            page.push(Glyph {
                x: trm.m31,
                y: self.height - trm.m32,
                width: width * size,
                size,
                text: text.to_owned(),
            });
        }
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }
}

enum PdfFailure {
    Encrypted,
    Other(String),
}

fn extract_pdf(bytes: &[u8]) -> Result<ExtractedText, ExtractError> {
    quiet_pdf_panics();
    let owned = bytes.to_vec();
    let worker = std::thread::Builder::new()
        .name(PDF_THREAD.to_owned())
        .spawn(move || -> Result<Vec<Vec<Glyph>>, PdfFailure> {
            let mut doc = pdf_extract::Document::load_mem(&owned)
                .map_err(|e| PdfFailure::Other(e.to_string()))?;
            if doc.is_encrypted() && doc.decrypt("").is_err() {
                return Err(PdfFailure::Encrypted);
            }
            let mut collector = Collector::default();
            pdf_extract::output_doc(&doc, &mut collector)
                .map_err(|e| PdfFailure::Other(e.to_string()))?;
            Ok(collector.pages)
        })
        .map_err(|e| ExtractError::Unreadable {
            detail: e.to_string(),
        })?;
    let pages = match worker.join() {
        Ok(Ok(pages)) => pages,
        Ok(Err(PdfFailure::Encrypted)) => return Err(ExtractError::Encrypted),
        Ok(Err(PdfFailure::Other(detail))) => return Err(ExtractError::Unreadable { detail }),
        Err(_) => {
            return Err(ExtractError::Unreadable {
                detail: "the PDF reader failed on its structure".to_owned(),
            });
        }
    };
    let page_count = pages.len();
    let mut per_page: Vec<Vec<Line>> = pages
        .iter()
        .enumerate()
        .map(|(i, glyphs)| layout_page(glyphs, i + 1))
        .collect();
    let removed = remove_running_lines(&mut per_page);
    let lines: Vec<Line> = per_page.into_iter().flatten().collect();
    if lines.iter().all(|l| l.text.trim().is_empty()) {
        return Err(ExtractError::NoText { pages: page_count });
    }
    Ok(ExtractedText {
        kind: DocumentKind::Pdf,
        pages: page_count,
        lines,
        removed,
    })
}

/// Normalizes a glyph's text: NFKC (ligatures), and private-use bullet
/// glyphs from symbol fonts become "•".
fn normalize_glyph(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{f0b7}' | '\u{f0a7}' | '\u{f076}' | '\u{f0d8}' | '\u{f0fc}' => '•',
            '\u{a0}' => ' ',
            c => c,
        })
        .collect::<String>()
        .nfkc()
        .collect()
}

struct Run<'a> {
    glyphs: Vec<&'a Glyph>,
    y: f64,
    size: f64,
}

/// Lays out one page's glyphs into lines.
fn layout_page(glyphs: &[Glyph], page: usize) -> Vec<Line> {
    // Generators that write spaces as glyphs (browsers, word processors)
    // also position letters with kerning; there a gap inside a word is not
    // a space unless it is wide. Generators that write no space glyphs
    // (some TeX setups) separate words by gaps only.
    let spaces = glyphs.iter().filter(|g| g.text == " ").count();
    let word_gap = if spaces * 20 >= glyphs.len() {
        0.8
    } else {
        0.2
    };
    // Runs: consecutive glyphs (content-stream order) on one baseline,
    // moving left to right.
    let mut runs: Vec<Run<'_>> = Vec::new();
    for glyph in glyphs {
        if glyph.text.trim().is_empty() && glyph.text != " " {
            continue;
        }
        match runs.last_mut() {
            Some(run)
                if (glyph.y - run.y).abs() < run.size.max(glyph.size) * 0.35
                    && run
                        .glyphs
                        .last()
                        .is_some_and(|last| glyph.x >= last.x - last.size * 0.5) =>
            {
                run.size = run.size.max(glyph.size);
                run.glyphs.push(glyph);
            }
            _ => runs.push(Run {
                glyphs: vec![glyph],
                y: glyph.y,
                size: glyph.size,
            }),
        }
    }
    // Lines: runs sharing a baseline, left to right, top to bottom.
    let mut rows: Vec<Vec<Run<'_>>> = Vec::new();
    for run in runs {
        match rows.iter_mut().find(|row| {
            let (y, size) = (row[0].y, row[0].size);
            (run.y - y).abs() < size.max(run.size) * 0.35
        }) {
            Some(row) => row.push(run),
            None => rows.push(vec![run]),
        }
    }
    rows.sort_by(|a, b| a[0].y.total_cmp(&b[0].y));
    let mut lines = Vec::new();
    let mut previous: Option<(f64, f64)> = None;
    for mut row in rows {
        row.sort_by(|a, b| a.glyphs[0].x.total_cmp(&b.glyphs[0].x));
        let size = row.iter().map(|r| r.size).fold(0.0, f64::max);
        let y = row[0].y;
        let mut text = String::new();
        let mut end: Option<f64> = None;
        for run in &row {
            for glyph in &run.glyphs {
                let piece = normalize_glyph(&glyph.text);
                if let Some(end) = end {
                    let gap = glyph.x - end;
                    let em = glyph.size.max(1.0);
                    if gap > em * 1.5 {
                        let trimmed = text.trim_end().len();
                        text.truncate(trimmed);
                        text.push('\t');
                    } else if gap > em * word_gap
                        && !text.ends_with(' ')
                        && !text.ends_with('\t')
                        && !piece.starts_with(' ')
                    {
                        text.push(' ');
                    }
                }
                if piece == " " && (text.ends_with(' ') || text.ends_with('\t') || text.is_empty())
                {
                    end = Some(glyph.x + glyph.width);
                    continue;
                }
                text.push_str(&piece);
                end = Some(glyph.x + glyph.width);
            }
        }
        let text = collapse_spaces(&text);
        if text.is_empty() {
            continue;
        }
        let gap_before =
            previous.is_some_and(|(prev_y, prev_size)| (y - prev_y) > prev_size.min(size) * 1.55);
        previous = Some((y, size));
        let mut line = Line {
            text,
            size: Some(size as f32),
            gap_before,
            heading: 0,
            bullet: false,
            page,
        };
        split_bullet(&mut line);
        if !line.text.is_empty() {
            lines.push(line);
        }
    }
    lines
}

fn collapse_spaces(text: &str) -> String {
    text.split('\t')
        .map(|part| part.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\t")
}

/// "Page 2 of 3", "2/3", "- 2 -", "2".
fn is_page_number(text: &str) -> bool {
    let lower = text.to_lowercase();
    let stripped: String = lower
        .replace("page", "")
        .replace("pagina", "")
        .replace("página", "")
        .replace("of", " ")
        .replace("de", " ")
        .chars()
        .filter(|c| !matches!(c, '-' | '–' | '—' | '/' | '|' | '.' | '(' | ')'))
        .collect();
    let parts: Vec<&str> = stripped.split_whitespace().collect();
    !parts.is_empty()
        && parts.len() <= 2
        && parts
            .iter()
            .all(|p| p.chars().all(|c| c.is_ascii_digit()) && p.len() <= 3)
}

/// Removes running headers and footers: lines at the top or bottom of a
/// page that repeat (digits ignored) on several pages, and page numbers.
fn remove_running_lines(pages: &mut [Vec<Line>]) -> Vec<String> {
    const EDGE: usize = 2;
    let key = |text: &str| -> String {
        text.to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_digit() { '#' } else { c })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let edges = |page: &Vec<Line>| -> Vec<usize> {
        let n = page.len();
        let mut idx: Vec<usize> = (0..n.min(EDGE)).collect();
        idx.extend(n.saturating_sub(EDGE)..n);
        idx.sort_unstable();
        idx.dedup();
        idx
    };
    let mut counts: HashMap<String, usize> = HashMap::new();
    for page in pages.iter() {
        let mut seen = Vec::new();
        for i in edges(page) {
            let k = key(&page[i].text);
            if !seen.contains(&k) {
                seen.push(k.clone());
                *counts.entry(k).or_insert(0) += 1;
            }
        }
    }
    let multi_page = pages.len() >= 2;
    let repeated = |k: &str| multi_page && counts.get(k).copied().unwrap_or(0) >= 2;
    let mut removed = Vec::new();
    for page in pages.iter_mut() {
        let drop: Vec<usize> = edges(page)
            .into_iter()
            .filter(|i| repeated(&key(&page[*i].text)) || is_page_number(&page[*i].text))
            .collect();
        for i in drop.into_iter().rev() {
            let line = page.remove(i);
            if !removed.contains(&line.text) {
                removed.push(line.text);
            }
        }
        if let Some(first) = page.first_mut() {
            first.gap_before = false;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_lines_lose_their_markup() {
        let md = "# Ana Lima\n\n**Backend Engineer** · [GitHub](https://github.com/ana)\n\n## Experience\n\n- Built *things*\n  with care\n\n---\n";
        let doc = extract(md.as_bytes(), Some("md")).unwrap();
        let texts: Vec<&str> = doc.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Ana Lima",
                "Backend Engineer · GitHub (https://github.com/ana)",
                "Experience",
                "Built things",
                "with care"
            ]
        );
        assert_eq!((doc.lines[0].heading, doc.lines[2].heading), (1, 2));
        assert!(doc.lines[3].bullet && !doc.lines[4].bullet);
        assert!(doc.lines[2].gap_before);
        assert_eq!(doc.kind, DocumentKind::Markdown);
    }

    #[test]
    fn rejects_what_it_cannot_read() {
        assert!(matches!(
            extract(b"", Some("pdf")),
            Err(ExtractError::EmptyFile)
        ));
        assert!(matches!(
            extract(b"hello", Some("pdf")),
            Err(ExtractError::NotPdf)
        ));
        assert!(matches!(
            extract(b"PK\x03\x04", Some("docx")),
            Err(ExtractError::Unsupported { .. })
        ));
        assert!(matches!(
            extract(&[0xff, 0xfe, 0x00], Some("txt")),
            Err(ExtractError::NotUtf8)
        ));
        assert!(matches!(
            extract(b"   \n\n  ", Some("txt")),
            Err(ExtractError::NoTextInFile)
        ));
        let truncated = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n";
        assert!(matches!(
            extract(truncated, Some("pdf")),
            Err(ExtractError::Unreadable { .. })
        ));
    }

    #[test]
    fn recognizes_page_numbers() {
        for text in ["Page 2 of 3", "2/3", "- 2 -", "2", "Page 1  of 2"] {
            assert!(is_page_number(text), "{text}");
        }
        for text in ["2019", "Page Turner Inc", "Software Engineer II"] {
            assert!(!is_page_number(text), "{text}");
        }
    }

    #[test]
    fn running_headers_are_removed_only_when_repeated() {
        let line = |text: &str, page| Line {
            text: text.into(),
            size: None,
            gap_before: false,
            heading: 0,
            bullet: false,
            page,
        };
        let mut pages = vec![
            vec![
                line("Ana Lima · Resume", 1),
                line("Ana Lima", 1),
                line("Body one", 1),
                line("Page 1 of 2", 1),
            ],
            vec![
                line("Ana Lima · Resume", 2),
                line("Body two", 2),
                line("Page 2 of 2", 2),
            ],
        ];
        let removed = remove_running_lines(&mut pages);
        assert_eq!(removed, ["Page 1 of 2", "Ana Lima · Resume", "Page 2 of 2"]);
        let texts: Vec<Vec<&str>> = pages
            .iter()
            .map(|p| p.iter().map(|l| l.text.as_str()).collect())
            .collect();
        assert_eq!(texts, [vec!["Ana Lima", "Body one"], vec!["Body two"]]);
    }
}
