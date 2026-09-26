//! API errors: an HTTP status plus the product's stable error codes.

use axum::Json;
use axum::response::{IntoResponse, Response};
use http::{HeaderValue, StatusCode, header};
use jobhunt_app::{AppError, ErrorKind};

use super::types::{ErrorBody, ErrorDetail};
use crate::auth::AuthError;

/// An error answer.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub hint: Option<String>,
    /// For 401s: the `WWW-Authenticate` challenge.
    pub challenge: Option<String>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            hint: None,
            challenge: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn not_found(what: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("{what} not found"),
        )
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request", message)
    }

    pub fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Narrow failed to answer. Try again; if it keeps failing, report the \
             request id.",
        )
    }

    /// A 401, with the challenge MCP clients use to discover how to sign in.
    pub fn unauthenticated(error: &AuthError, resource_metadata: Option<&str>) -> Self {
        let (status, code) = match error {
            AuthError::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "auth_unavailable"),
            _ => (StatusCode::UNAUTHORIZED, "unauthenticated"),
        };
        let mut challenge = String::from("Bearer realm=\"jobhunt\"");
        if let Some(url) = resource_metadata {
            challenge.push_str(&format!(", resource_metadata=\"{url}\""));
        }
        if !matches!(error, AuthError::Missing) {
            challenge.push_str(", error=\"invalid_token\"");
        }
        let message = match error {
            // The provider's details stay in the server log.
            AuthError::Unavailable(detail) => {
                tracing::warn!(%detail, "identity provider unavailable");
                "The identity provider could not be reached to check the token; try again."
                    .to_owned()
            }
            other => other.to_string(),
        };
        Self {
            status,
            code,
            message,
            hint: Some("Sign in with `jobhunt login`, or send a personal access token.".into()),
            challenge: (status == StatusCode::UNAUTHORIZED).then_some(challenge),
        }
    }
}

impl From<AppError> for ApiError {
    fn from(error: AppError) -> Self {
        let kind = error.kind();
        let status = match kind {
            ErrorKind::UnknownOpportunity | ErrorKind::NoJobs => StatusCode::NOT_FOUND,
            ErrorKind::AmbiguousId | ErrorKind::Conflict | ErrorKind::NoProfile => {
                StatusCode::CONFLICT
            }
            ErrorKind::InvalidPreference | ErrorKind::InvalidImport => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            ErrorKind::InvalidArguments => StatusCode::BAD_REQUEST,
            ErrorKind::SourceUnavailable
            | ErrorKind::VerificationUnavailable
            | ErrorKind::CloudUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            ErrorKind::Unauthenticated => StatusCode::UNAUTHORIZED,
            ErrorKind::Config | ErrorKind::Storage => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            // The cause (which may name internals) stays in the log.
            tracing::error!(error = %jobhunt_core::ErrorChain(&error), code = kind.as_str(), "use case failed");
            let mut e = Self::internal();
            e.code = kind.as_str();
            return e;
        }
        Self {
            status,
            code: kind.as_str(),
            message: error.public_message(),
            hint: error.hint().map(str::to_owned),
            challenge: None,
        }
    }
}

impl From<jobhunt_jobs::StorageError> for ApiError {
    fn from(error: jobhunt_jobs::StorageError) -> Self {
        AppError::from(error).into()
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            error: ErrorDetail {
                code: self.code.to_owned(),
                message: self.message,
                hint: self.hint,
            },
        };
        let mut response = (self.status, Json(body)).into_response();
        if let Some(challenge) = self.challenge
            && let Ok(value) = HeaderValue::from_str(&challenge)
        {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, value);
        }
        response
    }
}
