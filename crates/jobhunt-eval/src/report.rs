//! The benchmark report, in Markdown: readable in a terminal, and committed
//! as the recorded baseline (`baseline/recommendation.md`).
//!
//! Everything in it is deterministic: fixtures are read in a fixed order,
//! the clock is fixed, and no map is iterated in hash order.

use std::fmt::Write;

use jobhunt_eligibility::RULES_VERSION;
use jobhunt_ranking::RANKING_VERSION;

use crate::build;
use crate::fixture::{Fixtures, Group};
use crate::metrics::Metrics;
use crate::run::{CandidateRun, Case, Run};
use crate::taxonomy::{Contradiction, Fit, Practicality};

/// How much of each case to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Options {
    /// Every case's reasons, not just the failures'.
    pub details: bool,
}

fn fit(f: Fit) -> &'static str {
    match f {
        Fit::StrongYes => "strong yes",
        Fit::Maybe => "maybe",
        Fit::No => "no",
    }
}

fn practicality(p: Practicality) -> &'static str {
    match p {
        Practicality::Valid => "valid",
        Practicality::Unknown => "unknown",
        Practicality::Concern => "concern",
        Practicality::Impossible => "impossible",
    }
}

fn yes(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn list<T: ToString>(items: &[T]) -> String {
    if items.is_empty() {
        "—".to_owned()
    } else {
        items
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// A table cell: pipes escaped, on one line.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

fn metrics_table(out: &mut String, m: &Metrics, per_candidate: Option<&[(String, Metrics)]>) {
    let _ = writeln!(out, "| Metric | Value |");
    let _ = writeln!(out, "| --- | --- |");
    let rows = [
        (
            "Strict Today precision (Strong yes only)",
            m.strict_precision.to_string(),
        ),
        (
            "Relaxed Today precision (Strong yes + Maybe)",
            m.relaxed_precision.to_string(),
        ),
        (
            "Actionable precision (fits enough, no known practical problem)",
            m.actionable_precision.to_string(),
        ),
        (
            "Obvious false positives (No or Impossible in Today)",
            m.obvious_false_positives.to_string(),
        ),
        ("Maybes in Today", m.maybes_surfaced.to_string()),
        (
            "Practical Strong yes surfaced",
            m.strong_yes_surfaced.to_string(),
        ),
        (
            "Surfaced only because of pay",
            m.surfaced_on_pay.to_string(),
        ),
        (
            "Pay ranges for another location read as meeting the candidate's pay",
            m.foreign_range_read_as_met.to_string(),
        ),
        (
            "Surfaced with eligibility unconfirmed",
            m.surfaced_unconfirmed.to_string(),
        ),
        (
            "Density: labeled worth your attention",
            format!("{} of {} judged", m.surfaced, m.judged),
        ),
        (
            "Density: on the feed (one per company, at most 5)",
            m.on_feed.to_string(),
        ),
        (
            "Feed precision, strict / relaxed",
            format!("{} / {}", m.feed_strict_precision, m.feed_relaxed_precision),
        ),
        ("Cases failing their judgment", m.failures.to_string()),
    ];
    for (name, value) in rows {
        let _ = writeln!(out, "| {name} | {value} |");
    }
    if let Some(each) = per_candidate {
        let densities: Vec<String> = each
            .iter()
            .map(|(id, m)| format!("{id} {}", m.surfaced))
            .collect();
        let _ = writeln!(
            out,
            "| Density per candidate (labeled worth your attention) | {} |",
            densities.join(" · ")
        );
    }
}

fn contradictions_table(out: &mut String, m: &Metrics) {
    let _ = writeln!(
        out,
        "| Contradiction | Cases | Surfaced anyway (miss rate) | No ranker signal for it |"
    );
    let _ = writeln!(out, "| --- | --- | --- | --- |");
    for c in &m.contradictions {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} |",
            c.kind,
            c.cases,
            c.miss_rate(),
            c.undetected
        );
    }
}

fn reasons_of(case: &Case) -> String {
    list(&case.judgment.reasons)
}

/// The current ranker's own reasons: what it found for, against, and
/// doesn't know.
fn current_reasons(out: &mut String, case: &Case) {
    let o = &case.observed;
    for w in &o.worth {
        let _ = writeln!(out, "  - \\+ {}", cell(w));
    }
    for c in &o.caveats {
        let _ = writeln!(out, "  - − {}", cell(c));
    }
    for u in &o.unknowns {
        let _ = writeln!(out, "  - ? {}", cell(u));
    }
    if o.worth.is_empty() && o.caveats.is_empty() && o.unknowns.is_empty() {
        let _ = writeln!(out, "  - (no reasons)");
    }
}

fn case_detail(out: &mut String, candidate: &str, case: &Case) {
    let o = &case.observed;
    let _ = writeln!(
        out,
        "#### {} — {} (`{}`, for `{candidate}`)\n",
        cell(&case.company),
        cell(&case.title),
        case.job
    );
    let _ = writeln!(
        out,
        "- Expected: **{}** (fit {}, practicality {}); Today: {}",
        case.label(),
        fit(case.judgment.fit),
        practicality(case.judgment.practicality),
        case.judgment.today().as_str()
    );
    let _ = writeln!(out, "- Expected reasons: {}", reasons_of(case));
    let _ = writeln!(out, "- Why: {}", cell(&case.judgment.why));
    let _ = writeln!(
        out,
        "- Actual: gate {}{}, tier {}, score {:.2}",
        o.gate,
        o.gate_why
            .as_deref()
            .filter(|w| !w.is_empty())
            .map(|w| format!(" ({})", cell(w)))
            .unwrap_or_default(),
        o.tier.label(),
        o.score
    );
    let _ = writeln!(
        out,
        "- Today: labeled worth your attention: **{}**; on the feed: {}",
        yes(o.qualifies),
        yes(o.in_feed)
    );
    let _ = writeln!(out, "- Result: **{}**", case.verdict().as_str());
    let _ = writeln!(
        out,
        "- Contradictions named: {}; surfaced despite: {}; no ranker signal for: {}",
        list(&case.contradictions()),
        list(&case.missed()),
        list(&case.undetected())
    );
    let _ = writeln!(
        out,
        "- Practical classification: expected {}; eligibility {:?}; pay read as meeting what the candidate wants: {}{}",
        practicality(case.judgment.practicality),
        o.eligibility,
        yes(o.pay_read_as_met),
        if o.pay_carried {
            "; surfaced only because of pay"
        } else {
            ""
        }
    );
    let _ = writeln!(out, "- Current reasons:");
    current_reasons(out, case);
    let _ = writeln!(out);
}

fn candidate_section(out: &mut String, c: &CandidateRun, options: Options) {
    let m = Metrics::of(&c.cases);
    let _ = writeln!(out, "## Candidate `{}`\n", c.id);
    let _ = writeln!(out, "{}\n", c.summary.trim());
    let r = &c.reading;
    let _ = writeln!(out, "What the current ranker reads:\n");
    let _ = writeln!(
        out,
        "- location: {}; latest level: {}",
        r.location.as_deref().unwrap_or("—"),
        r.level.as_deref().unwrap_or("—")
    );
    let _ = writeln!(out, "- role experience: {}", list(&r.roles));
    let _ = writeln!(out, "- technologies: {}", list(&r.technologies));
    let _ = writeln!(out, "- stated preferences: {}", list(&r.stated));
    if !c.unexpressed.is_empty() {
        let _ = writeln!(out, "\nWhat the current preference model can't express:\n");
        for u in &c.unexpressed {
            let _ = writeln!(out, "- {}", u.trim());
        }
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "| Case | Group | Expected | Fit / practicality | Gate | Tier | Today? | Result |"
    );
    let _ = writeln!(out, "| --- | --- | --- | --- | --- | --- | --- | --- |");
    for case in &c.cases {
        let o = &case.observed;
        let _ = writeln!(
            out,
            "| {} — {} | {} | {} | {} / {} | {} | {} | {}{} | {} |",
            cell(&case.company),
            cell(&case.title),
            case.group.as_str(),
            case.label(),
            fit(case.judgment.fit),
            practicality(case.judgment.practicality),
            o.gate,
            o.tier.label(),
            yes(o.qualifies),
            if o.in_feed { " (feed)" } else { "" },
            case.verdict().as_str()
        );
    }
    let _ = writeln!(out);
    metrics_table(out, &m, None);
    let _ = writeln!(out);
    let feed: Vec<String> = c.feed.iter().map(|j| format!("`{j}`")).collect();
    let _ = writeln!(
        out,
        "Simulated feed, best first: {}\n",
        if feed.is_empty() {
            "empty (caught up)".to_owned()
        } else {
            feed.join(", ")
        }
    );
    let shown: Vec<&Case> = c
        .cases
        .iter()
        .filter(|case| options.details || case.verdict().failed())
        .collect();
    if !shown.is_empty() {
        let _ = writeln!(
            out,
            "### {}\n",
            if options.details {
                "Every case"
            } else {
                "Failing cases"
            }
        );
        for case in shown {
            case_detail(out, &c.id, case);
        }
    }
}

fn pairs_section(out: &mut String, run: &Run) {
    let _ = writeln!(out, "## Contrastive pairs\n");
    let _ = writeln!(
        out,
        "Each pair differs in one meaningful way; the candidate's judgments should differ with it, and so should the ranking.\n"
    );
    let _ = writeln!(
        out,
        "| Pair | Candidate | Job | Expected | Gate | Tier | Score | Today? | Result |"
    );
    let _ = writeln!(
        out,
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- |"
    );
    let mut rows: Vec<(&str, &str, &Case)> = Vec::new();
    for c in &run.candidates {
        for case in &c.cases {
            if let Some(pair) = &case.pair {
                rows.push((pair, &c.id, case));
            }
        }
    }
    rows.sort_by(|a, b| a.0.cmp(b.0).then(a.1.cmp(b.1)).then(a.2.job.cmp(&b.2.job)));
    for (pair, candidate, case) in rows {
        let o = &case.observed;
        let _ = writeln!(
            out,
            "| {pair} | {candidate} | `{}` | {} | {} | {} | {:.2} | {} | {} |",
            case.job,
            case.label(),
            o.gate,
            o.tier.label(),
            o.score,
            yes(o.qualifies),
            case.verdict().as_str()
        );
    }
    let _ = writeln!(out);
}

fn golden_section(out: &mut String, run: &Run) {
    let _ = writeln!(out, "## Golden cases\n");
    let _ = writeln!(
        out,
        "The production failures this benchmark exists for, on the synthetic candidate standing in for the person they were observed on.\n"
    );
    let mut observed: Vec<(&str, &Case)> = Vec::new();
    let mut others: Vec<(&str, &Case)> = Vec::new();
    for c in &run.candidates {
        for case in c.cases.iter().filter(|case| case.group == Group::Golden) {
            if case.as_observed {
                observed.push((&c.id, case));
            } else {
                others.push((&c.id, case));
            }
        }
    }
    let _ = writeln!(
        out,
        "| Case | Expected | Today? | Result | Contradiction missed | Practical classification |"
    );
    let _ = writeln!(out, "| --- | --- | --- | --- | --- | --- |");
    for (_, case) in &observed {
        let o = &case.observed;
        let _ = writeln!(
            out,
            "| {} — {} | {} | {} | {} | {} | expected {}; gate {} |",
            cell(&case.company),
            cell(&case.title),
            case.label(),
            yes(o.qualifies),
            case.verdict().as_str(),
            list(&case.missed()),
            practicality(case.judgment.practicality),
            o.gate
        );
    }
    let _ = writeln!(out);
    for (candidate, case) in &observed {
        case_detail(out, candidate, case);
    }
    if !others.is_empty() {
        let _ = writeln!(
            out,
            "### The same postings, for the other candidates\n\nFit is the candidate's, not the job's.\n"
        );
        let _ = writeln!(
            out,
            "| Job | Candidate | Expected | Gate | Tier | Today? | Result |"
        );
        let _ = writeln!(out, "| --- | --- | --- | --- | --- | --- | --- |");
        others.sort_by(|a, b| a.1.job.cmp(&b.1.job).then(a.0.cmp(b.0)));
        for (candidate, case) in others {
            let o = &case.observed;
            let _ = writeln!(
                out,
                "| `{}` | {candidate} | {} | {} | {} | {} | {} |",
                case.job,
                case.label(),
                o.gate,
                o.tier.label(),
                yes(o.qualifies),
                case.verdict().as_str()
            );
        }
        let _ = writeln!(out);
    }
}

/// The whole report.
pub fn render(fixtures: &Fixtures, run: &Run, options: Options) -> String {
    let mut out = String::new();
    let judged: usize = run.candidates.iter().map(|c| c.cases.len()).sum();
    let _ = writeln!(out, "# Recommendation benchmark\n");
    let _ = writeln!(
        out,
        "Generated by `cargo run -p jobhunt-eval --bin narrow-eval -- recommendation-benchmark`. Deterministic: fixed fixtures, fixed clock ({}), no network, no model calls.\n",
        build::now().format("%Y-%m-%dT%H:%MZ")
    );
    let _ = writeln!(
        out,
        "- Ranker: `RANKING_VERSION` {RANKING_VERSION}, eligibility `RULES_VERSION` {RULES_VERSION}, no learned taste (no feedback yet)"
    );
    let _ = writeln!(
        out,
        "- Fixtures: {} candidates, {} jobs, {judged} judgments",
        run.candidates.len(),
        fixtures.jobs.len()
    );
    let _ = writeln!(
        out,
        "- Surfaced = labeled worth your attention: not excluded, tier at least worth reviewing (what Today selects from). Feed = the best of each company, at most 5."
    );
    let _ = writeln!(
        out,
        "- Results: pass; FAIL: false positive (No or Impossible surfaced); FAIL: maybe in Today; FAIL: missed (a practical Strong yes not surfaced). A Strong yes with a known practical concern passes either way.\n"
    );
    let all: Vec<&Case> = run.candidates.iter().flat_map(|c| &c.cases).collect();
    let overall = Metrics::of(all.iter().copied());
    let per: Vec<(String, Metrics)> = run
        .candidates
        .iter()
        .map(|c| (c.id.clone(), Metrics::of(&c.cases)))
        .collect();
    let _ = writeln!(out, "## Summary, all candidates\n");
    metrics_table(&mut out, &overall, Some(&per));
    let _ = writeln!(out);
    let _ = writeln!(out, "### Contradictions\n");
    contradictions_table(&mut out, &overall);
    let _ = writeln!(out);
    let _ = writeln!(out, "### By group\n");
    let _ = writeln!(out, "| Group | Cases | Failing | Surfaced |");
    let _ = writeln!(out, "| --- | --- | --- | --- |");
    for group in [
        Group::Golden,
        Group::Positive,
        Group::Contrastive,
        Group::Compensation,
        Group::Practicality,
    ] {
        let m = Metrics::of(all.iter().copied().filter(|c| c.group == group));
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} |",
            group.as_str(),
            m.judged,
            m.failures,
            m.surfaced
        );
    }
    let _ = writeln!(out);
    golden_section(&mut out, run);
    pairs_section(&mut out, run);
    for c in &run.candidates {
        candidate_section(&mut out, c, options);
    }
    let _ = writeln!(out, "## Contradiction kinds\n");
    for kind in Contradiction::ALL {
        let _ = writeln!(
            out,
            "- **{kind}**: {}",
            match kind {
                Contradiction::Seniority => "seniority_mismatch",
                Contradiction::RoleDepth =>
                    "specialization_too_deep, technical_depth_mismatch, engineering_work_mismatch",
                Contradiction::Eligibility =>
                    "geography_invalid, work_authorization_invalid, remote_scope_invalid, relocation_invalid, timezone_invalid",
                Contradiction::CompanyShape =>
                    "company_shape_mismatch, large_company_mismatch, team_shape_mismatch",
            }
        );
    }
    out
}
