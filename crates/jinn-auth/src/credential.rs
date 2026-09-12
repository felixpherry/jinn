//! Stored credential shapes.
//!
//! A credential is what jinn keeps on disk after a successful login: the
//! tokens needed to make subscription-backed requests, plus the account
//! metadata the upstream protocol requires. Credentials are secrets — their
//! [`Debug`] output is redacted so they cannot leak into logs or provider
//! debug dumps.

use std::fmt;

use serde::{Deserialize, Serialize};

/// An OAuth credential for a subscription provider.
///
/// `expires_at_ms` is the absolute expiry of `access_token` in Unix
/// milliseconds, matching the upstream protocol's `expires_in` arithmetic.
/// `account_id` is the upstream account the tokens belong to; requests carry
/// it alongside the bearer token.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthCredential {
    /// Short-lived bearer token sent with each model request.
    pub access_token: String,
    /// Long-lived token used to mint a fresh `access_token`.
    pub refresh_token: String,
    /// Absolute expiry of `access_token`, in Unix milliseconds.
    pub expires_at_ms: i64,
    /// Upstream account identifier extracted from the access token.
    pub account_id: String,
}

/// How long before nominal expiry a token is treated as already expired.
///
/// Refreshing slightly early avoids losing a request to a token that expires
/// while it is in flight.
const EXPIRY_SKEW_MS: i64 = 5 * 60_000;

impl OAuthCredential {
    /// Whether the access token is expired (or close enough to expiry that it
    /// should be refreshed before the next request).
    #[must_use]
    pub fn is_expired_at(&self, now_ms: i64) -> bool {
        self.expires_at_ms.saturating_sub(EXPIRY_SKEW_MS) <= now_ms
    }

    /// Whether the access token is expired as of now.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.is_expired_at(jiff::Timestamp::now().as_millisecond())
    }
}

impl fmt::Debug for OAuthCredential {
    /// Redacts both tokens. Only non-secret metadata is printable.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthCredential")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("expires_at_ms", &self.expires_at_ms)
            .field("account_id", &self.account_id)
            .finish()
    }
}

/// A credential as persisted, tagged by kind.
///
/// Only OAuth credentials exist today. The tag keeps the on-disk format open
/// for other credential kinds without a migration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StoredCredential {
    /// An OAuth credential obtained through a subscription login.
    Oauth(OAuthCredential),
}

impl StoredCredential {
    /// Borrows the OAuth payload.
    #[must_use]
    pub fn as_oauth(&self) -> &OAuthCredential {
        match self {
            Self::Oauth(credential) => credential,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;

    fn credential(expires_at_ms: i64) -> OAuthCredential {
        OAuthCredential {
            access_token: "access-secret".to_owned(),
            refresh_token: "refresh-secret".to_owned(),
            expires_at_ms,
            account_id: "acct-1".to_owned(),
        }
    }

    #[rstest::rstest]
    fn debug_output_hides_both_tokens() {
        // Given a credential holding secrets.
        let credential = credential(0);

        // When formatting it for diagnostics.
        let rendered = format!("{credential:?}");

        // Then neither secret appears in the output.
        assert!(
            !rendered.contains("access-secret"),
            "access token must not appear in debug output: {rendered}"
        );
        assert!(
            !rendered.contains("refresh-secret"),
            "refresh token must not appear in debug output: {rendered}"
        );
    }

    #[rstest::rstest]
    fn credential_past_its_expiry_is_expired() {
        // Given a credential that expired an hour ago.
        let credential = credential(1_000_000);

        // When checking expiry well after that instant.
        // Then it reports as expired.
        assert!(credential.is_expired_at(5_000_000));
    }

    #[rstest::rstest]
    fn credential_within_the_skew_window_is_expired() {
        // Given a credential that expires thirty seconds from now.
        let credential = credential(1_030_000);

        // When checking expiry now.
        // Then the skew window treats it as already expired.
        assert!(credential.is_expired_at(1_000_000));
    }

    #[rstest::rstest]
    fn credential_with_four_minutes_remaining_needs_refresh() {
        // Given a token inside pi's five-minute minimum validity window.
        let credential = credential(1_240_000);

        // When checking whether it is safe to start a request.
        let expired = credential.is_expired_at(1_000_000);

        // Then it requires refresh even though nominal expiry is in the future.
        assert!(expired);
    }

    #[rstest::rstest]
    fn credential_far_from_expiry_is_live() {
        // Given a credential that expires in an hour.
        let credential = credential(4_600_000);

        // When checking expiry now.
        // Then it is not expired.
        assert!(!credential.is_expired_at(1_000_000));
    }

    #[rstest::rstest]
    fn stored_credential_round_trips_through_json() {
        // Given a stored OAuth credential.
        let stored = StoredCredential::Oauth(credential(42));

        // When serializing and deserializing it.
        let json = serde_json::to_string(&stored).expect("serialize");
        let back: StoredCredential = serde_json::from_str(&json).expect("deserialize");

        // Then every field survives the round trip.
        assert_eq!(back, stored);
    }
}
