use std::error::Error;
use std::fmt;

/// Type-erased error used to carry an underlying cause across crate
/// boundaries without leaking the concrete dependency (reqwest, sqlx, ...)
/// into domain crates.
pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

/// Displays an error followed by its full `source()` chain, separated by
/// `": "`. Useful for single-line log fields and user-facing messages.
pub struct ErrorChain<'a>(pub &'a (dyn Error + 'static));

impl fmt::Display for ErrorChain<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        let mut current = self.0.source();
        while let Some(cause) = current {
            write!(f, ": {cause}")?;
            current = cause.source();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, thiserror::Error)]
    #[error("outer")]
    struct Outer(#[source] Inner);

    #[derive(Debug, thiserror::Error)]
    #[error("inner")]
    struct Inner;

    #[test]
    fn renders_full_chain() {
        let err = Outer(Inner);
        assert_eq!(ErrorChain(&err).to_string(), "outer: inner");
    }
}
