//! A cheap, synchronous view of which providers have stored credentials.
//!
//! Provider availability is read on the render path and while building picker
//! rows, where an async storage read would be inappropriate. The authoritative
//! owner of credentials publishes a snapshot here after every login, logout,
//! and startup restore; everyone else reads it.
//!
//! "Stored" means exactly that — a credential is on disk. It is not a promise
//! that a fresh authenticated request would succeed.

use std::collections::BTreeSet;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::provider_id::AuthProviderId;

/// Shared snapshot of the providers that currently have stored credentials.
///
/// Cloning shares the snapshot, so a publisher's update is visible to every
/// reader.
#[derive(Clone, Debug, Default)]
pub struct CredentialPresence {
    stored: Arc<RwLock<BTreeSet<AuthProviderId>>>,
}

impl CredentialPresence {
    /// Creates an empty snapshot.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether `provider` has a stored credential.
    #[must_use]
    pub fn has_credentials(&self, provider: AuthProviderId) -> bool {
        self.stored.read().contains(&provider)
    }

    /// The providers that have stored credentials.
    #[must_use]
    pub fn stored_providers(&self) -> Vec<AuthProviderId> {
        self.stored.read().iter().copied().collect()
    }

    /// Replaces the snapshot with the given set of providers.
    pub fn publish<I>(&self, providers: I)
    where
        I: IntoIterator<Item = AuthProviderId>,
    {
        *self.stored.write() = providers.into_iter().collect();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;

    #[rstest::rstest]
    fn an_empty_snapshot_reports_no_stored_credentials() {
        // Given a fresh snapshot.
        let presence = CredentialPresence::new();

        // When asking about a provider.
        // Then it has no stored credentials.
        assert!(!presence.has_credentials(AuthProviderId::OpenAiCodex));
    }

    #[rstest::rstest]
    fn a_published_snapshot_is_visible_through_clones() {
        // Given a snapshot shared with a clone.
        let presence = CredentialPresence::new();
        let reader = presence.clone();

        // When publishing a provider through the original.
        presence.publish([AuthProviderId::OpenAiCodex]);

        // Then the clone sees the provider as stored.
        assert!(reader.has_credentials(AuthProviderId::OpenAiCodex));
    }

    #[rstest::rstest]
    fn publishing_replaces_the_previous_snapshot() {
        // Given a snapshot listing one provider.
        let presence = CredentialPresence::new();
        presence.publish([AuthProviderId::OpenAiCodex]);

        // When publishing an empty snapshot, as logout does.
        presence.publish([]);

        // Then the provider is no longer reported as stored.
        assert!(!presence.has_credentials(AuthProviderId::OpenAiCodex));
    }
}
