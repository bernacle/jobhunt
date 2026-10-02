//! Optional model providers behind JobHunt's interpretation seams.
//!
//! JobHunt needs no model: without one, the built-in rules read the
//! person's words ([`jobhunt_profile::taste::RulesInterpreter`]). This crate
//! lets a deployment or a local user point the taste reader at a model:
//!
//! * [`Provider::Anthropic`]: the Anthropic Messages API, with structured
//!   outputs (`output_config.format`, a JSON schema), so the answer is
//!   JSON of a known shape, never prose;
//! * [`Provider::OpenAi`]: any server speaking the OpenAI chat completions
//!   API with `response_format: json_schema` (OpenAI, and self-hosted
//!   servers such as Ollama, vLLM or LM Studio), which keeps the
//!   open-source path free of any particular vendor.
//!
//! Both are plain HTTP (no SDK). What is sent is exactly
//! [`jobhunt_profile::taste::reading::render`] of the request, with the
//! fixed [`SYSTEM_PROMPT`];
//! see that module for what a request may contain. Neither the prompt nor
//! the answer is ever logged: logs carry the provider, model, status,
//! attempt and duration only.
//!
//! Transient failures (connection errors, timeouts, 408/409/429/5xx) are
//! retried with backoff, and so is an answer that fails validation (once
//! per attempt budget); refusals, truncated answers and other client
//! errors are not.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use jobhunt_profile::taste::reading::{
    InterpretError, SYSTEM_PROMPT, TasteInterpreter, TasteReading, TasteRequest, parse_reading,
    reading_schema, render,
};
use jobhunt_ranking::review::{self, FitReview, FitReviewRequest, FitReviewer, ReviewError};
use serde_json::{Value, json};
use url::Url;

/// Which API a model is reached through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// The Anthropic Messages API.
    Anthropic,
    /// An OpenAI-compatible chat completions API.
    OpenAi,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAi => "openai",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "anthropic" | "claude" => Some(Self::Anthropic),
            "openai" | "openai_compatible" | "openai-compatible" => Some(Self::OpenAi),
            _ => None,
        }
    }

    /// Where the API is unless configured otherwise.
    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::Anthropic => "https://api.anthropic.com",
            Self::OpenAi => "https://api.openai.com/v1",
        }
    }

    /// The model used unless configured otherwise (`None`: must be named).
    pub fn default_model(self) -> Option<&'static str> {
        match self {
            Self::Anthropic => Some("claude-opus-5-5"),
            Self::OpenAi => None,
        }
    }

    /// The environment variable read for the key unless configured
    /// otherwise.
    pub fn default_key_env(self) -> &'static str {
        match self {
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::OpenAi => "OPENAI_API_KEY",
        }
    }
}

/// How to reach a model.
#[derive(Clone, PartialEq, Eq)]
pub struct ModelConfig {
    pub provider: Provider,
    pub model: String,
    pub base_url: Url,
    /// Never printed (see the `Debug` implementation).
    pub api_key: Option<String>,
    /// Per attempt.
    pub timeout: Duration,
    /// Attempts after the first.
    pub max_retries: u32,
    /// Base of the exponential backoff between attempts.
    pub backoff: Duration,
    /// Anthropic `output_config.effort` (`low`, `medium`, …), when set.
    pub effort: Option<String>,
}

impl std::fmt::Debug for ModelConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelConfig")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("base_url", &self.base_url.as_str())
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("timeout", &self.timeout)
            .field("max_retries", &self.max_retries)
            .field("effort", &self.effort)
            .finish()
    }
}

/// Why a model can't be configured.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("unknown model provider {0:?} (use \"anthropic\" or \"openai\")")]
    Provider(String),
    #[error("the {0} provider needs a model name")]
    Model(&'static str),
    #[error("the model base URL {0:?} is not an http(s) URL")]
    BaseUrl(String),
    #[error("the {provider} provider needs an API key ({env} is not set)")]
    Key { provider: &'static str, env: String },
}

impl ModelConfig {
    /// A configuration with the provider's defaults for what isn't given.
    /// `api_key` is required for Anthropic and for the default OpenAI URL;
    /// a self-hosted OpenAI-compatible server may need none.
    pub fn new(
        provider: &str,
        model: Option<&str>,
        base_url: Option<&str>,
        api_key: Option<String>,
        key_env: &str,
    ) -> Result<Self, ConfigError> {
        let provider =
            Provider::parse(provider).ok_or_else(|| ConfigError::Provider(provider.to_owned()))?;
        let model = model
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .or(provider.default_model())
            .ok_or(ConfigError::Model(provider.as_str()))?
            .to_owned();
        let raw = base_url
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .unwrap_or(provider.default_base_url());
        let base_url = Url::parse(raw)
            .ok()
            .filter(|u| matches!(u.scheme(), "http" | "https"))
            .ok_or_else(|| ConfigError::BaseUrl(raw.to_owned()))?;
        let api_key = api_key.filter(|k| !k.trim().is_empty());
        let hosted = base_url.as_str().trim_end_matches('/')
            == provider.default_base_url().trim_end_matches('/');
        if api_key.is_none() && (provider == Provider::Anthropic || hosted) {
            return Err(ConfigError::Key {
                provider: provider.as_str(),
                env: key_env.to_owned(),
            });
        }
        Ok(Self {
            provider,
            effort: (provider == Provider::Anthropic).then(|| "low".to_owned()),
            model,
            base_url,
            api_key,
            timeout: Duration::from_secs(40),
            max_retries: 1,
            backoff: Duration::from_millis(500),
        })
    }

    fn endpoint(&self) -> String {
        let base = self.base_url.as_str().trim_end_matches('/');
        match self.provider {
            Provider::Anthropic => format!("{base}/v1/messages"),
            Provider::OpenAi => format!("{base}/chat/completions"),
        }
    }

    /// Whether this is the hosted Claude API (where server-side refusal
    /// fallbacks are available).
    fn is_claude_api(&self) -> bool {
        self.provider == Provider::Anthropic
            && self.base_url.host_str() == Some("api.anthropic.com")
    }
}

/// Reads taste with a model.
#[derive(Debug, Clone)]
pub struct ModelInterpreter {
    config: ModelConfig,
    http: reqwest::Client,
}

/// Models that accept the server-side refusal fallback (`fallbacks:
/// "default"`).
const FALLBACK_MODELS: [&str; 4] = [
    "claude-fable-5-1",
    "claude-opus-5-5",
    "claude-opus-5",
    "claude-sonnet-5-5",
];

const MAX_TOKENS: u32 = 8000;

impl ModelInterpreter {
    pub fn new(config: ModelConfig) -> Result<Self, InterpretError> {
        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            .user_agent(concat!("jobhunt/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| InterpretError::Transport(e.to_string()))?;
        Ok(Self { config, http })
    }

    pub fn config(&self) -> &ModelConfig {
        &self.config
    }

    fn body(&self, request: &TasteRequest) -> Value {
        self.body_for(
            SYSTEM_PROMPT,
            &render(request),
            reading_schema(),
            "taste_reading",
        )
    }

    /// A request for a JSON answer of `schema`, after `system`.
    fn body_for(&self, system: &str, user: &str, schema: Value, name: &str) -> Value {
        match self.config.provider {
            Provider::Anthropic => {
                let mut body = json!({
                    "model": self.config.model,
                    "max_tokens": MAX_TOKENS,
                    "system": system,
                    "messages": [{"role": "user", "content": user}],
                    "output_config": {
                        "format": {"type": "json_schema", "schema": schema}
                    },
                });
                if let Some(effort) = &self.config.effort {
                    body["output_config"]["effort"] = json!(effort);
                }
                if self.config.is_claude_api()
                    && FALLBACK_MODELS.contains(&self.config.model.as_str())
                {
                    body["fallbacks"] = json!("default");
                }
                body
            }
            Provider::OpenAi => json!({
                "model": self.config.model,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ],
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {"name": name, "strict": true, "schema": schema},
                },
            }),
        }
    }

    async fn attempt(&self, body: &Value) -> Result<String, InterpretError> {
        self.answer(body).await.map(|a| a.text)
    }

    /// One call: the answer's text and the tokens it took.
    async fn answer(&self, body: &Value) -> Result<Answer, InterpretError> {
        let mut request = self
            .http
            .post(self.config.endpoint())
            .header("content-type", "application/json");
        match self.config.provider {
            Provider::Anthropic => {
                request = request.header("anthropic-version", "2023-06-01");
                if let Some(key) = &self.config.api_key {
                    request = request.header("x-api-key", key);
                }
                if body.get("fallbacks").is_some() {
                    request = request.header("anthropic-beta", "server-side-fallback-2026-07-01");
                }
            }
            Provider::OpenAi => {
                if let Some(key) = &self.config.api_key {
                    request = request.bearer_auth(key);
                }
            }
        }
        let response = request.body(body.to_string()).send().await.map_err(|e| {
            InterpretError::Transport(if e.is_timeout() {
                "timed out".to_owned()
            } else if e.is_connect() {
                "connection failed".to_owned()
            } else {
                "request failed".to_owned()
            })
        })?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(InterpretError::Status {
                status,
                retryable: matches!(status, 408 | 409 | 429) || status >= 500,
            });
        }
        let text = response
            .text()
            .await
            .map_err(|_| InterpretError::Transport("the answer could not be read".to_owned()))?;
        let answer: Value = serde_json::from_str(&text)
            .map_err(|_| InterpretError::Malformed("the response is not JSON".to_owned()))?;
        let usage = answer.get("usage");
        let tokens = |names: [&str; 2]| {
            names
                .iter()
                .find_map(|n| usage.and_then(|u| u.get(*n)).and_then(Value::as_u64))
                .unwrap_or(0)
        };
        let input_tokens = tokens(["input_tokens", "prompt_tokens"]);
        let output_tokens = tokens(["output_tokens", "completion_tokens"]);
        // OpenAI reports reasoning separately (and inside the output);
        // Anthropic doesn't.
        let reasoning_tokens = usage
            .and_then(|u| u.get("completion_tokens_details"))
            .and_then(|d| d.get("reasoning_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let text = match self.config.provider {
            Provider::Anthropic => anthropic_text(&answer),
            Provider::OpenAi => openai_text(&answer),
        }?;
        Ok(Answer {
            text,
            input_tokens,
            output_tokens,
            reasoning_tokens,
        })
    }

    /// One call for a JSON answer of `schema`, after `system`, with no
    /// retry and no validation: for offline experiments that bring their
    /// own prompt and checks (BRU-330). Nothing is logged.
    pub async fn structured(
        &self,
        system: &str,
        user: &str,
        schema: Value,
        name: &str,
    ) -> Result<Answer, InterpretError> {
        self.answer(&self.body_for(system, user, schema, name))
            .await
    }
}

/// A model's answer: its text and the tokens it took, when the provider
/// says.
#[derive(Debug, Clone)]
pub struct Answer {
    pub text: String,
    pub input_tokens: u64,
    /// Including any reasoning.
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
}

fn anthropic_text(answer: &Value) -> Result<String, InterpretError> {
    match answer.get("stop_reason").and_then(Value::as_str) {
        Some("refusal") => return Err(InterpretError::Refused),
        Some("max_tokens") => return Err(InterpretError::Truncated),
        _ => {}
    }
    let text: String = answer
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect();
    if text.trim().is_empty() {
        return Err(InterpretError::Malformed(
            "no text in the answer".to_owned(),
        ));
    }
    Ok(text)
}

fn openai_text(answer: &Value) -> Result<String, InterpretError> {
    let choice = answer
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .ok_or_else(|| InterpretError::Malformed("no choices in the answer".to_owned()))?;
    if choice.get("finish_reason").and_then(Value::as_str) == Some("length") {
        return Err(InterpretError::Truncated);
    }
    let message = choice.get("message");
    if message
        .and_then(|m| m.get("refusal"))
        .is_some_and(|r| !r.is_null())
    {
        return Err(InterpretError::Refused);
    }
    message
        .and_then(|m| m.get("content"))
        .and_then(Value::as_str)
        .filter(|c| !c.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| InterpretError::Malformed("no content in the answer".to_owned()))
}

#[async_trait]
impl TasteInterpreter for ModelInterpreter {
    fn name(&self) -> String {
        format!(
            "model/{}:{}",
            self.config.provider.as_str(),
            self.config.model
        )
    }

    fn is_remote(&self) -> bool {
        true
    }

    async fn interpret(&self, request: &TasteRequest) -> Result<TasteReading, InterpretError> {
        let body = self.body(request);
        let name = self.name();
        let mut attempt = 0;
        loop {
            let started = Instant::now();
            let result = self
                .attempt(&body)
                .await
                .and_then(|text| parse_reading(&text, request, &name));
            let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            match result {
                Ok(reading) => {
                    tracing::info!(
                        provider = self.config.provider.as_str(),
                        model = %self.config.model,
                        attempt,
                        elapsed_ms,
                        statements = reading.assertions.len(),
                        rejected = reading.rejected,
                        "taste interpreted"
                    );
                    return Ok(reading);
                }
                Err(error) => {
                    tracing::warn!(
                        provider = self.config.provider.as_str(),
                        model = %self.config.model,
                        attempt,
                        elapsed_ms,
                        %error,
                        "taste interpretation failed"
                    );
                    if !error.is_retryable() || attempt >= self.config.max_retries {
                        return Err(error);
                    }
                    attempt += 1;
                    tokio::time::sleep(self.config.backoff * 2u32.pow(attempt - 1)).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_defaults_and_errors() {
        let c = ModelConfig::new(
            "anthropic",
            None,
            None,
            Some("k".into()),
            "ANTHROPIC_API_KEY",
        )
        .unwrap();
        assert_eq!(c.model, "claude-opus-5-5");
        assert_eq!(c.endpoint(), "https://api.anthropic.com/v1/messages");
        assert!(c.is_claude_api());
        assert!(
            !format!("{c:?}").contains("\"k\""),
            "keys are never printed"
        );
        assert_eq!(
            ModelConfig::new("anthropic", None, None, None, "ANTHROPIC_API_KEY"),
            Err(ConfigError::Key {
                provider: "anthropic",
                env: "ANTHROPIC_API_KEY".into()
            })
        );
        assert_eq!(
            ModelConfig::new("openai", None, None, Some("k".into()), "OPENAI_API_KEY"),
            Err(ConfigError::Model("openai"))
        );
        // A self-hosted OpenAI-compatible server needs no key.
        let local = ModelConfig::new(
            "openai",
            Some("llama3.1"),
            Some("http://localhost:11434/v1"),
            None,
            "OPENAI_API_KEY",
        )
        .unwrap();
        assert_eq!(
            local.endpoint(),
            "http://localhost:11434/v1/chat/completions"
        );
        assert!(ModelConfig::new("gemini", None, None, None, "X").is_err());
        assert!(
            ModelConfig::new("anthropic", None, Some("ftp://x"), Some("k".into()), "X").is_err()
        );
    }
}

/// Reviews job fit with a model (see [`jobhunt_ranking::review`]): the same
/// providers, configuration, retries and logging as the taste reader. What
/// is sent is exactly [`jobhunt_ranking::review::render`] of the request,
/// after the fixed [`jobhunt_ranking::review::SYSTEM_PROMPT`]; prompts and
/// answers are never logged.
#[derive(Debug, Clone)]
pub struct ModelFitReviewer {
    model: ModelInterpreter,
}

impl ModelFitReviewer {
    pub fn new(config: ModelConfig) -> Result<Self, InterpretError> {
        Ok(Self {
            model: ModelInterpreter::new(config)?,
        })
    }

    pub fn config(&self) -> &ModelConfig {
        self.model.config()
    }
}

fn review_error(e: InterpretError) -> ReviewError {
    match e {
        InterpretError::Transport(why) => ReviewError::Transport(why),
        InterpretError::Status { status, retryable } => ReviewError::Status { status, retryable },
        InterpretError::Refused => ReviewError::Refused,
        InterpretError::Truncated => ReviewError::Truncated,
        other => ReviewError::Malformed(other.to_string()),
    }
}

#[async_trait]
impl FitReviewer for ModelFitReviewer {
    fn name(&self) -> String {
        self.model.name()
    }

    async fn review(&self, request: &FitReviewRequest) -> Result<FitReview, ReviewError> {
        let config = self.model.config();
        let body = self.model.body_for(
            review::SYSTEM_PROMPT,
            &review::render(request),
            review::review_schema(),
            "fit_review",
        );
        let name = self.name();
        let mut attempt = 0;
        loop {
            let started = Instant::now();
            let result = self
                .model
                .answer(&body)
                .await
                .map_err(review_error)
                .and_then(|answer| {
                    let mut review = review::parse_review(&answer.text, request, &name)?;
                    review.input_tokens = answer.input_tokens;
                    review.output_tokens = answer.output_tokens;
                    Ok(review)
                });
            let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            match result {
                Ok(review) => {
                    tracing::info!(
                        provider = config.provider.as_str(),
                        model = %config.model,
                        attempt,
                        elapsed_ms,
                        fit = review.fit.as_str(),
                        rejected = review.rejected,
                        input_tokens = review.input_tokens,
                        output_tokens = review.output_tokens,
                        "fit reviewed"
                    );
                    return Ok(review);
                }
                Err(error) => {
                    tracing::warn!(
                        provider = config.provider.as_str(),
                        model = %config.model,
                        attempt,
                        elapsed_ms,
                        %error,
                        "fit review failed"
                    );
                    if !error.is_retryable() || attempt >= config.max_retries {
                        return Err(error);
                    }
                    attempt += 1;
                    tokio::time::sleep(config.backoff * 2u32.pow(attempt - 1)).await;
                }
            }
        }
    }
}
