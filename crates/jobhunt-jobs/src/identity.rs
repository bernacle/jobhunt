//! Cross-source identity: deciding when records from different sources are
//! the same logical opportunity.
//!
//! Every source record keeps its own stable [`JobId`]. On top of that, records
//! are grouped into opportunities ([`OpportunityId`]) using only deterministic
//! evidence that two records describe the same posting:
//!
//! * **ATS references** embedded in URLs: a Greenhouse job id (in
//!   `boards.greenhouse.io/<board>/jobs/<id>`, `job-boards.greenhouse.io/...`,
//!   or a first-party page's `?gh_jid=<id>`), a Lever posting id
//!   (`jobs.lever.co/<site>/<uuid>`), an Ashby job id
//!   (`jobs.ashbyhq.com/<board>/<uuid>` or `?ashby_jid=<uuid>`), or a Work at
//!   a Startup job id (`?signup_job_id=<id>`, `workatastartup.com/jobs/<id>`).
//!   These ids are global within their ATS, so two URLs carrying the same one
//!   point at the same posting even when hosts or paths differ.
//! * **Exact canonical URLs**: a posting URL or application URL that is
//!   identical (after [`CanonicalUrl`] normalization) in two records.
//!
//! Similar titles, companies or locations are never evidence on their own.
//! Titles are used only as a veto (see [`titles_compatible`]). Three
//! safeguards keep false merges out, because a wrong merge hides a real job
//! while a missed merge only shows a duplicate:
//!
//! 1. A key shared by two records *of the same source* is ambiguous (a
//!    generic "apply here" URL, a reused listing page) and is ignored.
//! 2. Linked records must have compatible titles.
//! 3. Two groups that each contain a record of the same source are never
//!    merged: a source does not list one posting twice.
//!
//! Records from different sources with the same company and title but no
//! shared evidence are reported as *look-alikes* (see [`Grouping`]) so the
//! remaining duplicates are visible, but they are never grouped.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};
use jobhunt_core::text::search_key;
use jobhunt_core::{CanonicalUrl, SourceKey};
use url::Url;

use crate::model::{JobId, JobPosting, OpportunityId};

/// A job's identity in an applicant tracking system, recovered from a URL.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AtsJobRef {
    /// `greenhouse`, `lever`, `ashby` or `workatastartup`.
    pub system: &'static str,
    /// The system's own job id (lowercased for UUIDs).
    pub id: String,
}

/// Recognizes the ATS job a URL points at, if any.
pub fn ats_job_ref(url: &CanonicalUrl) -> Option<AtsJobRef> {
    let parsed = Url::parse(url.as_str()).ok()?;
    let host = parsed.host_str()?;
    let segments: Vec<&str> = parsed
        .path_segments()
        .map(|s| s.filter(|seg| !seg.is_empty()).collect())
        .unwrap_or_default();
    let query = |name: &str| {
        parsed
            .query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    let make = |system: &'static str, id: String| Some(AtsJobRef { system, id });

    // Query parameters that ATS embeds add to first-party pages.
    if let Some(id) = query("gh_jid").filter(|v| is_numeric_id(v)) {
        return make("greenhouse", id);
    }
    if let Some(id) = query("ashby_jid").and_then(|v| uuid(&v)) {
        return make("ashby", id);
    }
    if let Some(id) = query("signup_job_id").filter(|v| is_numeric_id(v)) {
        return make("workatastartup", id);
    }

    match host {
        "boards.greenhouse.io"
        | "job-boards.greenhouse.io"
        | "boards.eu.greenhouse.io"
        | "job-boards.eu.greenhouse.io" => match segments.as_slice() {
            [_board, "jobs", id, ..] if is_numeric_id(id) => make("greenhouse", (*id).to_owned()),
            ["embed", "job_app"] => query("token")
                .filter(|v| is_numeric_id(v))
                .and_then(|id| make("greenhouse", id)),
            _ => None,
        },
        "jobs.lever.co" | "jobs.eu.lever.co" => match segments.as_slice() {
            [_site, id, ..] => uuid(id).and_then(|id| make("lever", id)),
            _ => None,
        },
        "jobs.ashbyhq.com" => match segments.as_slice() {
            [_board, id, ..] => uuid(id).and_then(|id| make("ashby", id)),
            _ => None,
        },
        "www.workatastartup.com" | "workatastartup.com" => match segments.as_slice() {
            ["jobs", id] if is_numeric_id(id) => make("workatastartup", (*id).to_owned()),
            _ => None,
        },
        _ => None,
    }
}

fn is_numeric_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 20 && value.bytes().all(|b| b.is_ascii_digit())
}

/// Returns the lowercase form of a canonical 8-4-4-4-12 UUID.
fn uuid(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let ok = bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
    ok.then(|| value.to_ascii_lowercase())
}

/// The identity evidence a posting carries, as storage-friendly keys:
/// `ats:<system>:<id>` and `url:<canonical url>`. Sorted and de-duplicated.
pub fn evidence_keys(posting: &JobPosting) -> Vec<String> {
    let mut keys = Vec::with_capacity(4);
    for url in std::iter::once(&posting.url).chain(posting.apply_url.as_ref()) {
        keys.push(format!("url:{url}"));
        if let Some(ats) = ats_job_ref(url) {
            keys.push(format!("ats:{}:{}", ats.system, ats.id));
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

/// Whether two titles plausibly name the same posting: equal after
/// normalization, or one is a whole-word part of the other ("Account
/// Manager" vs "Account Manager - London").
pub fn titles_compatible(a: &str, b: &str) -> bool {
    let a = format!(" {} ", search_key(a));
    let b = format!(" {} ", search_key(b));
    if a.trim().is_empty() || b.trim().is_empty() {
        return false;
    }
    a == b || a.contains(&b) || b.contains(&a)
}

/// What grouping needs to know about one stored source record.
#[derive(Debug, Clone)]
pub struct IdentityEntry {
    pub job: JobId,
    pub source: SourceKey,
    pub company: String,
    pub title: String,
    pub first_seen_at: DateTime<Utc>,
    /// The opportunity currently stored for the record.
    pub opportunity: OpportunityId,
    pub evidence: Vec<String>,
}

/// Two records linked by one piece of evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub a: JobId,
    pub b: JobId,
    pub evidence: String,
}

/// Result of [`group`].
#[derive(Debug, Default)]
pub struct Grouping {
    /// Records whose opportunity must change, with the new value.
    pub assignments: Vec<(JobId, OpportunityId)>,
    /// Every accepted cross-source link.
    pub links: Vec<Link>,
    /// Links rejected by a safeguard, for diagnostics.
    pub rejected: Vec<(Link, &'static str)>,
    /// Number of opportunities backed by more than one source record.
    pub multi_source_groups: usize,
    /// Company-and-title pairs (normalized) listed by more than one source
    /// in different opportunities: probably the same job, but without
    /// evidence, so kept apart. Each entry lists the records involved.
    pub look_alikes: Vec<Vec<JobId>>,
}

/// Groups records into opportunities. Deterministic: the same input always
/// yields the same grouping, regardless of input order.
pub fn group(entries: &[IdentityEntry]) -> Grouping {
    // key -> source -> records carrying it. BTreeMaps keep processing order
    // (and therefore tie-breaking) deterministic; `ats:` keys sort before
    // `url:` keys, so the strongest evidence is considered first.
    let mut by_key: BTreeMap<&str, BTreeMap<&SourceKey, Vec<usize>>> = BTreeMap::new();
    for (i, entry) in entries.iter().enumerate() {
        for key in &entry.evidence {
            by_key
                .entry(key.as_str())
                .or_default()
                .entry(&entry.source)
                .or_default()
                .push(i);
        }
    }

    let mut sets = DisjointSets::new(entries);
    let mut grouping = Grouping::default();
    for (key, sources) in &by_key {
        if sources.len() < 2 {
            continue;
        }
        if sources.values().any(|records| records.len() > 1) {
            // Safeguard 1: ambiguous within a source.
            continue;
        }
        let members: Vec<usize> = sources.values().map(|records| records[0]).collect();
        let anchor = members[0];
        for &other in &members[1..] {
            let (a, b) = (&entries[anchor], &entries[other]);
            let link = Link {
                a: a.job,
                b: b.job,
                evidence: (*key).to_owned(),
            };
            if !titles_compatible(&a.title, &b.title) {
                grouping.rejected.push((link, "titles differ"));
                continue;
            }
            match sets.union(anchor, other) {
                Union::Merged | Union::AlreadyTogether => grouping.links.push(link),
                Union::SourceConflict => {
                    grouping.rejected.push((link, "same source on both sides"))
                }
            }
        }
    }

    // Each group is named after its founding record: earliest first seen,
    // then smallest id.
    let mut founders: HashMap<usize, usize> = HashMap::new();
    for i in 0..entries.len() {
        let root = sets.find(i);
        let founder = founders.entry(root).or_insert(i);
        let (current, candidate) = (&entries[*founder], &entries[i]);
        if (candidate.first_seen_at, candidate.job) < (current.first_seen_at, current.job) {
            *founder = i;
        }
    }
    let mut sizes: HashMap<usize, usize> = HashMap::new();
    for i in 0..entries.len() {
        *sizes.entry(sets.find(i)).or_default() += 1;
    }
    grouping.multi_source_groups = sizes.values().filter(|&&n| n > 1).count();

    for (i, entry) in entries.iter().enumerate() {
        let founder = founders[&sets.find(i)];
        let opportunity = OpportunityId::founded_by(entries[founder].job);
        if entry.opportunity != opportunity {
            grouping.assignments.push((entry.job, opportunity));
        }
    }
    grouping.assignments.sort();

    // Look-alikes: same normalized company and title, several sources,
    // several opportunities.
    let mut by_name: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for (i, entry) in entries.iter().enumerate() {
        let key = (search_key(&entry.company), search_key(&entry.title));
        if !key.0.is_empty() && !key.1.is_empty() {
            by_name.entry(key).or_default().push(i);
        }
    }
    for members in by_name.into_values() {
        let mut sources: Vec<&SourceKey> = members.iter().map(|&i| &entries[i].source).collect();
        sources.sort();
        sources.dedup();
        let mut roots: Vec<usize> = members.iter().map(|&i| sets.find(i)).collect();
        roots.sort_unstable();
        roots.dedup();
        if sources.len() > 1 && roots.len() > 1 {
            let mut jobs: Vec<JobId> = members.iter().map(|&i| entries[i].job).collect();
            jobs.sort();
            grouping.look_alikes.push(jobs);
        }
    }
    grouping
}

enum Union {
    Merged,
    AlreadyTogether,
    SourceConflict,
}

/// Union-find over record indices that also tracks which sources each set
/// contains, to enforce safeguard 3.
struct DisjointSets<'a> {
    parent: Vec<usize>,
    sources: Vec<Vec<&'a SourceKey>>,
}

impl<'a> DisjointSets<'a> {
    fn new(entries: &'a [IdentityEntry]) -> Self {
        Self {
            parent: (0..entries.len()).collect(),
            sources: entries.iter().map(|e| vec![&e.source]).collect(),
        }
    }

    fn find(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }

    fn union(&mut self, a: usize, b: usize) -> Union {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return Union::AlreadyTogether;
        }
        if self.sources[ra]
            .iter()
            .any(|s| self.sources[rb].contains(s))
        {
            return Union::SourceConflict;
        }
        let (keep, merge) = if ra < rb { (ra, rb) } else { (rb, ra) };
        self.parent[merge] = keep;
        let moved = std::mem::take(&mut self.sources[merge]);
        self.sources[keep].extend(moved);
        Union::Merged
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::model::tests::posting;

    fn url(s: &str) -> CanonicalUrl {
        CanonicalUrl::parse(s).unwrap()
    }

    fn ats(s: &str) -> Option<(String, String)> {
        ats_job_ref(&url(s)).map(|r| (r.system.to_owned(), r.id))
    }

    fn pair(system: &str, id: &str) -> Option<(String, String)> {
        Some((system.to_owned(), id.to_owned()))
    }

    #[test]
    fn recognizes_greenhouse_job_urls() {
        let expected = pair("greenhouse", "5426468004");
        for u in [
            "https://boards.greenhouse.io/figma/jobs/5426468004?gh_jid=5426468004",
            "https://boards.greenhouse.io/figma/jobs/5426468004",
            "https://job-boards.greenhouse.io/figma/jobs/5426468004#app",
            "https://job-boards.eu.greenhouse.io/figma/jobs/5426468004",
            "https://boards.greenhouse.io/embed/job_app?for=figma&token=5426468004",
            "https://www.figma.com/careers/job/?gh_jid=5426468004&utm_source=x",
        ] {
            assert_eq!(ats(u), expected, "{u}");
        }
        assert_eq!(
            ats("https://stripe.com/jobs/search?gh_jid=8172510"),
            pair("greenhouse", "8172510")
        );
        // A board page, a non-numeric id, a different job: not this job.
        assert_eq!(ats("https://boards.greenhouse.io/figma"), None);
        assert_eq!(ats("https://boards.greenhouse.io/figma/jobs/abc"), None);
        assert_ne!(
            ats("https://boards.greenhouse.io/figma/jobs/5426468005"),
            expected
        );
    }

    #[test]
    fn recognizes_lever_ashby_and_waas_urls() {
        let lever = pair("lever", "2193db3f-77c5-43b8-b030-8f92c9882bf1");
        assert_eq!(
            ats("https://jobs.lever.co/spotify/2193db3f-77c5-43b8-b030-8f92c9882bf1"),
            lever
        );
        assert_eq!(
            ats("https://jobs.lever.co/spotify/2193DB3F-77C5-43B8-B030-8F92C9882BF1/apply"),
            lever
        );
        assert_eq!(ats("https://jobs.lever.co/spotify"), None);
        assert_eq!(ats("https://jobs.lever.co/spotify/not-a-uuid"), None);

        let ashby = pair("ashby", "d3bc1ced-3ce4-4086-a050-555055dbb1ff");
        assert_eq!(
            ats("https://jobs.ashbyhq.com/linear/d3bc1ced-3ce4-4086-a050-555055dbb1ff/application"),
            ashby
        );
        assert_eq!(
            ats("https://linear.app/careers/x?ashby_jid=d3bc1ced-3ce4-4086-a050-555055dbb1ff"),
            ashby
        );

        let waas = pair("workatastartup", "93346");
        assert_eq!(
            ats("https://www.workatastartup.com/application?signup_job_id=93346"),
            waas
        );
        assert_eq!(ats("https://www.workatastartup.com/jobs/93346"), waas);
        assert_eq!(ats("https://example.com/jobs/93346"), None);
    }

    #[test]
    fn evidence_includes_urls_and_ats_refs() {
        let mut p = posting("greenhouse:figma", Some("5426468004"), "Engineer");
        p.url = url("https://boards.greenhouse.io/figma/jobs/5426468004?gh_jid=5426468004");
        p.apply_url = None;
        assert_eq!(
            evidence_keys(&p),
            vec![
                "ats:greenhouse:5426468004",
                "url:https://boards.greenhouse.io/figma/jobs/5426468004?gh_jid=5426468004",
            ]
        );
    }

    #[test]
    fn title_compatibility() {
        assert!(titles_compatible("Account Manager ", "account manager"));
        assert!(titles_compatible(
            "Account Manager",
            "Account Manager - London"
        ));
        assert!(!titles_compatible("Account Manager", "Account Executive"));
        assert!(!titles_compatible("Engineer", "Engineering Manager"));
        assert!(!titles_compatible("", ""));
    }

    fn entry(source: &str, id: &str, title: &str, minute: u32, evidence: &[&str]) -> IdentityEntry {
        let job = posting(source, Some(id), title).id();
        IdentityEntry {
            job,
            source: source.parse().unwrap(),
            company: "Acme".to_owned(),
            title: title.to_owned(),
            first_seen_at: Utc.with_ymd_and_hms(2026, 9, 1, 0, minute, 0).unwrap(),
            opportunity: OpportunityId::founded_by(job),
            evidence: evidence.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    fn opportunity_of(entries: &[IdentityEntry], grouping: &Grouping, i: usize) -> OpportunityId {
        grouping
            .assignments
            .iter()
            .find(|(job, _)| *job == entries[i].job)
            .map_or(entries[i].opportunity, |(_, opp)| *opp)
    }

    #[test]
    fn links_records_sharing_strong_evidence_across_sources() {
        let entries = vec![
            entry(
                "greenhouse:figma",
                "1",
                "Engineer",
                5,
                &[
                    "ats:greenhouse:1",
                    "url:https://boards.greenhouse.io/figma/jobs/1",
                ],
            ),
            entry(
                "careers:figma",
                "a",
                "Engineer",
                1,
                &["ats:greenhouse:1", "url:https://figma.com/careers?gh_jid=1"],
            ),
            entry(
                "greenhouse:figma",
                "2",
                "Designer",
                0,
                &["ats:greenhouse:2"],
            ),
        ];
        let grouping = group(&entries);
        assert_eq!(grouping.links.len(), 1);
        assert_eq!(grouping.multi_source_groups, 1);
        // The earliest-seen record founds the group.
        let founder = OpportunityId::founded_by(entries[1].job);
        assert_eq!(opportunity_of(&entries, &grouping, 0), founder);
        assert_eq!(opportunity_of(&entries, &grouping, 1), founder);
        assert_eq!(
            opportunity_of(&entries, &grouping, 2),
            entries[2].opportunity
        );
        assert_eq!(
            grouping.assignments.len(),
            1,
            "only the joining record moves"
        );
    }

    #[test]
    fn identical_urls_link_but_similar_titles_alone_never_do() {
        let entries = vec![
            entry(
                "lever:acme",
                "1",
                "Backend Engineer",
                0,
                &["url:https://acme.com/jobs/1"],
            ),
            entry(
                "yc:acme",
                "9",
                "Backend Engineer",
                1,
                &["url:https://acme.com/jobs/1"],
            ),
            // Same company and title but no shared evidence: separate.
            entry(
                "greenhouse:acme",
                "7",
                "Backend Engineer",
                2,
                &["url:https://acme.com/jobs/7"],
            ),
        ];
        let grouping = group(&entries);
        assert_eq!(grouping.links.len(), 1);
        assert_eq!(
            opportunity_of(&entries, &grouping, 1),
            entries[0].opportunity
        );
        assert_eq!(
            opportunity_of(&entries, &grouping, 2),
            entries[2].opportunity
        );
        // The unlinked record is reported as a look-alike of the pair.
        assert_eq!(grouping.look_alikes.len(), 1);
        assert_eq!(grouping.look_alikes[0].len(), 3);
    }

    #[test]
    fn look_alikes_need_several_sources_and_one_company() {
        // Two same-titled jobs of one source are simply two openings.
        let entries = vec![
            entry("lever:acme", "1", "Engineer", 0, &[]),
            entry("lever:acme", "2", "Engineer", 1, &[]),
        ];
        assert!(group(&entries).look_alikes.is_empty());

        let mut other_company = entry("yc:beta", "3", "Engineer", 2, &[]);
        other_company.company = "Beta".into();
        let entries = vec![entry("lever:acme", "1", "Engineer", 0, &[]), other_company];
        assert!(group(&entries).look_alikes.is_empty());

        // Grouped records are not look-alikes of each other.
        let key = "ats:lever:x";
        let entries = vec![
            entry("lever:acme", "1", "Engineer", 0, &[key]),
            entry("yc:acme", "2", "Engineer", 1, &[key]),
        ];
        assert!(group(&entries).look_alikes.is_empty());
    }

    #[test]
    fn ambiguous_keys_are_ignored() {
        // Two different jobs from one source share a generic apply URL.
        let apply = "url:https://acme.com/careers/apply";
        let entries = vec![
            entry("lever:acme", "1", "Engineer", 0, &[apply]),
            entry("lever:acme", "2", "Engineer", 1, &[apply]),
            entry("yc:acme", "3", "Engineer", 2, &[apply]),
        ];
        let grouping = group(&entries);
        assert!(grouping.links.is_empty());
        assert!(grouping.assignments.is_empty());
    }

    #[test]
    fn conflicting_titles_veto_a_link() {
        let key = "ats:greenhouse:1";
        let entries = vec![
            entry("greenhouse:a", "1", "Engineer", 0, &[key]),
            entry("greenhouse:b", "1", "Office Manager", 1, &[key]),
        ];
        let grouping = group(&entries);
        assert!(grouping.links.is_empty());
        assert_eq!(grouping.rejected.len(), 1);
        assert!(grouping.assignments.is_empty());
    }

    #[test]
    fn never_merges_two_records_of_one_source() {
        // A1 ~ B via one key, B ~ A2 via another: A1 and A2 are distinct
        // jobs of source A, so the second link must be refused.
        let entries = vec![
            entry("lever:a", "1", "Engineer", 0, &["url:https://x.com/1"]),
            entry(
                "yc:b",
                "2",
                "Engineer",
                1,
                &["url:https://x.com/1", "url:https://x.com/2"],
            ),
            entry("lever:a", "3", "Engineer", 2, &["url:https://x.com/2"]),
        ];
        let grouping = group(&entries);
        assert_eq!(grouping.links.len(), 1);
        assert_eq!(grouping.rejected.len(), 1);
        assert_eq!(grouping.rejected[0].1, "same source on both sides");
        assert_ne!(
            opportunity_of(&entries, &grouping, 0),
            opportunity_of(&entries, &grouping, 2)
        );
    }

    #[test]
    fn grouping_is_order_independent_and_splits_when_evidence_disappears() {
        let key = "ats:lever:x";
        let mut entries = vec![
            entry("lever:a", "1", "Engineer", 3, &[key]),
            entry("lever:b", "1", "Engineer", 1, &[key]),
        ];
        let forward = group(&entries);
        entries.reverse();
        let backward = group(&entries);
        assert_eq!(forward.assignments, backward.assignments);

        // Apply the grouping, then drop the shared evidence: each record
        // goes back to its own opportunity.
        for e in &mut entries {
            if let Some((_, opp)) = forward.assignments.iter().find(|(j, _)| *j == e.job) {
                e.opportunity = *opp;
            }
            e.evidence.clear();
        }
        let split = group(&entries);
        assert_eq!(split.assignments.len(), 1);
        assert_eq!(split.multi_source_groups, 0);
    }
}
