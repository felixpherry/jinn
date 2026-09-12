//! Endpoint layout for OpenAI Codex authentication.
//!
//! The real deployment lives under a single authorization host. Keeping the
//! host injectable lets contract tests point the same flow at a local mock
//! server without changing any of the protocol logic.

/// Default OpenAI authorization host.
pub const DEFAULT_AUTH_BASE_URL: &str = "https://auth.openai.com";

/// Default local redirect the browser flow listens on.
pub const DEFAULT_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";

/// Default address the local callback listener binds.
pub const DEFAULT_CALLBACK_BIND_ADDR: &str = "127.0.0.1:1455";

/// Path the provider redirects to after browser authorization.
pub const CALLBACK_PATH: &str = "/auth/callback";

/// OAuth client id the Codex CLI flow is registered under.
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

/// Scopes requested during authorization. `offline_access` is what makes
/// refresh — and therefore uninterrupted normal use — possible.
pub const SCOPE: &str = "openid profile email offline_access";

/// Client identifier sent with authorization and model requests.
pub const ORIGINATOR: &str = "jinn";

/// URLs for one OpenAI Codex authorization deployment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexAuthEndpoints {
    /// Authorization host, without a trailing slash.
    pub auth_base_url: String,
    /// Redirect the browser flow registers with the provider.
    pub redirect_uri: String,
    /// Address the local callback listener binds.
    pub callback_bind_addr: String,
}

impl Default for CodexAuthEndpoints {
    fn default() -> Self {
        Self {
            auth_base_url: DEFAULT_AUTH_BASE_URL.to_owned(),
            redirect_uri: DEFAULT_REDIRECT_URI.to_owned(),
            callback_bind_addr: DEFAULT_CALLBACK_BIND_ADDR.to_owned(),
        }
    }
}

impl CodexAuthEndpoints {
    /// Points every authorization URL at `auth_base_url`, leaving the local
    /// callback settings at their defaults.
    #[must_use]
    pub fn with_auth_base_url(auth_base_url: &str) -> Self {
        Self {
            auth_base_url: auth_base_url.trim_end_matches('/').to_owned(),
            ..Self::default()
        }
    }

    /// Authorization page the user visits.
    #[must_use]
    pub fn authorize_url(&self) -> String {
        format!("{}/oauth/authorize", self.base())
    }

    /// Token exchange and refresh endpoint.
    #[must_use]
    pub fn token_url(&self) -> String {
        format!("{}/oauth/token", self.base())
    }

    /// Endpoint that mints a device user code.
    #[must_use]
    pub fn device_user_code_url(&self) -> String {
        format!("{}/api/accounts/deviceauth/usercode", self.base())
    }

    /// Endpoint polled while waiting for device authorization.
    #[must_use]
    pub fn device_token_url(&self) -> String {
        format!("{}/api/accounts/deviceauth/token", self.base())
    }

    /// Page where the user enters the device code.
    #[must_use]
    pub fn device_verification_uri(&self) -> String {
        format!("{}/codex/device", self.base())
    }

    /// Redirect the device flow exchanges its authorization code against.
    #[must_use]
    pub fn device_redirect_uri(&self) -> String {
        format!("{}/deviceauth/callback", self.base())
    }

    fn base(&self) -> &str {
        self.auth_base_url.trim_end_matches('/')
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;

    #[rstest::rstest]
    fn endpoints_hang_off_the_configured_host() {
        // Given endpoints pointed at a local mock host.
        let endpoints = CodexAuthEndpoints::with_auth_base_url("http://localhost:9000");

        // When building the token URL.
        // Then it is rooted at that host.
        assert_eq!(endpoints.token_url(), "http://localhost:9000/oauth/token");
    }

    #[rstest::rstest]
    fn a_trailing_slash_does_not_double_up() {
        // Given a host written with a trailing slash.
        let endpoints = CodexAuthEndpoints::with_auth_base_url("http://localhost:9000/");

        // When building the authorize URL.
        // Then the path has exactly one separator.
        assert_eq!(
            endpoints.authorize_url(),
            "http://localhost:9000/oauth/authorize"
        );
    }
}
