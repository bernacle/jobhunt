//! Output for `why`, `taste`, `pipeline`, `feedback` and the feedback
//! commands (the shortlist itself is `find`'s).

use std::io::{self, Write};

use anstyle::{AnsiColor, Style};
use chrono::{DateTime, Utc};
use jobhunt_app::feedback::{FeedbackOutcome, describe_signal};
use jobhunt_ranking::signals::{SignalKind, clip, describe_evidence};
use jobhunt_ranking::{
    Direction, FeedbackEvent, Gate, LearnedTaste, Person, PipelineEntry, Ranking, ReviewState,
    Stage, TasteModel, TasteStatus, Tier,
};

use crate::render::{DIM, TITLE, plural};

pub(crate) const GOOD: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Green)));
pub(crate) const OPEN: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Yellow)));
pub(crate) const BAD: Style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Red)));
pub(crate) const HEADING: Style = Style::new().bold();

fn heading(out: &mut impl Write, text: &str) -> io::Result<()> {
    writeln!(out, "{HEADING}{text}{HEADING:#}")
}

pub(crate) fn tier_style(tier: Tier) -> Style {
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
    block(out, "Things to check", SignalKind::Unknown, &b.unknowns)?;
    block(out, "Your history with it", SignalKind::Context, &b.history)?;
    if details {
        let f = &ranking.fit;
        writeln!(out)?;
        heading(out, "How the fit was assessed")?;
        writeln!(
            out,
            "  Fit {}: the work {:.2}, the rest {:.2} · {} · {}",
            f.level.as_str(),
            f.role_fit,
            f.support,
            f.assessor,
            match &f.review {
                ReviewState::NotReviewed => "not reviewed by a model".to_owned(),
                ReviewState::Reviewed { by, changed: true } =>
                    format!("reviewed by {by}, which changed it"),
                ReviewState::Reviewed { by, changed: false } => format!("reviewed by {by}"),
                ReviewState::Unavailable { why } => {
                    format!("the model review was unavailable ({why}); the rules decided")
                }
            }
        )?;
        for r in &f.reasons {
            writeln!(
                out,
                "  {GOOD}+{GOOD:#} {DIM}{:<14} {:<8} {:>4.2}{DIM:#} {}{}",
                r.aspect.as_str(),
                format!("{:?}", r.firmness).to_lowercase(),
                r.weight,
                r.text,
                if r.folded {
                    " (said with the role)"
                } else {
                    ""
                }
            )?;
        }
        for c in &f.contradictions {
            writeln!(
                out,
                "  {BAD}-{BAD:#} {DIM}{:<14} {:<8} {:<8}{DIM:#} {}",
                c.kind.as_str(),
                format!("{:?}", c.firmness).to_lowercase(),
                c.severity.as_str(),
                c.text
            )?;
        }
        for u in &f.uncertainties {
            writeln!(out, "  {OPEN}?{OPEN:#} {u}")?;
        }
        let p = &ranking.practicality;
        writeln!(out)?;
        heading(out, "Practicality")?;
        writeln!(out, "  {}", p.status.as_str())?;
        for fact in &p.facts {
            writeln!(out, "  • {fact}")?;
        }
        writeln!(out)?;
        heading(out, "Every signal")?;
        for s in &ranking.signals {
            writeln!(
                out,
                "  {DIM}{:<12} {:<26}{DIM:#} {}",
                s.group.as_str(),
                s.basis.as_str(),
                s.summary
            )?;
            for e in &s.evidence {
                writeln!(out, "  {DIM}{:<40}{}{DIM:#}", "", clip(e, 120))?;
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
        "{DIM}Decide: narrow save|reject|like|dislike {id} --reason \"…\" · evidence: narrow check {id}{DIM:#}"
    )?;
    if !details {
        writeln!(
            out,
            "{DIM}Every signal with its evidence: narrow why --details {id}{DIM:#}"
        )?;
    }
    out.flush()
}

/// What a feedback command did.
pub fn recorded(out: &mut impl Write, outcome: &FeedbackOutcome) -> io::Result<()> {
    let r = &outcome.recorded;
    let e = &r.event;
    if !r.recorded {
        writeln!(
            out,
            "Already {} {TITLE}{} — {}{TITLE:#}: nothing changed.",
            e.action.past(),
            e.title,
            e.company
        )?;
        writeln!(out, "{DIM}{} · {}{DIM:#}", e.opportunity, e.id)?;
        writeln!(
            out,
            "Status: {}",
            state_text(r.after.stage, r.after.sentiment)
        )?;
        return out.flush();
    }
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
            let read: Vec<String> = reading.signals.iter().map(describe_signal).collect();
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
    for learned in &outcome.learned {
        writeln!(out, "Learned: {learned}")?;
    }
    for unlearned in &outcome.unlearned {
        writeln!(out, "{DIM}No longer used: {unlearned}{DIM:#}")?;
    }
    if r.after.stage == Stage::Rejected {
        writeln!(
            out,
            "{DIM}It won't be recommended again. Changed your mind: narrow save {}{DIM:#}",
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
            "  Nothing yet. narrow preferences add \"I want backend roles at small companies; avoid pure SRE\""
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
        "{DIM}Reasons are read with {}; every one is kept verbatim (narrow feedback).{DIM:#}",
        model.reader
    )?;
    out.flush()
}

/// `pipeline`.
pub fn pipeline(out: &mut impl Write, entries: &[PipelineEntry]) -> io::Result<()> {
    if entries.is_empty() {
        writeln!(
            out,
            "Nothing in your pipeline yet. Save a job with `narrow save <id>`, or record an \
             application with `narrow applied <id>`."
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
