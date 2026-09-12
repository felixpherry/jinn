//! Identity of a built-in subscription authentication provider.
//!
//! A subscription provider is an account the user already pays for (a ChatGPT
//! plan, for example) that jinn can authenticate against instead of asking for
//! a separately billed API key. Each provider has a stable wire id used as the
//! credential-storage key and as the provider block name in the model picker.

use std::fmt;

use serde::{Deserialize, Serialize};
use wherror::Error;

/// Wire id for the OpenAI Codex (ChatGPT subscription) provider.
pub const OPENAI_CODEX_ID: &str = "openai-codex";

/// Raised when a stored id does not name a built-in subscription provider.
#[derive(Debug, Error)]
#[error("unknown subscription auth provider")]
pub struct UnknownAuthProviderId;

/// A built-in subscription authentication provider.
///
/// The enum is closed on purpose: subscription support ships with jinn rather
/// than being loaded from user configuration. Adding a provider means adding a
/// variant plus an adapter, and every shared piece (picker, storage, refresh)
/// keeps working unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthProviderId {
    /// OpenAI Codex, backed by a ChatGPT subscription.
    OpenAiCodex,
}

impl AuthProviderId {
    /// Every provider, in the order the login picker lists them.
    pub const ALL: [Self; 1] = [Self::OpenAiCodex];

    /// Stable wire id, used as the credential-storage key.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiCodex => OPENAI_CODEX_ID,
        }
    }

    /// Human-readable name shown in the login and logout pickers.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::OpenAiCodex => "OpenAI Codex",
        }
    }

    /// Short description of which account backs this provider.
    #[must_use]
    pub const fn account_description(self) -> &'static str {
        match self {
            Self::OpenAiCodex => "ChatGPT Plus/Pro subscription",
        }
    }
}

impl fmt::Display for AuthProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for AuthProviderId {
    type Err = UnknownAuthProviderId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            OPENAI_CODEX_ID => Ok(Self::OpenAiCodex),
            _ => Err(UnknownAuthProviderId),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;

    #[rstest::rstest]
    fn openai_codex_parses_from_its_wire_id() {
        // Given the OpenAI Codex wire id.
        // When parsing it.
        let parsed: AuthProviderId = OPENAI_CODEX_ID.parse().expect("known provider id");

        // Then it resolves to the OpenAI Codex provider.
        assert_eq!(parsed, AuthProviderId::OpenAiCodex);
    }

    #[rstest::rstest]
    fn unknown_id_is_rejected() {
        // Given an id that names no built-in provider.
        // When parsing it.
        let parsed = "anthropic-subscription".parse::<AuthProviderId>();

        // Then parsing fails.
        assert!(parsed.is_err(), "unknown ids must not resolve");
    }

    #[rstest::rstest]
    fn display_round_trips_through_from_str() {
        // Given every built-in provider.
        for provider in AuthProviderId::ALL {
            // When formatting and re-parsing it.
            let round_tripped: AuthProviderId =
                provider.to_string().parse().expect("display round-trips");

            // Then the same provider comes back.
            assert_eq!(round_tripped, provider);
        }
    }
}
