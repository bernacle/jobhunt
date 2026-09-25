//! `jobhunt claims`: the evidence behind the profile.

use std::io::Write;
use std::process::ExitCode;

use chrono::Utc;
use clap::Subcommand;
use jobhunt_profile::{
    ClaimKind, ClaimQuery, ProfileService, Provenance, RecordId, Subject, Verification,
};

use crate::config::LoadedConfig;
use crate::profile_args::{finish, open_store};
use crate::profile_render::{self, short};
use crate::render::plural;

#[derive(Debug, clap::Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct ClaimsArgs {
    #[command(flatten)]
    pub list: ListArgs,

    #[command(subcommand)]
    pub command: Option<ClaimsCommand>,
}

#[derive(Debug, Clone, Default, clap::Args)]
pub struct ListArgs {
    /// Only claims of this kind (repeatable).
    #[arg(long, value_enum)]
    pub kind: Vec<KindArg>,
    /// Only claims in this state.
    #[arg(long, value_enum)]
    pub state: Option<StateArg>,
    /// Only claims about this experience, project or education entry.
    #[arg(long = "for", value_name = "ID")]
    pub subject: Option<String>,
    /// Include rejected claims and claims no longer in your resume.
    #[arg(long)]
    pub all: bool,
}

#[derive(Debug, Subcommand)]
pub enum ClaimsCommand {
    /// List claims (the default).
    List(ListArgs),
    /// Claims that need your review, with the resume text behind each.
    Review {
        /// Show every claim needing review, not only the first 15.
        #[arg(long)]
        all: bool,
    },
    /// Everything about one claim: source, snippet, provenance, decision.
    Show { id: String },
    /// Confirm claims are true. Confirmed claims may be used for you.
    Confirm {
        #[arg(required = true, value_name = "ID")]
        ids: Vec<String>,
    },
    /// Reject claims. Rejected claims are never used and stay rejected
    /// across resume re-imports.
    Reject {
        #[arg(required = true, value_name = "ID")]
        ids: Vec<String>,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Undo a decision (back to unreviewed).
    Reset {
        #[arg(required = true, value_name = "ID")]
        ids: Vec<String>,
    },
    /// Add a claim in your own words.
    Add {
        text: String,
        /// What it is about (exp_…, proj_…, edu_…); the profile if omitted.
        #[arg(long = "for", value_name = "ID")]
        subject: Option<String>,
        #[arg(long, value_enum, default_value = "accomplishment")]
        kind: KindArg,
        /// Topic for technology, domain or role claims ("kubernetes", "payments").
        #[arg(long)]
        topic: Option<String>,
    },
    /// Rewrite a claim's text (your wording is kept across re-imports).
    Edit { id: String, text: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum KindArg {
    Employment,
    Responsibility,
    Accomplishment,
    Technology,
    Skill,
    Domain,
    Role,
    Ownership,
    Education,
    Project,
    Other,
}

impl From<KindArg> for ClaimKind {
    fn from(value: KindArg) -> Self {
        match value {
            KindArg::Employment => Self::Employment,
            KindArg::Responsibility => Self::Responsibility,
            KindArg::Accomplishment => Self::Accomplishment,
            KindArg::Technology => Self::Technology,
            KindArg::Skill => Self::Skill,
            KindArg::Domain => Self::Domain,
            KindArg::Role => Self::Role,
            KindArg::Ownership => Self::Ownership,
            KindArg::Education => Self::Education,
            KindArg::Project => Self::Project,
            KindArg::Other => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum StateArg {
    /// Needs your review.
    Review,
    /// Usable (confirmed, entered by you, or quoted from your resume).
    Usable,
    Confirmed,
    Rejected,
    Unverified,
    /// No longer in your latest resume.
    Stale,
    Extracted,
    Inferred,
    /// Entered by you.
    Mine,
}

pub async fn run(args: ClaimsArgs, loaded: &LoadedConfig) -> anyhow::Result<ExitCode> {
    let store = open_store(loaded).await?;
    let result = execute(args, &store).await;
    store.close().await;
    result
}

async fn execute(
    args: ClaimsArgs,
    store: &jobhunt_storage::SqliteJobStore,
) -> anyhow::Result<ExitCode> {
    let service = ProfileService::new(store);
    let now = Utc::now();
    let mut out = anstream::stdout().lock();
    match args.command.unwrap_or(ClaimsCommand::List(args.list)) {
        ClaimsCommand::List(list) => {
            let data = service.require().await?;
            let mut query = ClaimQuery {
                kinds: list.kind.iter().map(|k| ClaimKind::from(*k)).collect(),
                ..ClaimQuery::default()
            };
            if let Some(input) = &list.subject {
                query.subject = Some(match service.resolve(&data, input)? {
                    RecordId::Experience(id) => Subject::Experience(id),
                    RecordId::Project(id) => Subject::Project(id),
                    RecordId::Education(id) => Subject::Education(id),
                    _ => anyhow::bail!("{input:?} is not an experience, project or education id"),
                });
            }
            let mut show_hidden = list.all;
            match list.state {
                Some(StateArg::Review) => query.needs_review = true,
                Some(StateArg::Usable) => query.usable_only = true,
                Some(StateArg::Confirmed) => query.verification = Some(Verification::Confirmed),
                Some(StateArg::Rejected) => {
                    query.verification = Some(Verification::Rejected);
                    show_hidden = true;
                }
                Some(StateArg::Unverified) => query.verification = Some(Verification::Unverified),
                Some(StateArg::Stale) => show_hidden = true,
                Some(StateArg::Extracted) => query.provenance = Some(Provenance::Extracted),
                Some(StateArg::Inferred) => query.provenance = Some(Provenance::Inferred),
                Some(StateArg::Mine) => query.provenance = Some(Provenance::UserEntered),
                None => {}
            }
            let claims: Vec<_> = data
                .claims(&query)
                .into_iter()
                .filter(|c| match list.state {
                    Some(StateArg::Stale) => c.stale_since.is_some(),
                    _ => {
                        show_hidden
                            || (c.stale_since.is_none()
                                && data.standing(c) != jobhunt_profile::Standing::Rejected)
                    }
                })
                .collect();
            finish(profile_render::claim_list(&mut out, &data, &claims))
        }
        ClaimsCommand::Review { all } => {
            let data = service.require().await?;
            let queue = data.review_queue();
            let shown = if all {
                queue.len()
            } else {
                queue.len().min(15)
            };
            finish(profile_render::review(
                &mut out,
                &data,
                &queue[..shown],
                queue.len(),
            ))
        }
        ClaimsCommand::Show { id } => {
            let data = service.require().await?;
            let RecordId::Claim(claim) = service.resolve(&data, &id)? else {
                anyhow::bail!("{id:?} is not a claim id (clm_…)");
            };
            let Some(claim) = data.claim(claim) else {
                anyhow::bail!("no claim has the id {id}");
            };
            finish(profile_render::claim_detail(&mut out, &data, claim))
        }
        ClaimsCommand::Confirm { ids } => {
            let changed = service
                .decide_claims(&ids, Verification::Confirmed, None, now)
                .await?;
            decided(&mut out, "Confirmed", &changed)
        }
        ClaimsCommand::Reject { ids, reason } => {
            let changed = service
                .decide_claims(&ids, Verification::Rejected, reason, now)
                .await?;
            decided(&mut out, "Rejected", &changed)
        }
        ClaimsCommand::Reset { ids } => {
            let changed = service
                .decide_claims(&ids, Verification::Unverified, None, now)
                .await?;
            decided(&mut out, "Reset", &changed)
        }
        ClaimsCommand::Add {
            text,
            subject,
            kind,
            topic,
        } => {
            let claim = service
                .add_claim(
                    &text,
                    kind.into(),
                    subject.as_deref(),
                    topic.as_deref(),
                    now,
                )
                .await?;
            finish(writeln!(out, "Added {}: {}", short(claim.id), claim.text))
        }
        ClaimsCommand::Edit { id, text } => {
            let claim = service.edit_claim(&id, &text, now).await?;
            finish(writeln!(
                out,
                "Updated {}: {} (confirmed; your wording is kept across re-imports)",
                short(claim.id),
                claim.text
            ))
        }
    }
}

fn decided(
    out: &mut impl Write,
    what: &str,
    claims: &[jobhunt_profile::Claim],
) -> anyhow::Result<ExitCode> {
    let mut result = writeln!(
        out,
        "{what} {}:",
        plural(claims.len() as u64, "claim", "claims")
    );
    for claim in claims {
        if result.is_ok() {
            result = writeln!(out, "  {} {}", short(claim.id), claim.text);
        }
    }
    finish(result)
}
