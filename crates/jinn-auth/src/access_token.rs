//! Request-time access to a subscription provider's bearer token.
//!
//! The model transport asks for a token immediately before each request. The
//! implementation behind this trait owns refresh: a caller never sees an
//! expired token, and never has to know whether a refresh happened.

use error_stack::Report;
use wherror::Error;

/// Raised when no usable access token can be produced.
#[derive(Debug, Error)]
pub enum AccessTokenError {
    /// No credential is stored for this provider; the user must log in.
    #[error("not logged in")]
    NotAuthenticated,
    /// A credential is stored but can no longer be refreshed; the user must
    /// log in again.
    #[error("stored credentials are no longer valid")]
    Expired,
    /// Credential storage could not be read or written.
    #[error("credential storage failure")]
    Storage,
    /// The provider could not be reached while refreshing.
    #[error("provider error")]
    Provider,
}

/// A bearer token plus the account it belongs to.
///
/// [`Debug`] is redacted: the token never reaches logs or request dumps.
#[derive(Clone, PartialEq, Eq)]
pub struct AccessToken {
    /// Bearer token for the `Authorization` header.
    pub token: String,
    /// Upstream account identifier the request must be attributed to.
    pub account_id: String,
}

impl std::fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccessToken")
            .field("token", &"<redacted>")
            .field("account_id", &self.account_id)
            .finish()
    }
}

/// Produces a currently-valid access token, refreshing when needed.
#[async_trait::async_trait]
pub trait AccessTokenProvider: Send + Sync + std::fmt::Debug {
    /// Human-readable name for diagnostics.
    fn name(&self) -> &'static str;

    /// Returns a token valid for the request about to be sent.
    ///
    /// # Errors
    ///
    /// Returns an error when no credential is stored, the stored credential
    /// can no longer be refreshed, or storage or the provider is unreachable.
    async fn access_token(&self) -> Result<AccessToken, Report<AccessTokenError>>;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;

    #[rstest::rstest]
    fn debug_output_hides_the_bearer_token() {
        // Given an access token.
        let token = AccessToken {
            token: "bearer-secret".to_owned(),
            account_id: "acct-1".to_owned(),
        };

        // When formatting it for diagnostics.
        let rendered = format!("{token:?}");

        // Then the token value does not appear.
        assert!(
            !rendered.contains("bearer-secret"),
            "bearer token must not appear in debug output: {rendered}"
        );
    }
}
