//! The candidate taste profile as a use case: what Narrow understands
//! about what the person wants, kept apart from what they can practically
//! take. `GET/POST /api/v1/taste/profile`, the MCP `get_taste_profile` and
//! `update_taste_profile` tools, and `narrow preferences describe|show|…`
//! all come here.
//!
//! * [`LocalApp::describe_taste`]: the person answers "What kind of job are
//!   you looking for?". The words are kept verbatim as the taste brief and
//!   as a preference statement (so the deterministic statement parser
//!   still reads practical constraints, and what ranking reads stays what
//!   it was), then interpreted once.
//! * Interpretation runs on three triggers only: a new or changed
//!   description, an explicit "reinterpret", and a correction or added
//!   sentence (just that sentence). Never on a read, a ranking or a job.
//!   The same input is not interpreted twice.
//! * With a model configured ([`crate::TasteModel`]) it reads; if it
//!   fails, the built-in rules read instead and the interpretation says
//!   so. Without one, the rules read. Nothing here needs the network.
//! * [`LocalApp::review_taste`]: confirm, correct, "doesn't matter",
//!   remove, add a sentence. The person's decisions win (see
//!   `jobhunt_profile::taste::edit`). A decision about a statement that
//!   came from an earlier structured preference is applied to that
//!   preference too, so what ranking reads today agrees with what the
//!   person sees.

use chrono::{DateTime, Utc};
use jobhunt_core::text::clean_block;
use jobhunt_profile::ids::resolve_prefix;
use jobhunt_profile::taste::edit::{self, TasteEditError};
use jobhunt_profile::taste::reading::{TasteReading, TasteRequest, Words};
use jobhunt_profile::taste::{
    ComposedAssertion, InterpretationOutcome, LearnedSignal, Polarity, RulesInterpreter,
    TasteDimension, TasteOrigin, TasteProfile, TasteReview, TasteSource, compose, vocab,
};
use jobhunt_profile::{
    Preference, PreferenceValue, ProfileData, ProfileError, ProfileEventKind, Stance, TasteId,
    WorkMode,
};
use jobhunt_ranking::TasteModel as LearnedModel;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::LocalApp;
use crate::error::AppError;
use crate::preferences::{
    PreferenceUpdate, PreferenceUpdateResult, WorkSetupInput, layer, retry_conflicts,
};
use crate::views::time;

/// The longest description kept.
pub const MAX_WORDS: usize = 2000;

/// Which way a statement leans, as given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PolarityInput {
    Prefer,
    Open,
    Avoid,
    /// "I don't care about this."
    Neutral,
}

impl From<PolarityInput> for Polarity {
    fn from(value: PolarityInput) -> Self {
        match value {
            PolarityInput::Prefer => Polarity::Prefer,
            PolarityInput::Open => Polarity::Open,
            PolarityInput::Avoid => Polarity::Avoid,
            PolarityInput::Neutral => Polarity::Neutral,
        }
    }
}

/// A change to the taste profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum TasteAction {
    /// What kind of job the person is looking for, in their words. Replaces
    /// the previous description and interprets it.
    Describe { text: String },
    /// Reads the description again (for example after a model was
    /// configured), keeping every decision the person made.
    Reinterpret,
    /// "Looks right": the given statements (`taste_…`), or with none every
    /// statement of the summary Narrow read.
    Confirm {
        #[serde(default)]
        ids: Vec<String>,
    },
    /// Changes a statement: new words, another polarity, or both.
    Correct {
        id: String,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        polarity: Option<PolarityInput>,
    },
    /// "I don't care about this": kept as not mattering, never inferred
    /// again.
    Neutral { id: String },
    /// Removes a statement; it never comes back.
    Remove { id: String },
    /// Adds one sentence of the person's own.
    Add { text: String },
}

impl TasteAction {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Describe { .. } => "describe",
            Self::Reinterpret => "reinterpret",
            Self::Confirm { .. } => "confirm",
            Self::Correct { .. } => "correct",
            Self::Neutral { .. } => "neutral",
            Self::Remove { .. } => "remove",
            Self::Add { .. } => "add",
        }
    }
}

/// Where one statement comes from, readable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TasteSourceView {
    /// `words`, `preference`, `evidence`, `feedback` or `person`.
    pub kind: String,
    /// "You wrote: “small technical teams”".
    pub text: String,
}

impl TasteSourceView {
    fn of(s: &TasteSource) -> Self {
        Self {
            kind: s.kind().to_owned(),
            text: s.describe(),
        }
    }
}

/// One statement of the taste profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TasteItemView {
    /// `taste_…`: pass it to confirm, correct or remove it.
    pub id: String,
    /// `seniority`, `work_shape`, `specialization`, `ownership`,
    /// `company`, `team`, `culture`, `domain`, `technology`,
    /// `work_style`, `other`.
    pub dimension: String,
    /// The canonical value (`small_team`) or the person's words
    /// normalized.
    pub value: String,
    /// `prefer`, `open`, `avoid` or `neutral`.
    pub polarity: String,
    /// "Small technical teams".
    pub text: String,
    /// `low`, `medium` or `high`.
    pub confidence: String,
    /// `stated`, `interpreted`, `profile`, `learned` or `legacy`.
    pub origin: String,
    /// `unreviewed`, `confirmed`, `corrected` or `removed`.
    pub review: String,
    /// Where it comes from, in a few words: "You said", "Narrow's reading
    /// of your words", "Inferred from your profile", "Learned from your
    /// feedback", "From your earlier settings".
    pub basis: String,
    pub sources: Vec<TasteSourceView>,
    /// Sources that point the other way.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub against: Vec<TasteSourceView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    /// What Narrow had read before the person corrected it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original: Option<String>,
    /// `rules/1`, `model/anthropic:claude-opus-5-5`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpreter: Option<String>,
}

fn basis(a: &ComposedAssertion) -> &'static str {
    match (a.origin, a.review) {
        (_, TasteReview::Removed) => "You removed this",
        (_, TasteReview::Corrected) => "You corrected this",
        (TasteOrigin::Stated, _) => "You said",
        (_, TasteReview::Confirmed) => "You confirmed",
        (TasteOrigin::Interpreted, _) => "Narrow's reading of your words",
        (TasteOrigin::Profile, _) => "Inferred from your profile",
        (TasteOrigin::Learned, _) => "Learned from your feedback",
        (TasteOrigin::Legacy, _) => "From your earlier settings",
    }
}

impl TasteItemView {
    pub fn of(a: &ComposedAssertion) -> Self {
        Self {
            id: a.id.to_string(),
            dimension: a.dimension.as_str().to_owned(),
            value: a.value.clone(),
            polarity: a.polarity.as_str().to_owned(),
            text: a.text.clone(),
            confidence: a.confidence.as_str().to_owned(),
            origin: a.origin.as_str().to_owned(),
            review: a.review.as_str().to_owned(),
            basis: basis(a).to_owned(),
            sources: a.sources.iter().map(TasteSourceView::of).collect(),
            against: a.against.iter().map(TasteSourceView::of).collect(),
            explanation: a.explanation.clone(),
            original: a.original.as_ref().map(|o| o.text.clone()),
            interpreter: a.interpreter.clone(),
        }
    }
}

/// One line of the summary: statements about one dimension that lean the
/// same way ("Backend engineering · Platform engineering").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TasteLineView {
    pub text: String,
    pub dimension: String,
    /// Every statement of the line is Narrow's inference (from the profile
    /// or feedback), not something the person said.
    pub inferred: bool,
    pub items: Vec<TasteItemView>,
}

/// A practical constraint: whether the person can take a job, not
/// whether they'd want it. Read from their preferences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConstraintView {
    /// `work_setup`, `location`, `authorization`, `relocation`,
    /// `remote_geography`, `timezone`, `sponsorship`, `engagement`,
    /// `pay_floor`, `pay_target`, `policy`.
    pub kind: String,
    /// "Remote only", "Based in São Paulo, Brazil".
    pub text: String,
    /// `requirement` (a posting that states otherwise is left out) or
    /// `preference`.
    pub layer: String,
    /// The preferences behind it (`pref_…`); empty when read from the
    /// resume.
    pub ids: Vec<String>,
}

/// How the description was last interpreted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InterpretationView {
    pub interpreter: String,
    /// `read`, or `fallback` when a model couldn't be used and the rules
    /// read instead.
    pub outcome: String,
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// What was unclear, for the person to settle.
    pub ambiguities: Vec<String>,
    /// Practical constraints the words mentioned (never taste).
    pub constraints_noted: Vec<String>,
    /// Statements the interpreter produced that failed validation.
    pub rejected: u32,
}

/// What reads the person's words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReaderView {
    /// `model` or `rules`.
    pub kind: String,
    /// `rules/1`, `model/anthropic:claude-opus-5-5`.
    pub name: String,
    /// Why a configured model isn't used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The taste profile, as the Preferences page shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TasteProfileView {
    /// What the person is looking for, in their words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub looking_for: Option<String>,
    /// `description` (what they described), `statements` (their earlier
    /// words, not yet interpreted as a description), or absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub looking_for_source: Option<String>,
    /// What Narrow understands they want, one line per dimension.
    pub understood: Vec<TasteLineView>,
    /// What they tend to avoid.
    pub avoid: Vec<TasteLineView>,
    /// Weak inferences, not part of the summary unless confirmed.
    pub unsure: Vec<TasteItemView>,
    /// Said not to matter.
    pub neutral: Vec<TasteItemView>,
    /// Patterns learned from feedback, kept apart from what they said.
    pub learned: Vec<TasteItemView>,
    /// Statements they removed (never read again).
    pub removed: Vec<TasteItemView>,
    /// Whether they can take a job: work setup, where they live and may
    /// work, relocation, pay floor.
    pub constraints: Vec<ConstraintView>,
    /// The summary has statements Narrow read that they haven't reviewed.
    pub needs_confirmation: bool,
    /// When they last said it looks right.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interpretation: Option<InterpretationView>,
    pub reader: ReaderView,
}

/// The answer of a change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TasteUpdateResult {
    /// `describe`, `reinterpret`, `confirm`, `correct`, `neutral`,
    /// `remove`, `add`.
    pub action: String,
    /// Something changed.
    pub changed: bool,
    /// The words were interpreted by this call (false when the same input
    /// was already interpreted).
    pub interpreted: bool,
    /// For a description: the practical constraints and structured
    /// preferences read from the words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferences: Option<PreferenceUpdateResult>,
    /// The profile afterwards.
    pub profile: TasteProfileView,
}

/// Learned patterns as taste signals: only patterns ranking uses (active),
/// never pay or particular employers.
pub fn learned_signals(model: &LearnedModel) -> Vec<LearnedSignal> {
    jobhunt_ranking::taste::learned_signals(model)
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The practical constraints in effect.
pub fn constraints(data: &ProfileData) -> Vec<ConstraintView> {
    let active: Vec<&Preference> = data.preferences.iter().filter(|p| p.active).collect();
    let mut out = Vec::new();
    let modes: Vec<&Preference> = active
        .iter()
        .copied()
        .filter(|p| matches!(p.value, PreferenceValue::WorkMode { .. }))
        .collect();
    if !modes.is_empty() {
        let pairs: Vec<(WorkMode, Stance)> = modes
            .iter()
            .filter_map(|p| match p.value {
                PreferenceValue::WorkMode { mode } => Some((mode, p.stance)),
                _ => None,
            })
            .collect();
        let text = match WorkSetupInput::of(&pairs) {
            Some(WorkSetupInput::RemoteOnly) => "Remote only".to_owned(),
            Some(WorkSetupInput::PreferRemote) => "Prefers remote (not a must)".to_owned(),
            Some(WorkSetupInput::HybridOkay) => "Remote or hybrid, not on-site".to_owned(),
            Some(WorkSetupInput::OnsiteOkay) => "On-site is fine".to_owned(),
            Some(WorkSetupInput::NoPreference) | None => capitalize(
                &modes
                    .iter()
                    .map(|p| format!("{} {}", p.stance.as_str(), p.value))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        };
        out.push(ConstraintView {
            kind: "work_setup".into(),
            text,
            layer: if pairs.iter().any(|(_, s)| *s == Stance::Required) {
                "requirement"
            } else {
                "preference"
            }
            .into(),
            ids: modes.iter().map(|p| p.id.to_string()).collect(),
        });
    }
    let has_home = active
        .iter()
        .any(|p| matches!(p.value, PreferenceValue::CurrentLocation { .. }));
    if !has_home && let Some(place) = &data.profile.location {
        out.push(ConstraintView {
            kind: "location".into(),
            text: format!("Based in {place} (from your resume)"),
            layer: "requirement".into(),
            ids: Vec::new(),
        });
    }
    for p in &active {
        let kind = match &p.value {
            PreferenceValue::CurrentLocation { .. } => "location",
            PreferenceValue::WorkAuthorization { .. } => "authorization",
            PreferenceValue::Relocation { .. } => "relocation",
            PreferenceValue::Region { .. } => "remote_geography",
            PreferenceValue::Timezone { .. } => "timezone",
            PreferenceValue::Sponsorship { .. } => "sponsorship",
            PreferenceValue::Engagement { .. } => "engagement",
            PreferenceValue::Compensation {
                bound: jobhunt_profile::CompensationBound::Minimum,
                ..
            } => "pay_floor",
            PreferenceValue::Compensation { .. } => "pay_target",
            PreferenceValue::UnknownPay { show: false }
            | PreferenceValue::UnclearEligibility { show: false } => "policy",
            _ => continue,
        };
        let mut text = capitalize(&p.value.to_string());
        if p.stance == Stance::Unwanted && kind != "policy" {
            text = format!("Not: {}", p.value);
        }
        out.push(ConstraintView {
            kind: kind.into(),
            text,
            layer: layer(p).into(),
            ids: vec![p.id.to_string()],
        });
    }
    out
}

fn lines(items: &[&ComposedAssertion]) -> Vec<TasteLineView> {
    let mut out: Vec<TasteLineView> = Vec::new();
    for dimension in TasteDimension::ALL {
        let group: Vec<&&ComposedAssertion> =
            items.iter().filter(|a| a.dimension == dimension).collect();
        if group.is_empty() {
            continue;
        }
        out.push(TasteLineView {
            text: group
                .iter()
                .map(|a| a.text.as_str())
                .collect::<Vec<_>>()
                .join(" · "),
            dimension: dimension.as_str().to_owned(),
            inferred: group
                .iter()
                .all(|a| !a.is_persons() && a.origin == TasteOrigin::Profile),
            items: group.iter().map(|a| TasteItemView::of(a)).collect(),
        });
    }
    out
}

/// The view of a composed profile.
pub fn view(data: &ProfileData, profile: &TasteProfile, reader: ReaderView) -> TasteProfileView {
    let firm = |a: &&ComposedAssertion| {
        a.polarity != Polarity::Neutral
            && a.is_firm()
            && (a.origin != TasteOrigin::Learned || a.is_persons())
    };
    let wanted: Vec<&ComposedAssertion> = profile
        .assertions
        .iter()
        .filter(firm)
        .filter(|a| matches!(a.polarity, Polarity::Prefer | Polarity::Open))
        .collect();
    let avoided: Vec<&ComposedAssertion> = profile
        .assertions
        .iter()
        .filter(firm)
        .filter(|a| a.polarity == Polarity::Avoid)
        .collect();
    let needs_confirmation = wanted.iter().chain(&avoided).any(|a| {
        !a.is_persons() && matches!(a.origin, TasteOrigin::Interpreted | TasteOrigin::Profile)
    });
    let (looking_for, looking_for_source) = match &data.taste_brief {
        Some(b) => (Some(b.text.clone()), Some("description".to_owned())),
        None if !data.statements.is_empty() => (
            Some(
                data.statements
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            Some("statements".to_owned()),
        ),
        None => (None, None),
    };
    TasteProfileView {
        looking_for,
        looking_for_source,
        understood: lines(&wanted),
        avoid: lines(&avoided),
        unsure: profile
            .assertions
            .iter()
            .filter(|a| {
                a.polarity != Polarity::Neutral && !a.is_firm() && a.origin != TasteOrigin::Learned
            })
            .map(TasteItemView::of)
            .collect(),
        neutral: profile
            .assertions
            .iter()
            .filter(|a| a.polarity == Polarity::Neutral)
            .map(TasteItemView::of)
            .collect(),
        learned: profile
            .assertions
            .iter()
            .filter(|a| a.origin == TasteOrigin::Learned && !a.is_persons())
            .filter(|a| a.polarity != Polarity::Neutral)
            .map(TasteItemView::of)
            .collect(),
        removed: profile.removed.iter().map(TasteItemView::of).collect(),
        constraints: constraints(data),
        needs_confirmation,
        confirmed_at: data
            .taste_brief
            .as_ref()
            .and_then(|b| b.confirmed_at)
            .map(time),
        interpretation: data
            .taste_brief
            .as_ref()
            .and_then(|b| b.interpretation.as_ref())
            .map(|i| InterpretationView {
                interpreter: i.interpreter.clone(),
                outcome: i.outcome.as_str().to_owned(),
                at: time(i.at),
                note: i.note.clone(),
                summary: i.summary.clone(),
                ambiguities: i.ambiguities.clone(),
                constraints_noted: i.constraints_noted.clone(),
                rejected: i.rejected,
            }),
        reader,
    }
}

fn edit_error(e: TasteEditError) -> ProfileError {
    match e {
        TasteEditError::NotFound(id) => ProfileError::NotFound {
            what: "taste statement",
            input: id,
        },
        TasteEditError::Invalid(message) => ProfileError::Invalid(message),
    }
}

fn clean_words(text: &str) -> Result<String, AppError> {
    let text = clean_block(text).ok_or_else(|| {
        AppError::InvalidArguments("say what you're looking for in a few words".into())
    })?;
    if text.chars().count() > MAX_WORDS {
        return Err(AppError::InvalidArguments(format!(
            "keep it under {MAX_WORDS} characters"
        )));
    }
    Ok(text)
}

impl LocalApp {
    /// What reads the person's words now.
    pub fn taste_reader(&self) -> ReaderView {
        let model = self.taste_model();
        match &model.model {
            Some(m) => ReaderView {
                kind: "model".into(),
                name: m.name(),
                note: None,
            },
            None => ReaderView {
                kind: "rules".into(),
                name: RulesInterpreter::NAME.into(),
                note: model.problem.as_ref().map(|p| {
                    format!("A model is configured but can't be used ({p}); Narrow reads with its built-in rules.")
                }),
            },
        }
    }

    fn reader_name(&self) -> String {
        self.taste_model()
            .model
            .as_ref()
            .map_or_else(|| RulesInterpreter::NAME.to_owned(), |m| m.name())
    }

    /// Reads a request: with the model when there is one, falling back to
    /// the rules (and saying so) when it fails.
    async fn read_taste(
        &self,
        request: &TasteRequest,
    ) -> (TasteReading, InterpretationOutcome, Option<String>) {
        let model = self.taste_model();
        match &model.model {
            Some(m) => match m.interpret(request).await {
                Ok(reading) => (reading, InterpretationOutcome::Read, None),
                Err(error) => {
                    tracing::warn!(%error, "taste model failed; reading with the built-in rules");
                    (
                        RulesInterpreter.read(request),
                        InterpretationOutcome::Fallback,
                        Some(format!(
                            "The AI reader couldn't be used ({error}); this is Narrow's simpler reading. Try again later."
                        )),
                    )
                }
            },
            None => (
                RulesInterpreter.read(request),
                if model.problem.is_some() {
                    InterpretationOutcome::Fallback
                } else {
                    InterpretationOutcome::Read
                },
                model.problem.as_ref().map(|p| {
                    format!("The AI reader is configured but can't be used ({p}); this is Narrow's simpler reading.")
                }),
            ),
        }
    }

    /// The profile, its taste profile composed, and the learned patterns.
    pub async fn candidate_taste(
        &self,
    ) -> Result<(ProfileData, TasteProfile, Vec<LearnedSignal>), AppError> {
        let ranking = self.ranking();
        let (data, person) = ranking.person().await?;
        let data = data.ok_or(AppError::NoProfile)?;
        let learned = learned_signals(&ranking.taste(&person).await?);
        let profile = compose(&data, &learned);
        Ok((data, profile, learned))
    }

    /// The taste profile as the Preferences page shows it. Reads only.
    pub async fn taste_profile(&self) -> Result<TasteProfileView, AppError> {
        let (data, profile, _) = self.candidate_taste().await?;
        Ok(view(&data, &profile, self.taste_reader()))
    }

    async fn result(
        &self,
        action: &TasteAction,
        changed: bool,
        interpreted: bool,
        preferences: Option<PreferenceUpdateResult>,
    ) -> Result<TasteUpdateResult, AppError> {
        Ok(TasteUpdateResult {
            action: action.name().to_owned(),
            changed,
            interpreted,
            preferences,
            profile: self.taste_profile().await?,
        })
    }

    /// "What kind of job are you looking for?": stores the words, reads
    /// practical constraints from them, and interprets them.
    pub async fn describe_taste(
        &self,
        text: &str,
        now: DateTime<Utc>,
    ) -> Result<TasteUpdateResult, AppError> {
        let text = clean_words(text)?;
        let profiles = self.profiles();
        let current = profiles.load_or_new(now).await?;
        // The previous description's statement goes with it (what its
        // words set is replaced by what the new words set); older
        // statements in the person's words are left alone.
        let replaced: Vec<String> = current
            .taste_brief
            .as_ref()
            .filter(|b| b.text != text)
            .and_then(|b| b.statement)
            .filter(|id| {
                current
                    .statements
                    .iter()
                    .any(|s| s.id == *id && s.text != text)
            })
            .map(|id| vec![id.to_string()])
            .unwrap_or_default();
        let changes = self
            .update_preferences(
                &PreferenceUpdate {
                    statement: Some(text.clone()),
                    set: Vec::new(),
                    remove: replaced,
                },
                now,
            )
            .await?;
        let statement = changes.statement.as_ref().map(|s| s.statement.id);
        let preferences = PreferenceUpdateResult::of(&changes);
        let changed = self
            .exclusive(retry_conflicts(|| {
                profiles.change_taste(
                    ProfileEventKind::TasteDescribed,
                    "described what they are looking for",
                    now,
                    |d| Ok(edit::set_brief(d, &text, statement, now)),
                )
            }))
            .await?
            .0;
        let interpreted = self.interpret_brief(false, now).await?;
        let action = TasteAction::Describe { text };
        self.result(
            &action,
            changed || interpreted,
            interpreted,
            Some(preferences),
        )
        .await
    }

    /// Interprets the description (made from the person's earlier words
    /// when they never described what they want). Skipped when the same
    /// input was already interpreted, unless `force`. Returns whether it
    /// was interpreted.
    pub async fn interpret_brief(&self, force: bool, now: DateTime<Utc>) -> Result<bool, AppError> {
        let (data, _, learned) = self.candidate_taste().await?;
        let profiles = self.profiles();
        let brief = match &data.taste_brief {
            Some(b) => b.clone(),
            None if !data.statements.is_empty() => {
                // Their earlier words, verbatim, become the description.
                let text = data
                    .statements
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                let statement = (data.statements.len() == 1).then(|| data.statements[0].id);
                self.exclusive(retry_conflicts(|| {
                    profiles.change_taste(
                        ProfileEventKind::TasteDescribed,
                        "description made from earlier words",
                        now,
                        |d| Ok(edit::set_brief(d, &text, statement, now)),
                    )
                }))
                .await?
                .1
                .taste_brief
                .ok_or(AppError::NoProfile)?
            }
            None => {
                return Err(AppError::InvalidArguments(
                    "say what kind of job you're looking for first".into(),
                ));
            }
        };
        let request = TasteRequest::build(
            &data,
            vec![Words {
                text: brief.text.clone(),
                statement: brief.statement,
            }],
            &learned,
            None,
        );
        let expected = request.digest(&self.reader_name());
        if !force
            && brief
                .interpretation
                .as_ref()
                .is_some_and(|i| i.input_digest == expected)
        {
            return Ok(false);
        }
        let (reading, outcome, note) = self.read_taste(&request).await;
        let digest = request.digest(&reading.interpreter);
        let detail = format!(
            "{} statements read by {}",
            reading.assertions.len(),
            reading.interpreter
        );
        self.exclusive(retry_conflicts(|| {
            profiles.change_taste(
                ProfileEventKind::TasteInterpreted,
                detail.clone(),
                now,
                |d| {
                    Ok(edit::apply_reading(
                        d,
                        &reading,
                        outcome,
                        note.clone(),
                        digest.clone(),
                        now,
                    ))
                },
            )
        }))
        .await?;
        Ok(true)
    }

    /// Reads one sentence of the person's (a correction or an addition):
    /// only what the words say, never inferences.
    async fn read_sentence(
        &self,
        data: &ProfileData,
        text: &str,
        focus: Option<TasteDimension>,
    ) -> TasteReading {
        let request = TasteRequest::build(
            data,
            vec![Words {
                text: text.to_owned(),
                statement: None,
            }],
            &[],
            focus.or(Some(TasteDimension::Other)),
        );
        let (mut reading, _, _) = self.read_taste(&request).await;
        reading
            .assertions
            .retain(|a| a.origin == TasteOrigin::Interpreted);
        reading
    }

    fn resolve_taste(&self, profile: &TasteProfile, input: &str) -> Result<TasteId, AppError> {
        let input = input.trim();
        if let Ok(id) = input.parse::<TasteId>() {
            return Ok(id);
        }
        let ids: Vec<TasteId> = profile.assertions.iter().map(|a| a.id).collect();
        resolve_prefix(input, &ids).ok_or_else(|| {
            AppError::InvalidArguments(format!(
                "{input:?} is not a taste statement id (taste_…) of this profile"
            ))
        })
    }

    /// Brings the structured preferences behind a decided statement in
    /// line with it: removed, or given the new stance.
    async fn apply_to_preferences(
        &self,
        decided: &edit::Decided,
        now: DateTime<Utc>,
    ) -> Result<(), AppError> {
        if decided.preferences.is_empty() {
            return Ok(());
        }
        let profiles = self.profiles();
        let data = profiles.load_or_new(now).await?;
        for id in &decided.preferences {
            let Some(p) = data.preferences.iter().find(|p| p.id == *id && p.active) else {
                continue;
            };
            match decided.polarity.and_then(vocab::stance_of) {
                Some(stance) if vocab::polarity_of(p.stance) != vocab::polarity_of(stance) => {
                    let value = p.value.clone();
                    retry_conflicts(|| profiles.set_preference(value.clone(), stance, now)).await?;
                }
                Some(_) => {}
                None => {
                    let id = id.to_string();
                    retry_conflicts(|| profiles.remove(&id, now)).await?;
                }
            }
        }
        Ok(())
    }

    /// A change to the taste profile (see [`TasteAction`]).
    pub async fn review_taste(
        &self,
        action: &TasteAction,
        now: DateTime<Utc>,
    ) -> Result<TasteUpdateResult, AppError> {
        match action {
            TasteAction::Describe { text } => return self.describe_taste(text, now).await,
            TasteAction::Reinterpret => {
                let interpreted = self.interpret_brief(true, now).await?;
                return self.result(action, interpreted, interpreted, None).await;
            }
            _ => {}
        }
        let (data, profile, learned) = self.candidate_taste().await?;
        let profiles = self.profiles();
        let kind = ProfileEventKind::TasteReviewed;
        let decided: Option<edit::Decided> = match action {
            TasteAction::Confirm { ids } => {
                let ids = ids
                    .iter()
                    .map(|i| self.resolve_taste(&profile, i))
                    .collect::<Result<Vec<_>, _>>()?;
                self.exclusive(retry_conflicts(|| {
                    profiles.change_taste(kind, "confirmed", now, |d| {
                        let p = compose(d, &learned);
                        edit::confirm(d, &p, &ids, now).map_err(edit_error)
                    })
                }))
                .await?;
                None
            }
            TasteAction::Correct { id, text, polarity } => {
                let id = self.resolve_taste(&profile, id)?;
                let target = profile.find(id).ok_or_else(|| {
                    AppError::InvalidArguments(format!("no taste statement {id}"))
                })?;
                let text = text.as_deref().map(str::trim).filter(|t| !t.is_empty());
                let reading = match text {
                    Some(t) if t != target.text => {
                        Some(self.read_sentence(&data, t, Some(target.dimension)).await)
                    }
                    _ => None,
                };
                let polarity = polarity.map(Polarity::from);
                let (decided, _) = self
                    .exclusive(retry_conflicts(|| {
                        profiles.change_taste(kind, "corrected", now, |d| {
                            let p = compose(d, &learned);
                            edit::correct(d, &p, id, polarity, text, reading.as_ref(), now)
                                .map_err(edit_error)
                        })
                    }))
                    .await?;
                Some(decided)
            }
            TasteAction::Neutral { id } | TasteAction::Remove { id } => {
                let id = self.resolve_taste(&profile, id)?;
                let neutral = matches!(action, TasteAction::Neutral { .. });
                let detail = if neutral {
                    "marked as not mattering"
                } else {
                    "removed"
                };
                let (decided, _) = self
                    .exclusive(retry_conflicts(|| {
                        profiles.change_taste(kind, detail, now, |d| {
                            let p = compose(d, &learned);
                            if neutral {
                                edit::correct(d, &p, id, Some(Polarity::Neutral), None, None, now)
                            } else {
                                edit::remove(d, &p, id, now)
                            }
                            .map_err(edit_error)
                        })
                    }))
                    .await?;
                Some(decided)
            }
            TasteAction::Add { text } => {
                let text = clean_words(text)?;
                let reading = self.read_sentence(&data, &text, None).await;
                self.exclusive(retry_conflicts(|| {
                    profiles.change_taste(kind, "added", now, |d| {
                        edit::add(d, &text, &reading, now).map_err(edit_error)
                    })
                }))
                .await?;
                None
            }
            TasteAction::Describe { .. } | TasteAction::Reinterpret => None,
        };
        if let Some(decided) = &decided {
            self.exclusive(self.apply_to_preferences(decided, now))
                .await?;
        }
        self.result(action, true, false, None).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_parse_from_json() {
        let a: TasteAction = serde_json::from_str(
            r#"{"action": "correct", "id": "taste_1", "polarity": "neutral"}"#,
        )
        .unwrap();
        assert_eq!(
            a,
            TasteAction::Correct {
                id: "taste_1".into(),
                text: None,
                polarity: Some(PolarityInput::Neutral)
            }
        );
        let confirm: TasteAction = serde_json::from_str(r#"{"action": "confirm"}"#).unwrap();
        assert_eq!(confirm, TasteAction::Confirm { ids: Vec::new() });
        assert!(serde_json::from_str::<TasteAction>(r#"{"action": "tune"}"#).is_err());
        assert!(
            serde_json::from_str::<TasteAction>(r#"{"action": "remove", "id": "x", "y": 1}"#)
                .is_err()
        );
    }
}
