//! The deterministic resume parser.
//!
//! It turns [`ExtractedText`] into a [`ParsedResume`] with rules that are
//! easy to follow and never invent anything:
//!
//! 1. **Sections** start at known headings ("Experience", "Work History",
//!    "Projects", "Education", "Skills", "Languages", "Summary",
//!    "Certifications", ...; English and Portuguese), matched on the whole
//!    line. Markdown `#`/`##` headings and larger all-caps lines also start
//!    sections. Everything before the first heading is the header.
//! 2. **Header**: the name (the largest of the first lines, or the first
//!    line), a headline, contact details (emails, phones, LinkedIn, GitHub,
//!    other URLs) and a location ("City, Country").
//! 3. **Entries** (experience, projects, education) are split at header
//!    lines: lines with a date range, a column gap, or a separator
//!    (`—`, `|`, `·`, " at ") that are short and do not end a sentence.
//!    A company line without a title followed by several titled roles is
//!    one company with several positions.
//! 4. **Titles and companies** are told apart by title words (engineer,
//!    developer, manager, founder, ...). When neither side has one, the
//!    company is assumed to come first and the entry is flagged ambiguous.
//! 5. **Bullets** are list items when the document marks them; otherwise
//!    sentences, rebuilt from wrapped lines (a line that ends a sentence, or
//!    is clearly shorter than a full line, ends an item).
//! 6. **Technologies** come from "Tech:"/"Stack:" lines; the domain layer
//!    also finds them in bullets.
//!
//! Whatever is not understood is reported in [`ParsedResume::ignored`] or
//! as a note, never silently dropped.

use jobhunt_core::text::search_key;
use jobhunt_profile::{
    Contact, ContactKind, EmploymentKind, ParsedEducation, ParsedExperience, ParsedOther,
    ParsedProject, ParsedResume, ParsedSkillLine, SpokenLanguage,
};

use crate::dates::{self, DateRange};
use crate::extract::{ExtractedText, Line};

/// Turns extracted text into a structured resume. Implementations must
/// keep the document's own words in every text field.
pub trait ResumeParser: Send + Sync {
    /// Recorded with the imported document (`deterministic/1`).
    fn name(&self) -> &str;
    fn parse(&self, text: &ExtractedText) -> ParsedResume;
}

/// The built-in rule-based parser. Needs no network and no API key.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeterministicParser;

impl ResumeParser for DeterministicParser {
    fn name(&self) -> &str {
        "deterministic/1"
    }

    fn parse(&self, text: &ExtractedText) -> ParsedResume {
        Parser::new(text).run()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Section {
    Summary,
    Experience,
    Projects,
    Education,
    Skills,
    Languages,
    /// Kept as claims (certifications, awards, ...), with the heading.
    Other(String),
    /// Not used (interests, references).
    Ignored(String),
}

const HEADINGS: &[(&str, Section)] = &[
    ("summary", Section::Summary),
    ("professional summary", Section::Summary),
    ("career summary", Section::Summary),
    ("profile", Section::Summary),
    ("professional profile", Section::Summary),
    ("about", Section::Summary),
    ("about me", Section::Summary),
    ("objective", Section::Summary),
    ("overview", Section::Summary),
    ("resumo", Section::Summary),
    ("perfil", Section::Summary),
    ("sobre", Section::Summary),
    ("sobre mim", Section::Summary),
    ("experience", Section::Experience),
    ("work experience", Section::Experience),
    ("professional experience", Section::Experience),
    ("relevant experience", Section::Experience),
    ("employment", Section::Experience),
    ("employment history", Section::Experience),
    ("work history", Section::Experience),
    ("career history", Section::Experience),
    ("experiência", Section::Experience),
    ("experiência profissional", Section::Experience),
    ("experiencia", Section::Experience),
    ("experiencia profesional", Section::Experience),
    ("projects", Section::Projects),
    ("selected projects", Section::Projects),
    ("personal projects", Section::Projects),
    ("side projects", Section::Projects),
    ("open source", Section::Projects),
    ("open source projects", Section::Projects),
    ("projetos", Section::Projects),
    ("education", Section::Education),
    ("academic background", Section::Education),
    ("formação", Section::Education),
    ("formação acadêmica", Section::Education),
    ("educação", Section::Education),
    ("educación", Section::Education),
    ("skills", Section::Skills),
    ("technical skills", Section::Skills),
    ("core skills", Section::Skills),
    ("key skills", Section::Skills),
    ("skills tools", Section::Skills),
    ("skills and tools", Section::Skills),
    ("technologies", Section::Skills),
    ("tech stack", Section::Skills),
    ("tools", Section::Skills),
    ("competencies", Section::Skills),
    ("habilidades", Section::Skills),
    ("competências", Section::Skills),
    ("languages", Section::Languages),
    ("spoken languages", Section::Languages),
    ("idiomas", Section::Languages),
];

const OTHER_HEADINGS: &[&str] = &[
    "certifications",
    "certificates",
    "certification",
    "awards",
    "honors",
    "honors and awards",
    "achievements",
    "publications",
    "talks",
    "speaking",
    "volunteering",
    "volunteer experience",
    "courses",
    "certificações",
    "prêmios",
];

const IGNORED_HEADINGS: &[&str] = &[
    "interests",
    "hobbies",
    "references",
    "interesses",
    "referências",
];

const TITLE_WORDS: &[&str] = &[
    "engineer",
    "developer",
    "programmer",
    "architect",
    "manager",
    "lead",
    "founder",
    "cofounder",
    "cto",
    "ceo",
    "coo",
    "cpo",
    "vp",
    "director",
    "head",
    "intern",
    "consultant",
    "scientist",
    "designer",
    "analyst",
    "specialist",
    "administrator",
    "sre",
    "devops",
    "researcher",
    "officer",
    "owner",
    "principal",
    "tester",
    "qa",
    "technician",
    "trainee",
    "apprentice",
    "fellow",
    "freelancer",
    "contractor",
    "engenheiro",
    "engenheira",
    "desenvolvedor",
    "desenvolvedora",
    "estagiário",
    "estagiária",
    "analista",
    "gerente",
    "arquiteto",
    "coordinator",
    "associate",
    "staff",
];

const PROJECT_ROLE_WORDS: &[&str] = &[
    "author",
    "creator",
    "co-creator",
    "maintainer",
    "contributor",
    "core contributor",
];

const DEGREE_WORDS: &[&str] = &[
    "bsc",
    "b sc",
    "bs",
    "b s",
    "ba",
    "b a",
    "bachelor",
    "bachelors",
    "bachelor s",
    "msc",
    "m sc",
    "ms",
    "m s",
    "ma",
    "m a",
    "mba",
    "master",
    "masters",
    "master s",
    "phd",
    "ph d",
    "doctorate",
    "associate",
    "diploma",
    "degree",
    "licenciatura",
    "bacharelado",
    "tecnólogo",
    "tecnologo",
    "mestrado",
    "doutorado",
    "beng",
    "b eng",
    "meng",
    "m eng",
    "btech",
    "b tech",
    "mtech",
    "certificate",
    "bootcamp",
];

const INSTITUTION_WORDS: &[&str] = &[
    "university",
    "universidade",
    "universidad",
    "università",
    "universität",
    "college",
    "institute",
    "instituto",
    "school",
    "escola",
    "faculdade",
    "faculty",
    "academy",
    "polytechnic",
    "politécnico",
    "politecnico",
    "bootcamp",
    "unicamp",
    "usp",
    "mit",
];

const TECH_LABELS: &[&str] = &[
    "tech",
    "technologies",
    "technology",
    "tech stack",
    "stack",
    "tools",
    "environment",
    "built with",
    "skills used",
    "key technologies",
    "tecnologias",
];

const PRESENT_WORK_MODES: &[&str] = &[
    "remote",
    "hybrid",
    "on site",
    "onsite",
    "in office",
    "remoto",
    "híbrido",
];

#[derive(Debug, Clone)]
struct L {
    text: String,
    bullet: bool,
    gap: bool,
    heading: u8,
    size: Option<f32>,
}

impl L {
    fn words(&self) -> usize {
        self.text.split_whitespace().count()
    }
}

struct Parser {
    lines: Vec<L>,
    body_size: Option<f32>,
    /// Typical length of a full line, for rebuilding wrapped sentences.
    full_line: usize,
    out: ParsedResume,
}

fn heading_key(text: &str) -> String {
    search_key(text.trim().trim_end_matches(':'))
}

fn words_lower(text: &str) -> Vec<String> {
    search_key(text)
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

fn has_word(text: &str, list: &[&str]) -> bool {
    let key = format!(" {} ", search_key(text));
    list.iter()
        .any(|w| key.contains(&format!(" {} ", search_key(w))))
}

fn title_like(text: &str) -> bool {
    has_word(text, TITLE_WORDS)
        || words_lower(text)
            .iter()
            .any(|w| w.ends_with("engineer") || w.ends_with("developer"))
}

fn snippet(text: &str) -> String {
    text.replace('\t', "  ")
}

fn is_work_mode(text: &str) -> bool {
    PRESENT_WORK_MODES.contains(&search_key(text).as_str())
}

/// "São Paulo, Brazil", "Lisbon, PT", "Remote".
fn location_like(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() || t.chars().any(|c| c.is_ascii_digit()) || title_like(t) {
        return false;
    }
    if is_work_mode(t) {
        return true;
    }
    let parts: Vec<&str> = t.split(',').map(str::trim).collect();
    let org_suffix = [
        "inc", "llc", "ltd", "gmbh", "sa", "s a", "ltda", "corp", "co",
    ];
    parts.len() >= 2
        && parts.len() <= 3
        && parts.iter().all(|p| {
            let n = p.split_whitespace().count();
            (1..=3).contains(&n) && p.chars().next().is_some_and(char::is_uppercase)
        })
        && !org_suffix.contains(&search_key(parts[parts.len() - 1]).as_str())
}

fn employment_of(text: &str) -> Option<EmploymentKind> {
    EmploymentKind::recognize(text.trim().trim_matches(|c| c == '(' || c == ')'))
}

/// Splits a header line into fragments at column gaps and separators.
fn fragments(line: &str) -> Vec<String> {
    let mut parts: Vec<String> = vec![line.to_owned()];
    for sep in ["\t", " — ", " – ", " | ", " · ", " • ", " - ", " @ ", " ― "] {
        parts = parts
            .iter()
            .flat_map(|p| p.split(sep).map(str::to_owned).collect::<Vec<_>>())
            .collect();
    }
    parts
        .into_iter()
        .map(|p| {
            p.trim()
                .trim_matches(|c| c == ',' || c == '|')
                .trim()
                .to_owned()
        })
        .filter(|p| !p.is_empty())
        .collect()
}

fn label_of(text: &str) -> Option<(String, String)> {
    let (label, rest) = text.split_once(':')?;
    let key = search_key(label);
    let rest = rest.trim();
    (TECH_LABELS.contains(&key.as_str()) && !rest.is_empty())
        .then(|| (label.trim().to_owned(), rest.to_owned()))
}

/// Splits a list ("Rust, TypeScript; Go · SQL") into items.
fn list_items(text: &str) -> Vec<String> {
    text.split([',', ';', '|', '•', '·'])
        .map(|item| {
            // "Rust (advanced)" → "Rust"
            let item = match item.find('(') {
                Some(at) if item.trim_end().ends_with(')') => &item[..at],
                _ => item,
            };
            item.trim()
                .trim_end_matches('.')
                .trim_start_matches(['-', '*'])
                .trim()
                .to_owned()
        })
        .filter(|item| !item.is_empty())
        .collect()
}

/// Joins a wrapped line onto the text before it.
fn join_wrapped(text: &mut String, next: &str) {
    let hyphenated = text.ends_with('-')
        && text.chars().rev().nth(1).is_some_and(char::is_alphabetic)
        && next.chars().next().is_some_and(char::is_lowercase);
    if !hyphenated && !text.is_empty() {
        text.push(' ');
    }
    text.push_str(next.trim());
}

fn ends_sentence(text: &str) -> bool {
    text.trim_end().ends_with(['.', '!', '?', ';'])
}

impl Parser {
    fn new(text: &ExtractedText) -> Self {
        let lines: Vec<L> = text.lines.iter().map(from_line).collect();
        let mut sizes: Vec<i64> = lines
            .iter()
            .filter_map(|l| l.size.map(|s| (s * 2.0).round() as i64))
            .collect();
        sizes.sort_unstable();
        let body_size = mode(&sizes).map(|s| s as f32 / 2.0);
        let mut lengths: Vec<usize> = lines.iter().map(|l| l.text.chars().count()).collect();
        lengths.sort_unstable();
        let full_line = lengths
            .get(lengths.len().saturating_sub(1) * 9 / 10)
            .copied()
            .unwrap_or(80)
            .max(40);
        Self {
            lines,
            body_size,
            full_line,
            out: ParsedResume::default(),
        }
    }

    fn section_of(&self, index: usize) -> Option<Section> {
        let line = &self.lines[index];
        if line.bullet || line.words() > 5 || line.text.contains('\t') {
            return None;
        }
        let key = heading_key(&line.text);
        if key.is_empty() {
            return None;
        }
        if let Some((_, section)) = HEADINGS.iter().find(|(k, _)| search_key(k) == key) {
            return Some(section.clone());
        }
        let name = line.text.trim().trim_end_matches(':').to_owned();
        if OTHER_HEADINGS.iter().any(|k| search_key(k) == key) {
            return Some(Section::Other(name));
        }
        if IGNORED_HEADINGS.iter().any(|k| search_key(k) == key) {
            return Some(Section::Ignored(name));
        }
        // An unknown level-1/2 Markdown heading after the first line, or a
        // larger all-caps line, starts a section we do not interpret.
        let caps = line.text.chars().any(char::is_alphabetic)
            && !line
                .text
                .chars()
                .any(|c| c.is_lowercase() || c.is_ascii_digit());
        let larger = matches!((line.size, self.body_size), (Some(s), Some(b)) if s > b * 1.08);
        if (line.heading == 1 || line.heading == 2) && index > 0
            || (caps && larger && line.words() <= 4)
        {
            return Some(Section::Other(name));
        }
        None
    }

    fn run(mut self) -> ParsedResume {
        let mut sections: Vec<(Section, Vec<L>)> = Vec::new();
        let mut header: Vec<L> = Vec::new();
        for i in 0..self.lines.len() {
            if let Some(section) = self.section_of(i) {
                sections.push((section, Vec::new()));
                continue;
            }
            let line = self.lines[i].clone();
            match sections.last_mut() {
                Some((_, lines)) => lines.push(line),
                None => header.push(line),
            }
        }
        self.parse_header(&header);
        let mut saw_experience = false;
        for (section, lines) in sections {
            match section {
                Section::Summary => {
                    let text = paragraphs(&lines);
                    if !text.is_empty() {
                        self.out.basics.summary = Some(text);
                    }
                }
                Section::Experience => {
                    saw_experience = true;
                    self.parse_experience(&lines);
                }
                Section::Projects => self.parse_projects(&lines),
                Section::Education => self.parse_education(&lines),
                Section::Skills => self.parse_skills(&lines),
                Section::Languages => self.parse_languages(&lines),
                Section::Other(name) => {
                    for item in self.items(&lines).bullets {
                        self.out.other.push(ParsedOther {
                            section: name.clone(),
                            line: item,
                        });
                    }
                }
                Section::Ignored(name) => {
                    for line in lines {
                        self.out
                            .ignored
                            .push(format!("{name}: {}", snippet(&line.text)));
                    }
                }
            }
        }
        if !saw_experience {
            self.out
                .notes
                .push("no experience section was found (looked for headings such as \"Experience\" or \"Work History\")".into());
        } else if self.out.experiences.is_empty() {
            self.out
                .notes
                .push("the experience section had no entries that could be read".into());
        }
        self.out
    }

    fn parse_header(&mut self, lines: &[L]) {
        if lines.is_empty() {
            self.out
                .notes
                .push("no name or contact header found".into());
            return;
        }
        // The name: the largest of the first lines, else the first line.
        let candidates = lines.len().min(3);
        let name_index = match lines[..candidates]
            .iter()
            .map(|l| l.size)
            .collect::<Option<Vec<_>>>()
        {
            Some(sizes) => sizes
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1).then(b.0.cmp(&a.0)))
                .map_or(0, |(i, _)| i),
            None => 0,
        };
        let name_line = &lines[name_index].text;
        let looks_like_name = (1..=6).contains(&lines[name_index].words())
            && !name_line
                .chars()
                .any(|c| c.is_ascii_digit() || c == '@' || c == '/')
            && !title_like(name_line);
        if looks_like_name {
            self.out.basics.name = Some(name_line.trim().to_owned());
        } else {
            self.out
                .notes
                .push("could not tell which line is your name".into());
        }
        let mut summary_lines: Vec<String> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            if i == name_index && looks_like_name {
                continue;
            }
            let mut leftover: Vec<String> = Vec::new();
            for fragment in fragments(&line.text) {
                if let Some(contact) = contact_of(&fragment) {
                    if !self.out.basics.contacts.contains(&contact) {
                        self.out.basics.contacts.push(contact);
                    }
                } else if self.out.basics.location.is_none()
                    && location_like(&fragment)
                    && !is_work_mode(&fragment)
                {
                    self.out.basics.location = Some(fragment);
                } else {
                    leftover.push(fragment);
                }
            }
            if leftover.is_empty() {
                continue;
            }
            let joined = leftover.join(" · ");
            if self.out.basics.headline.is_none()
                && i == name_index + 1
                && joined.split_whitespace().count() <= 15
            {
                self.out.basics.headline = Some(joined);
            } else if joined.split_whitespace().count() >= 8 {
                summary_lines.push(line.text.clone());
            } else {
                self.out
                    .ignored
                    .push(format!("header: {}", snippet(&line.text)));
            }
        }
        if !summary_lines.is_empty() && self.out.basics.summary.is_none() {
            self.out.basics.summary = Some(summary_lines.join(" "));
        }
    }

    fn is_date_only(line: &L) -> bool {
        !line.bullet
            && dates::find(&line.text)
                .is_some_and(|r| dates::remove(&line.text, &r.span).is_empty())
    }

    fn is_meta(line: &L) -> bool {
        if line.bullet || line.words() > 8 {
            return false;
        }
        let mut text = line.text.clone();
        if let Some(r) = dates::find(&text) {
            text = dates::remove(&text, &r.span);
        }
        let frags: Vec<String> = fragments(&text)
            .into_iter()
            .flat_map(|f| {
                if location_like(&f) {
                    vec![f]
                } else {
                    f.split(',').map(|s| s.trim().to_owned()).collect()
                }
            })
            .filter(|f| !f.is_empty())
            .collect();
        !frags.is_empty()
            && frags
                .iter()
                .all(|f| employment_of(f).is_some() || is_work_mode(f) || location_like(f))
    }

    fn is_strong_header(&self, line: &L) -> bool {
        if line.bullet || label_of(&line.text).is_some() {
            return false;
        }
        if line.heading >= 3 {
            return true;
        }
        let text = line.text.trim();
        let words = line.words();
        if words > 16 || ends_sentence(text) {
            return false;
        }
        let range = dates::find(text);
        let dated = range.as_ref().is_some_and(|r| {
            !r.single || text[r.span.end..].trim().is_empty() || text.contains('\t')
        });
        let separated = [" — ", " – ", " | ", " @ ", " - ", " · "]
            .iter()
            .any(|s| text.contains(s))
            && words <= 12;
        let at_pattern = text.contains(" at ")
            && words <= 10
            && title_like(text.split(" at ").next().unwrap_or(""));
        (dated && words <= 14) || (text.contains('\t') && words <= 16) || separated || at_pattern
    }

    fn is_weak_header(line: &L) -> bool {
        let text = line.text.trim();
        !line.bullet
            && label_of(text).is_none()
            && line.words() <= 8
            && !ends_sentence(text)
            && !text.ends_with(',')
            && text
                .chars()
                .next()
                .is_some_and(|c| c.is_uppercase() || c.is_ascii_digit())
    }

    /// Splits a section into entries: header lines, then content lines.
    fn entries(&self, lines: &[L], complete: impl Fn(&[L]) -> bool) -> Vec<Entry> {
        let mut entries: Vec<Entry> = Vec::new();
        let mut in_content = false;
        for (i, line) in lines.iter().enumerate() {
            let next = lines.get(i + 1);
            let Some(current) = entries.last_mut() else {
                entries.push(Entry::new(line.clone()));
                in_content = false;
                continue;
            };
            let strong = self.is_strong_header(line);
            if !in_content {
                let header_dated = current
                    .header
                    .iter()
                    .any(|h| dates::find(&h.text).is_some());
                let line_dated = dates::find(&line.text).is_some();
                if (Self::is_date_only(line) && !header_dated) || Self::is_meta(line) {
                    current.header.push(line.clone());
                } else if strong && header_dated && line_dated {
                    // Consecutive positions without bullets.
                    entries.push(Entry::new(line.clone()));
                } else if (strong || Self::is_weak_header(line))
                    && current.header.len() < 3
                    && !complete(&current.header)
                    && !line.bullet
                {
                    current.header.push(line.clone());
                } else {
                    current.content.push(line.clone());
                    in_content = true;
                }
                continue;
            }
            let next_is_header =
                next.is_some_and(|n| Self::is_date_only(n) || self.is_strong_header(n));
            // In a bulleted document, a short line after a break that is
            // followed by bullets heads a new entry.
            let bulleted_entry =
                next.is_some_and(|n| n.bullet) && current.content.iter().any(|c| c.bullet);
            let starts_entry = strong
                || (Self::is_weak_header(line)
                    && (line.gap || line.heading > 0)
                    && (next_is_header || bulleted_entry));
            if starts_entry {
                entries.push(Entry::new(line.clone()));
                in_content = false;
            } else {
                current.content.push(line.clone());
            }
        }
        entries
    }

    fn items(&self, content: &[L]) -> Items {
        let mut items = Items::default();
        let marked = content.iter().any(|l| l.bullet);
        let mut current: Option<String> = None;
        let mut paragraph: Vec<String> = Vec::new();
        let mut previous: Option<&L> = None;
        for line in content {
            let text = line.text.trim();
            if let Some((_, rest)) = label_of(text) {
                if let Some(item) = current.take() {
                    items.bullets.push(item);
                }
                items.tech_line = Some(snippet(text));
                items.technologies.extend(list_items(&rest));
                previous = None;
                continue;
            }
            if marked {
                if line.bullet {
                    if let Some(item) = current.take() {
                        items.bullets.push(item);
                    }
                    current = Some(text.to_owned());
                } else if let Some(item) = current.as_mut().filter(|_| !line.gap) {
                    join_wrapped(item, text);
                } else {
                    if let Some(item) = current.take() {
                        items.bullets.push(item);
                    }
                    paragraph.push(text.to_owned());
                }
            } else {
                let new_item = match (previous, current.as_ref()) {
                    (Some(prev), Some(_)) => {
                        let prev_text = prev.text.trim();
                        line.gap
                            || ends_sentence(prev_text)
                            || (prev_text.chars().count() * 10 < self.full_line * 7
                                && text.chars().next().is_some_and(|c| c.is_uppercase()))
                    }
                    _ => true,
                };
                if new_item {
                    if let Some(item) = current.take() {
                        items.bullets.push(item);
                    }
                    current = Some(text.to_owned());
                } else if let Some(item) = current.as_mut() {
                    join_wrapped(item, text);
                }
            }
            previous = Some(line);
        }
        if let Some(item) = current.take() {
            items.bullets.push(item);
        }
        if !paragraph.is_empty() {
            items.summary = Some(paragraph.join(" "));
        }
        items.bullets.retain(|b| !b.trim().is_empty());
        items
    }

    fn parse_experience(&mut self, lines: &[L]) {
        let entries = self.entries(lines, |header| {
            let h = interpret_role(header);
            h.title.is_some()
                && (h.company.is_some() || h.employment == Some(EmploymentKind::Freelance))
        });
        // A company line without a title, followed by titled roles.
        let mut block: Option<(String, Option<String>)> = None;
        for entry in entries {
            let mut role = interpret_role(&entry.header);
            let items = self.items(&entry.content);
            let first = interpret_role(&entry.header[..1]);
            if role.company.is_some()
                && role.title.is_none()
                && items.bullets.is_empty()
                && items.tech_line.is_none()
            {
                block = role.company.clone().map(|c| (c, role.location.clone()));
                continue;
            }
            if first.company.is_some() && first.title.is_none() {
                block = first.company.clone().map(|c| (c, first.location.clone()));
            } else if role.company.is_some() {
                block = None;
            }
            if role.company.is_none()
                && role.title.is_some()
                && role.employment != Some(EmploymentKind::Freelance)
                && let Some((company, location)) = &block
            {
                role.company = Some(company.clone());
                if role.location.is_none() {
                    role.location = location.clone();
                }
            }
            if role.title.is_none() && role.company.is_none() {
                self.out.ignored.push(format!(
                    "experience: {}",
                    snippet(&header_text(&entry.header))
                ));
                continue;
            }
            if role.title.is_none() {
                role.notes.push("no title found".into());
            }
            if role.company.is_none() && role.employment != Some(EmploymentKind::Freelance) {
                role.notes.push("no company found".into());
            }
            match (&role.range, role.current) {
                (None, _) => role.notes.push("no dates found".into()),
                (Some(r), false) if r.end.is_none() && !r.single => {
                    role.notes.push("no end date found".into())
                }
                _ => {}
            }
            let (start, end) = match &role.range {
                Some(r) if r.single => (r.start, None),
                Some(r) => (r.start, r.end),
                None => (None, None),
            };
            if role.range.as_ref().is_some_and(|r| r.single) {
                role.notes
                    .push("only one date found; it is taken as the start".into());
            }
            self.out.experiences.push(ParsedExperience {
                company: role.company,
                title: role.title,
                employment: role.employment,
                location: role.location,
                start,
                end,
                current: role.current,
                summary: items.summary,
                header: snippet(&header_text(&entry.header)),
                bullets: items.bullets,
                tech_line: items.tech_line,
                technologies: items.technologies,
                ambiguous_header: role.ambiguous,
                notes: role.notes,
            });
        }
    }

    fn parse_projects(&mut self, lines: &[L]) {
        let entries = self.entries(lines, |header| !header.is_empty());
        for entry in entries {
            let items = self.items(&entry.content);
            let mut range: Option<DateRange> = None;
            let mut frags: Vec<String> = Vec::new();
            for line in &entry.header {
                let mut text = line.text.clone();
                if range.is_none()
                    && let Some(r) = dates::find(&text)
                {
                    text = dates::remove(&text, &r.span);
                    range = Some(r);
                }
                frags.extend(fragments(&text));
            }
            let mut url = None;
            let mut role = None;
            let mut rest: Vec<String> = Vec::new();
            for frag in frags {
                if let Some(Contact { value, .. }) = contact_of(&frag)
                    .filter(|c| c.kind != ContactKind::Email && c.kind != ContactKind::Phone)
                {
                    url.get_or_insert(value);
                } else if role.is_none()
                    && frag.split_whitespace().count() <= 4
                    && (has_word(&frag, PROJECT_ROLE_WORDS) || title_like(&frag))
                    && !rest.is_empty()
                {
                    role = Some(frag);
                } else {
                    rest.push(frag);
                }
            }
            if rest.is_empty() {
                continue;
            }
            let name = rest.remove(0);
            let description = (!rest.is_empty()).then(|| rest.join(" — "));
            let (start, end, current) = match &range {
                Some(r) => (r.start, r.end, r.current),
                None => (None, None, false),
            };
            let mut notes = Vec::new();
            if range.is_none() {
                notes.push("no dates found".to_owned());
            }
            self.out.projects.push(ParsedProject {
                name,
                key: None,
                description,
                role,
                url,
                start,
                end,
                current,
                header: snippet(&header_text(&entry.header)),
                bullets: items.bullets,
                tech_line: items.tech_line,
                technologies: items.technologies,
                notes,
            });
        }
    }

    fn parse_education(&mut self, lines: &[L]) {
        let entries = self.entries(lines, |header| !header.is_empty());
        for entry in entries {
            let mut range: Option<DateRange> = None;
            let mut frags: Vec<String> = Vec::new();
            for line in &entry.header {
                let mut text = line.text.clone();
                if range.is_none()
                    && let Some(r) = dates::find(&text)
                {
                    text = dates::remove(&text, &r.span);
                    range = Some(r);
                }
                for frag in fragments(&text) {
                    // "B.Sc. in Computer Science, University X"
                    let parts: Vec<&str> = frag.split(", ").collect();
                    if parts.len() == 2
                        && (has_word(parts[0], DEGREE_WORDS) != has_word(parts[1], DEGREE_WORDS))
                    {
                        frags.extend(parts.into_iter().map(str::to_owned));
                    } else {
                        frags.push(frag);
                    }
                }
            }
            let degree_at = frags.iter().position(|f| has_word(f, DEGREE_WORDS));
            let institution_at = frags
                .iter()
                .enumerate()
                .position(|(i, f)| Some(i) != degree_at && has_word(f, INSTITUTION_WORDS))
                .or_else(|| (0..frags.len()).find(|i| Some(*i) != degree_at));
            let Some(institution_at) = institution_at else {
                self.out.ignored.push(format!(
                    "education: {}",
                    snippet(&header_text(&entry.header))
                ));
                continue;
            };
            let (degree, field) = match degree_at {
                Some(i) => split_degree(&frags[i]),
                None => (None, None),
            };
            let mut notes = Vec::new();
            if degree.is_none() {
                notes.push("no degree found".to_owned());
            }
            let (start, end, current) = match &range {
                // A single year on a degree is when it was completed.
                Some(r) if r.single => (None, r.end, false),
                Some(r) => (r.start, r.end, r.current),
                None => (None, None, false),
            };
            for line in &entry.content {
                self.out
                    .ignored
                    .push(format!("education: {}", snippet(&line.text)));
            }
            self.out.education.push(ParsedEducation {
                institution: frags[institution_at].clone(),
                degree,
                field,
                start,
                end,
                current,
                header: snippet(&header_text(&entry.header)),
                notes,
            });
        }
    }

    fn parse_skills(&mut self, lines: &[L]) {
        let mut pending: Option<ParsedSkillLine> = None;
        for line in lines {
            let text = line.text.trim();
            let continues = pending
                .as_ref()
                .is_some_and(|p| p.line.trim_end().ends_with(','));
            if continues && let Some(p) = pending.as_mut() {
                p.line = format!("{} {text}", p.line);
                p.skills.extend(list_items(text));
                continue;
            }
            if let Some(done) = pending.take() {
                self.push_skills(done);
            }
            let (category, list) = match text.split_once(':') {
                Some((c, rest)) if c.split_whitespace().count() <= 4 => {
                    (Some(c.trim().to_owned()), rest)
                }
                _ => (None, text),
            };
            pending = Some(ParsedSkillLine {
                category,
                skills: list_items(list),
                line: snippet(text),
            });
        }
        if let Some(done) = pending.take() {
            self.push_skills(done);
        }
    }

    fn push_skills(&mut self, mut line: ParsedSkillLine) {
        let (keep, prose): (Vec<String>, Vec<String>) = line
            .skills
            .drain(..)
            .partition(|s| s.split_whitespace().count() <= 4);
        if !prose.is_empty() {
            self.out.ignored.push(format!("skills: {}", line.line));
        }
        if !keep.is_empty() {
            line.skills = keep;
            self.out.skills.push(line);
        }
    }

    fn parse_languages(&mut self, lines: &[L]) {
        for line in lines {
            for item in line.text.split([',', ';', '|', '·', '•']) {
                let item = item.trim();
                if item.is_empty() {
                    continue;
                }
                let (name, level) = if let Some((name, rest)) = item.split_once('(') {
                    (name.trim(), Some(rest.trim_end_matches(')').trim()))
                } else if let Some((name, level)) = item.split_once([':', '-', '–']) {
                    (name.trim(), Some(level.trim()))
                } else {
                    (item, None)
                };
                if name.is_empty() || name.split_whitespace().count() > 3 {
                    self.out.ignored.push(format!("languages: {item}"));
                    continue;
                }
                self.out.basics.languages.push(SpokenLanguage {
                    name: name.to_owned(),
                    level: level.filter(|l| !l.is_empty()).map(str::to_owned),
                });
            }
        }
    }
}

fn from_line(line: &Line) -> L {
    L {
        text: line.text.clone(),
        bullet: line.bullet,
        gap: line.gap_before,
        heading: line.heading,
        size: line.size,
    }
}

fn mode(sorted: &[i64]) -> Option<i64> {
    let mut best: Option<(i64, usize)> = None;
    let mut i = 0;
    while i < sorted.len() {
        let j = sorted[i..].iter().take_while(|v| **v == sorted[i]).count();
        if best.is_none_or(|(_, n)| j > n) {
            best = Some((sorted[i], j));
        }
        i += j;
    }
    best.map(|(v, _)| v)
}

fn paragraphs(lines: &[L]) -> String {
    let mut out = String::new();
    for line in lines {
        if line.gap && !out.is_empty() {
            out.push_str("\n\n");
        } else if !out.is_empty() {
            if out.ends_with('\n') {
                // already separated
            } else {
                join_wrapped(&mut out, "");
                out.pop();
            }
        }
        let text = line.text.trim();
        if out.is_empty() || out.ends_with('\n') {
            out.push_str(text);
        } else {
            join_wrapped(&mut out, text);
        }
    }
    out
}

#[derive(Debug, Clone)]
struct Entry {
    header: Vec<L>,
    content: Vec<L>,
}

impl Entry {
    fn new(line: L) -> Self {
        Self {
            header: vec![line],
            content: Vec::new(),
        }
    }
}

fn header_text(lines: &[L]) -> String {
    lines
        .iter()
        .map(|l| l.text.trim())
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Default)]
struct Items {
    summary: Option<String>,
    bullets: Vec<String>,
    tech_line: Option<String>,
    technologies: Vec<String>,
}

#[derive(Debug, Default)]
struct Role {
    company: Option<String>,
    title: Option<String>,
    employment: Option<EmploymentKind>,
    location: Option<String>,
    range: Option<DateRange>,
    current: bool,
    ambiguous: bool,
    notes: Vec<String>,
}

/// Reads company, title, dates, location and employment type from an
/// experience entry's header lines.
fn interpret_role(header: &[L]) -> Role {
    let mut role = Role::default();
    let mut titles: Vec<String> = Vec::new();
    let mut orgs: Vec<String> = Vec::new();
    for line in header {
        let mut text = line.text.clone();
        if role.range.is_none()
            && let Some(r) = dates::find(&text)
        {
            text = dates::remove(&text, &r.span);
            role.current = r.current;
            role.range = Some(r);
        }
        for mut frag in fragments(&text) {
            // "(Contract)" inside a fragment.
            if let Some(open) = frag.rfind('(')
                && frag.ends_with(')')
                && let Some(kind) = employment_of(&frag[open..])
            {
                role.employment.get_or_insert(kind);
                frag = frag[..open].trim().to_owned();
            }
            if frag.is_empty() {
                continue;
            }
            if let Some(kind) = employment_of(&frag) {
                role.employment.get_or_insert(kind);
                continue;
            }
            if is_work_mode(&frag) || location_like(&frag) {
                role.location.get_or_insert(frag);
                continue;
            }
            // "Senior Engineer at Acme", "Senior Engineer, Acme Corp"
            if let Some((left, right)) = frag.split_once(" at ").or_else(|| frag.split_once(", "))
                && title_like(left)
                && !title_like(right)
                && !right.trim().is_empty()
            {
                titles.push(left.trim().to_owned());
                orgs.push(right.trim().to_owned());
                continue;
            }
            if title_like(&frag) {
                // "Independent Consultant", "Freelance Developer"
                if let Some(first) = frag.split_whitespace().next()
                    && employment_of(first) == Some(EmploymentKind::Freelance)
                {
                    role.employment.get_or_insert(EmploymentKind::Freelance);
                }
                titles.push(frag);
            } else {
                orgs.push(frag);
            }
        }
    }
    match (titles.len(), orgs.len()) {
        (0, 0) => {}
        (0, 1) => role.company = orgs.pop(),
        (0, _) => {
            role.company = Some(orgs[0].clone());
            role.title = Some(orgs[1].clone());
            role.ambiguous = true;
            role.notes.push(format!(
                "could not tell the title from the company in “{} — {}”; assumed the company comes first",
                orgs[0], orgs[1]
            ));
        }
        (_, _) => {
            role.title = Some(titles[0].clone());
            role.company = orgs.first().cloned();
        }
    }
    role
}

/// "B.Sc. in Computer Science" → ("B.Sc.", "Computer Science").
fn split_degree(text: &str) -> (Option<String>, Option<String>) {
    let text = text.trim();
    for sep in [" in ", " em ", ", "] {
        if let Some(at) = text.rfind(sep) {
            let (degree, field) = (text[..at].trim(), text[at + sep.len()..].trim());
            if has_word(degree, DEGREE_WORDS) && !field.is_empty() {
                return (Some(degree.to_owned()), Some(field.to_owned()));
            }
        }
    }
    // "BSc Computer Science": degree abbreviation, then the field.
    if let Some((first, rest)) = text.split_once(' ')
        && has_word(first, DEGREE_WORDS)
        && !has_word(rest, DEGREE_WORDS)
        && rest.chars().next().is_some_and(char::is_uppercase)
    {
        return (Some(first.to_owned()), Some(rest.trim().to_owned()));
    }
    (Some(text.to_owned()), None)
}

/// Recognizes an email, phone number or URL.
fn contact_of(fragment: &str) -> Option<Contact> {
    let f = fragment.trim();
    let token = f
        .split_whitespace()
        .find(|t| {
            t.contains('@') || t.contains("://") || t.starts_with("www.") || t.contains(".com/")
        })
        .unwrap_or(f);
    let token =
        token.trim_matches(|c: char| c == '(' || c == ')' || c == '<' || c == '>' || c == ',');
    if let Some((user, domain)) = token.split_once('@')
        && !user.is_empty()
        && domain.contains('.')
        && !token.contains('/')
    {
        return Some(Contact {
            kind: ContactKind::Email,
            value: token.to_owned(),
        });
    }
    let lower = token.to_lowercase();
    let url_like = lower.contains("://")
        || lower.starts_with("www.")
        || [
            ".com/", ".io/", ".dev/", ".me/", ".org/", ".net/", ".co/", ".app/",
        ]
        .iter()
        .any(|tld| lower.contains(tld))
        || [".com", ".io", ".dev", ".me", ".org", ".net", ".app"]
            .iter()
            .any(|tld| lower.ends_with(tld) && !lower.contains(' ') && lower.len() > 4);
    if url_like && !token.contains(' ') {
        let kind = if lower.contains("linkedin.com") {
            ContactKind::Linkedin
        } else if lower.contains("github.com") {
            ContactKind::Github
        } else {
            ContactKind::Website
        };
        return Some(Contact {
            kind,
            value: token.to_owned(),
        });
    }
    let digits = f.chars().filter(char::is_ascii_digit).count();
    if digits >= 8
        && f.chars()
            .all(|c| c.is_ascii_digit() || " +-().".contains(c))
    {
        return Some(Contact {
            kind: ContactKind::Phone,
            value: f.to_owned(),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use jobhunt_profile::{DocumentKind, PartialDate};

    use super::*;
    use crate::extract::extract;

    fn parse_text(text: &str) -> ParsedResume {
        let doc = extract(text.as_bytes(), Some("txt")).unwrap();
        DeterministicParser.parse(&doc)
    }

    fn d(s: &str) -> Option<PartialDate> {
        Some(s.parse().unwrap())
    }

    const RESUME: &str = "\
Ana Lima
Staff Engineer
Lisbon, Portugal | ana@example.org | github.com/analima

EXPERIENCE

Acme Payments — Staff Software Engineer | Jan 2021 – Present
- Led the redesign of the settlement service, cutting batch time by 60%.
- Mentored five engineers.
Tech: Go, PostgreSQL, Kafka

Acme Payments — Senior Software Engineer | Mar 2018 – Dec 2020
- Built the PIX integration used by 2M customers.

Globex
Backend Developer, Jun 2016 – Feb 2018
- Maintained billing APIs in Python and Django.
Platform Engineer, 2015 – 2016
- Ran the Kubernetes clusters.

Independent Consultant
- Advised startups on architecture.

EDUCATION
University of Porto — BSc in Informatics, 2012 – 2015

SKILLS
Languages: Go, Python, SQL,
  TypeScript
Infrastructure: Kubernetes, Terraform

INTERESTS
Climbing
";

    #[test]
    fn reads_a_plain_text_resume() {
        let r = parse_text(RESUME);
        assert_eq!(r.basics.name.as_deref(), Some("Ana Lima"));
        assert_eq!(r.basics.headline.as_deref(), Some("Staff Engineer"));
        assert_eq!(r.basics.location.as_deref(), Some("Lisbon, Portugal"));
        assert_eq!(r.basics.contacts.len(), 2);

        let exps: Vec<(Option<&str>, Option<&str>)> = r
            .experiences
            .iter()
            .map(|e| (e.company.as_deref(), e.title.as_deref()))
            .collect();
        assert_eq!(
            exps,
            [
                (Some("Acme Payments"), Some("Staff Software Engineer")),
                (Some("Acme Payments"), Some("Senior Software Engineer")),
                (Some("Globex"), Some("Backend Developer")),
                (Some("Globex"), Some("Platform Engineer")),
                (None, Some("Independent Consultant")),
            ]
        );
        let staff = &r.experiences[0];
        assert_eq!(
            (staff.start, staff.end, staff.current),
            (d("2021-01"), None, true)
        );
        assert_eq!(staff.bullets.len(), 2);
        assert_eq!(staff.technologies, ["Go", "PostgreSQL", "Kafka"]);
        assert_eq!(
            staff.tech_line.as_deref(),
            Some("Tech: Go, PostgreSQL, Kafka")
        );
        let platform = &r.experiences[3];
        assert_eq!((platform.start, platform.end), (d("2015"), d("2016")));
        assert!(platform.notes.is_empty(), "{:?}", platform.notes);
        let consultant = &r.experiences[4];
        assert!(consultant.notes.contains(&"no dates found".to_owned()));

        assert_eq!(r.education.len(), 1);
        let edu = &r.education[0];
        assert_eq!(edu.institution, "University of Porto");
        assert_eq!(edu.degree.as_deref(), Some("BSc"));
        assert_eq!(edu.field.as_deref(), Some("Informatics"));

        let skills: Vec<&str> = r
            .skills
            .iter()
            .flat_map(|s| s.skills.iter().map(String::as_str))
            .collect();
        assert_eq!(
            skills,
            [
                "Go",
                "Python",
                "SQL",
                "TypeScript",
                "Kubernetes",
                "Terraform"
            ]
        );
        assert_eq!(r.ignored, ["INTERESTS: Climbing"]);
        let _ = DocumentKind::Text;
    }

    #[test]
    fn contact_details() {
        assert_eq!(
            contact_of("ana@example.org").unwrap().kind,
            ContactKind::Email
        );
        assert_eq!(
            contact_of("linkedin.com/in/ana").unwrap().kind,
            ContactKind::Linkedin
        );
        assert_eq!(
            contact_of("https://ana.dev").unwrap().kind,
            ContactKind::Website
        );
        assert_eq!(
            contact_of("+55 11 91234-5678").unwrap().kind,
            ContactKind::Phone
        );
        assert_eq!(contact_of("São Paulo, Brazil"), None);
        assert_eq!(contact_of("Node.js"), None);
    }

    #[test]
    fn degrees_split_into_degree_and_field() {
        assert_eq!(
            split_degree("B.Sc. in Computer Science"),
            (Some("B.Sc.".into()), Some("Computer Science".into()))
        );
        assert_eq!(
            split_degree("Bachelor of Science in Computer Engineering"),
            (
                Some("Bachelor of Science".into()),
                Some("Computer Engineering".into())
            )
        );
        assert_eq!(split_degree("MBA"), (Some("MBA".into()), None));
    }

    #[test]
    fn ambiguous_headers_are_flagged() {
        let role = interpret_role(&[L {
            text: "Initech — Payments Platform\t2019 – 2020".into(),
            bullet: false,
            gap: false,
            heading: 0,
            size: None,
        }]);
        assert_eq!(role.company.as_deref(), Some("Initech"));
        assert_eq!(role.title.as_deref(), Some("Payments Platform"));
        assert!(role.ambiguous);
    }
}
