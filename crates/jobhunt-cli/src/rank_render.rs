//! Output for `rank`, `why`, `taste`, `pipeline`, `feedback` and the
//! feedback commands.

use std::io::{self, Write};

use anstyle::{AnsiColor, Style};
use chrono::{DateTime, Utc};
use jobhunt_ranking::reason::{ReadSignal, Target};
use jobhunt_ranking::signals::{SignalKind, clip, describe_evidence};
use jobhunt_ranking::{
    Direction, FeedbackEvent, Gate, LearnedTaste, Person, PipelineEntry, RankReport, Ranking,
    Recorded, Stage, TasteModel, TasteStatus, Tier,
};

use crate::render::{DIM, TITLE, plural};

const GOOD: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Green)));
const OPEN: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Yellow)));
const BAD: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Red)));
const HEADING: Style = Style::new().bold();

fn heading(out: &mut impl Write, text: &str) -> io::Result<()> {
    writeln!(out, "{HEADING}{text}{HEADING:#}")
}

fn tier_style(tier: Tier) -> Style {
    match tier {
        Tier::StrongFit => GOOD.bold(),
        Tier::WorthReviewing => GOOD,
        Tier::Maybe => OPEN,
        Tier::LowPriority => DIM,
    }
}

fn marker(kind: SignalKind) -> (&'static str, Style) {
    match kind {
        SignalKind::Plus => ("+", GOOD),
        SignalKind::Minus => ("-", BAD),
        SignalKind::Condition => ("~", OPEN),
        SignalKind::Unknown => ("?", OPEN),
        SignalKind::Blocker => ("✗", BAD),
        SignalKind::Context => ("•", Style::new()),
    }
}

fn date(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%d").to_string()
}

/// The sections `rank` prints, in order.
fn section(gate: &Gate) -> &'static str {
    match gate {
        Gate::Recommended => "Worth your attention",
        Gate::VerifyFirst { .. } => "Promising, but verify before spending time",
        Gate::EligibilityUnclear { .. } => "Could be worth it, if you can take it",
        Gate::Excluded { .. } => "Not recommended",
    }
}

/// `rank`: the few jobs worth attention, grouped by gate, each with its
/// short brief.
pub fn rank_list(
    out: &mut impl Write,
    report: &RankReport,
    limit: usize,
    all: bool,
) -> io::Result<()> {
    let shown: Vec<&Ranking> = report
        .rankings
        .iter()
        .filter(|r| all || r.tier >= Tier::WorthReviewing)
        .take(limit)
        .collect();
    let hidden_tiers = report
        .rankings
        .iter()
        .filter(|r| r.tier < Tier::WorthReviewing)
        .count();
    if shown.is_empty() {
        if report.rankings.is_empty() {
            writeln!(
                out,
                "Nothing to recommend among {}.",
                plural(
                    report.considered as u64,
                    "stored open job",
                    "stored open jobs"
                )
            )?;
        } else {
            writeln!(
                out,
                "No job stands out yet: {} rank as maybe or low priority (--all shows them).",
                plural(hidden_tiers as u64, "job", "jobs")
            )?;
        }
    }
    let mut current: Option<&'static str> = None;
    for (i, r) in shown.iter().enumerate() {
        let name = section(&r.gate);
        if current != Some(name) {
            if current.is_some() {
                writeln!(out)?;
            }
            heading(out, name)?;
            current = Some(name);
        }
        let s = tier_style(r.tier);
        writeln!(
            out,
            " {DIM}{:>2}.{DIM:#} {TITLE}{}{TITLE:#} — {}   {s}{}{s:#}",
            i + 1,
            r.title,
            r.company,
            r.tier.label()
        )?;
        writeln!(out, "     {DIM}{}{DIM:#}", r.brief.summary)?;
        let lines = r
            .brief
            .worth
            .iter()
            .take(3)
            .map(|l| (SignalKind::Plus, l))
            .chain(
                r.brief
                    .caveats
                    .iter()
                    .take(2)
                    .map(|l| (SignalKind::Minus, l)),
            )
            .chain(
                r.brief
                    .unknowns
                    .iter()
                    .take(1)
                    .map(|l| (SignalKind::Unknown, l)),
            );
        for (kind, line) in lines {
            let (m, st) = marker(kind);
            writeln!(out, "     {st}{m}{st:#} {line}")?;
        }
        let next = match &r.gate {
            Gate::VerifyFirst { .. } => format!("jobhunt verify {}", r.opportunity),
            Gate::EligibilityUnclear { .. } => format!("jobhunt check {}", r.opportunity),
            _ => format!("jobhunt why {}", r.opportunity),
        };
        writeln!(out, "     {DIM}{} · {next}{DIM:#}", r.opportunity)?;
    }
    writeln!(out)?;
    let e = &report.excluded;
    let mut not_shown: Vec<String> = Vec::new();
    let mut add = |n: usize, one: &str, many: &str| {
        if n > 0 {
            not_shown.push(plural(n as u64, one, many));
        }
    };
    add(e.ineligible, "job you can't take", "jobs you can't take");
    add(
        e.below_minimum,
        "job below your pay minimum",
        "jobs below your pay minimum",
    );
    add(e.rejected, "you rejected", "you rejected");
    add(e.in_pipeline, "in your pipeline", "in your pipeline");
    add(e.closed, "closed", "closed");
    if !all {
        add(
            hidden_tiers,
            "maybe or low priority (--all)",
            "maybe or low priority (--all)",
        );
    }
    let beyond = report
        .rankings
        .iter()
        .filter(|r| all || r.tier >= Tier::WorthReviewing)
        .count()
        .saturating_sub(shown.len());
    add(beyond, "more beyond --limit", "more beyond --limit");
    if !not_shown.is_empty() {
        writeln!(out, "{DIM}Not shown: {}.{DIM:#}", not_shown.join(" · "))?;
    }
    let taste = &report.taste;
    let learned = taste.active().count();
    writeln!(
        out,
        "{DIM}Ranked {} against your profile{}. Tiers are coarse on purpose; \
         `jobhunt why <id>` shows every reason.{DIM:#}",
        plural(
            report.considered as u64,
            "stored open job",
            "stored open jobs"
        ),
        if taste.events > 0 {
            format!(
                ", {} and {} learned from it",
                plural(
                    taste.events as u64,
                    "piece of feedback",
                    "pieces of feedback"
                ),
                plural(learned as u64, "pattern", "patterns")
            )
        } else {
            String::new()
        }
    )?;
    if !report.person.has_preferences() {
        writeln!(
            out,
            "{DIM}Say what you want (jobhunt preferences add \"…\") and give feedback \
             (jobhunt save|reject <id> --reason \"…\") to sharpen this.{DIM:#}"
        )?;
    }
    out.flush()
}

/// `why`: the decision brief of one job.
pub fn brief(
    out: &mut impl Write,
    ranking: &Ranking,
    details: bool,
    reused: bool,
) -> io::Result<()> {
    let b = &ranking.brief;
    writeln!(
        out,
        "{TITLE}{} — {}{TITLE:#}",
        ranking.title, ranking.company
    )?;
    writeln!(out, "{DIM}{} · {}{DIM:#}", ranking.job, ranking.opportunity)?;
    writeln!(out)?;
    let s = tier_style(ranking.tier);
    let label = match &ranking.gate {
        Gate::Excluded { .. } => "NOT RECOMMENDED".to_owned(),
        _ => ranking.tier.label().to_uppercase(),
    };
    writeln!(out, "{s}{label}{s:#}")?;
    writeln!(out, "{}", b.verdict)?;
    writeln!(out, "{DIM}{}{DIM:#}", b.summary)?;
    let block = |out: &mut dyn Write, title: &str, kind: SignalKind, lines: &[String]| {
        if lines.is_empty() {
            return Ok(());
        }
        writeln!(out)?;
        writeln!(out, "{HEADING}{title}{HEADING:#}")?;
        let (m, st) = marker(kind);
        for line in lines {
            writeln!(out, "  {st}{m}{st:#} {line}")?;
        }
        io::Result::Ok(())
    };
    block(
        out,
        "Why it may be worth your time",
        SignalKind::Plus,
        &b.worth,
    )?;
    block(out, "Caveats", SignalKind::Minus, &b.caveats)?;
    block(out, "Unknown", SignalKind::Unknown, &b.unknowns)?;
    block(out, "Your history with it", SignalKind::Context, &b.history)?;
    if details {
        writeln!(out)?;
        heading(out, "Every signal")?;
        for s in &ranking.signals {
            writeln!(
                out,
                "  {DIM}{:<12} {:>+6.2}  {:<26}{DIM:#} {}",
                s.group.as_str(),
                s.weight,
                s.basis.as_str(),
                s.summary
            )?;
            for e in &s.evidence {
                writeln!(out, "  {DIM}{:<48}{}{DIM:#}", "", clip(e, 120))?;
            }
        }
        writeln!(
            out,
            "  {DIM}Score {:.2}: orders jobs within a tier; it is not a match percentage.{DIM:#}",
            ranking.score
        )?;
        writeln!(
            out,
            "  {DIM}Ranking rules {} · {} · {}{DIM:#}",
            ranking.ranking_version,
            ranking.taste_digest,
            if reused {
                "the stored ranking for these exact inputs"
            } else {
                "computed now"
            }
        )?;
    }
    writeln!(out)?;
    let id = ranking.opportunity;
    writeln!(
        out,
        "{DIM}Decide: jobhunt save|reject|like|dislike {id} --reason \"…\" · evidence: jobhunt check {id}{DIM:#}"
    )?;
    if !details {
        writeln!(
            out,
            "{DIM}Every signal with its evidence: jobhunt why --details {id}{DIM:#}"
        )?;
    }
    out.flush()
}

/// How a reason was read, in words.
pub fn reading(signal: &ReadSignal) -> String {
    let what = match &signal.target {
        Target::Key { key } => key.label(),
        Target::Job { reference } => reference.label().to_owned(),
    };
    let scope = match signal.scope {
        jobhunt_ranking::reason::Scope::General => "",
        jobhunt_ranking::reason::Scope::ThisOpportunity => " (this job only)",
    };
    format!("{} {what}{scope}", signal.direction.as_str())
}

/// What a feedback command did.
pub fn recorded(out: &mut impl Write, r: &Recorded) -> io::Result<()> {
    let e = &r.event;
    let verb = e.action.past();
    writeln!(
        out,
        "{} {TITLE}{} — {}{TITLE:#}",
        capitalize(verb),
        e.title,
        e.company
    )?;
    writeln!(out, "{DIM}{} · {}{DIM:#}", e.opportunity, e.id)?;
    if let (Some(reason), Some(reading)) = (&e.reason, &r.reading) {
        writeln!(out, "Reason: “{reason}”")?;
        if reading.is_unread() {
            writeln!(
                out,
                "{DIM}Kept as written: JobHunt didn't recognize anything to learn from in it.{DIM:#}"
            )?;
        } else {
            let read: Vec<String> = reading.signals.iter().map(self::reading).collect();
            writeln!(out, "Read as: {}", read.join("; "))?;
        }
    }
    let before = state_text(r.before.stage, r.before.sentiment);
    let after = state_text(r.after.stage, r.after.sentiment);
    if before == after {
        writeln!(out, "Status: {after} (unchanged)")?;
    } else {
        writeln!(out, "Status: {after} (was {before})")?;
    }
    if r.after.stage == Stage::Rejected {
        writeln!(
            out,
            "{DIM}It won't be recommended again. Changed your mind: jobhunt save {}{DIM:#}",
            e.opportunity
        )?;
    }
    out.flush()
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn state_text(stage: Stage, sentiment: Option<jobhunt_ranking::Sentiment>) -> String {
    let mut text = stage.as_str().to_owned();
    match sentiment {
        Some(jobhunt_ranking::Sentiment::Liked) => text.push_str(", liked"),
        Some(jobhunt_ranking::Sentiment::Disliked) => text.push_str(", disliked"),
        None => {}
    }
    text
}

fn learned_block(out: &mut impl Write, l: &LearnedTaste, evidence: usize) -> io::Result<()> {
    let (arrow, st) = match (&l.status, l.direction) {
        (TasteStatus::Mixed, _) => ("~", OPEN),
        (_, Direction::Prefer) => ("↑", GOOD),
        (_, Direction::Avoid) => ("↓", BAD),
    };
    let status = match &l.status {
        TasteStatus::Active { confidence } => confidence.as_str().to_owned(),
        TasteStatus::Mixed => "contradictory, not used".to_owned(),
        TasteStatus::NotEnough => "not enough evidence yet".to_owned(),
        TasteStatus::Explicit { preference, agrees } => format!(
            "you said “{preference}”{}",
            if *agrees {
                ", which your feedback agrees with"
            } else {
                "; your feedback leans the other way, and what you said wins"
            }
        ),
    };
    writeln!(
        out,
        "  {st}{arrow}{st:#} {} — {status}",
        capitalize(&l.key.label())
    )?;
    writeln!(
        out,
        "    {DIM}{}{}; last reinforced {}{DIM:#}",
        l.basis(),
        if l.opportunities > 1 {
            format!(" across {} jobs", l.opportunities)
        } else {
            String::new()
        },
        date(l.last_reinforced)
    )?;
    for e in l.support.iter().take(evidence) {
        writeln!(out, "    {DIM}for:{DIM:#} {}", describe_evidence(e))?;
    }
    for e in l.against.iter().take(evidence) {
        writeln!(out, "    {DIM}against:{DIM:#} {}", describe_evidence(e))?;
    }
    Ok(())
}

/// `taste`: explicit preferences, learned patterns with evidence, and what
/// couldn't be read.
pub fn taste(
    out: &mut impl Write,
    model: &TasteModel,
    person: &Person,
    all: bool,
) -> io::Result<()> {
    heading(out, "What you told JobHunt (this always wins)")?;
    let mut said: Vec<String> = person
        .stated
        .iter()
        .map(|p| format!("{} {}", p.stance.as_str(), p.text))
        .collect();
    said.extend(
        person
            .pay
            .iter()
            .map(|p| format!("{} {}", p.stance.as_str(), p.text)),
    );
    said.extend(
        person
            .work_modes
            .iter()
            .map(|(m, s)| format!("{} {} work", s.as_str(), m.as_str())),
    );
    if said.is_empty() {
        writeln!(
            out,
            "  Nothing yet. jobhunt preferences add \"I want backend roles at small companies; avoid pure SRE\""
        )?;
    }
    for line in said {
        writeln!(out, "  • {line}")?;
    }
    writeln!(out)?;
    heading(
        out,
        &format!(
            "What JobHunt learned from your feedback ({} on {})",
            plural(model.events as u64, "action", "actions"),
            plural(model.opportunities as u64, "job", "jobs")
        ),
    )?;
    let active: Vec<&LearnedTaste> = model.active().collect();
    if active.is_empty() {
        writeln!(
            out,
            "  Nothing yet. Patterns appear after a reason in your words, or after several \
             jobs you treated the same way."
        )?;
    }
    for l in &active {
        learned_block(out, l, if all { usize::MAX } else { 2 })?;
    }
    let of = |pred: fn(&TasteStatus) -> bool| -> Vec<&LearnedTaste> {
        model.learned.iter().filter(|l| pred(&l.status)).collect()
    };
    let mixed = of(|s| matches!(s, TasteStatus::Mixed));
    if !mixed.is_empty() {
        writeln!(out)?;
        heading(out, "Contradictory, so not used")?;
        for l in mixed {
            learned_block(out, l, if all { usize::MAX } else { 2 })?;
        }
    }
    let explicit = of(|s| matches!(s, TasteStatus::Explicit { .. }));
    if !explicit.is_empty() {
        writeln!(out)?;
        heading(out, "Covered by what you said")?;
        for l in explicit {
            learned_block(out, l, if all { usize::MAX } else { 1 })?;
        }
    }
    let weak = of(|s| matches!(s, TasteStatus::NotEnough));
    if !weak.is_empty() {
        writeln!(out)?;
        if all {
            heading(out, "Not enough evidence yet")?;
            for l in weak {
                learned_block(out, l, usize::MAX)?;
            }
        } else {
            writeln!(
                out,
                "{DIM}{} with too little evidence to use yet (--all lists them).{DIM:#}",
                plural(weak.len() as u64, "pattern", "patterns")
            )?;
        }
    }
    if !model.notes.is_empty() {
        writeln!(out)?;
        heading(out, "About single jobs (not generalized)")?;
        for n in &model.notes {
            writeln!(
                out,
                "  • “{}” — {} {} ({}; {})",
                n.reason,
                n.action.past(),
                n.opportunity,
                n.key.value_label(),
                date(n.at)
            )?;
        }
    }
    if !model.unread.is_empty() {
        writeln!(out)?;
        heading(out, "Kept as written (nothing recognized)")?;
        for u in &model.unread {
            writeln!(
                out,
                "  • “{}” — {} {} at {}, {}",
                u.reason,
                u.action.past(),
                u.title,
                u.company,
                date(u.at)
            )?;
        }
    }
    writeln!(out)?;
    writeln!(
        out,
        "{DIM}Reasons are read with {}; every one is kept verbatim (jobhunt feedback).{DIM:#}",
        model.reader
    )?;
    out.flush()
}

/// `pipeline`.
pub fn pipeline(out: &mut impl Write, entries: &[PipelineEntry]) -> io::Result<()> {
    if entries.is_empty() {
        writeln!(
            out,
            "Nothing in your pipeline yet. Save a job with `jobhunt save <id>`, or record an \
             application with `jobhunt applied <id>`."
        )?;
        return out.flush();
    }
    let mut current: Option<Stage> = None;
    for e in entries {
        if current != Some(e.state.stage) {
            if current.is_some() {
                writeln!(out)?;
            }
            let name = match e.state.stage {
                Stage::Offer => "Offers",
                Stage::Interviewing => "Interviewing",
                Stage::Applied => "Applied",
                Stage::Saved => "Saved",
                Stage::Rejected => "Rejected",
                Stage::Seen | Stage::Unseen => "Seen",
            };
            heading(out, name)?;
            current = Some(e.state.stage);
        }
        let since = e.state.since().map(date).unwrap_or_default();
        let liked = match e.state.sentiment {
            Some(jobhunt_ranking::Sentiment::Liked) => " · liked",
            Some(jobhunt_ranking::Sentiment::Disliked) => " · disliked",
            None => "",
        };
        let closed = if e.closed { " · listing closed" } else { "" };
        writeln!(
            out,
            "  {TITLE}{}{TITLE:#} — {} {DIM}· since {since}{liked}{closed} · {}{DIM:#}",
            e.title, e.company, e.opportunity
        )?;
        if let Some(reason) = e
            .state
            .events
            .iter()
            .rev()
            .find_map(|ev| ev.reason.as_ref())
        {
            writeln!(out, "    “{reason}”")?;
        }
    }
    out.flush()
}

/// `feedback`: every event, verbatim.
pub fn feedback_log(out: &mut impl Write, events: &[FeedbackEvent]) -> io::Result<()> {
    // Looking at a job is state, not feedback.
    let events: Vec<&FeedbackEvent> = events
        .iter()
        .filter(|e| e.action != jobhunt_ranking::FeedbackAction::Seen)
        .collect();
    if events.is_empty() {
        writeln!(out, "No feedback yet.")?;
        return out.flush();
    }
    for e in events {
        writeln!(
            out,
            "{DIM}{}{DIM:#}  {:<10} {} — {}{}",
            e.at.format("%Y-%m-%d %H:%M"),
            e.action.as_str(),
            e.title,
            e.company,
            e.reason
                .as_ref()
                .map(|r| format!("  “{r}”"))
                .unwrap_or_default()
        )?;
        writeln!(
            out,
            "{DIM}                  {} · {}{DIM:#}",
            e.opportunity, e.id
        )?;
    }
    out.flush()
}
