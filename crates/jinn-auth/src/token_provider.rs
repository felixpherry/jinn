//! Turning a stored credential into a usable bearer token.
//!
//! Every subscription request goes through here. A live token is handed back
//! as-is; an expired one is refreshed, persisted, and then handed back. The
//! refresh runs under an exclusive storage lock and re-reads storage after taking
//! it, so several concurrent requests produce at most one refresh and none of
//! them observes a token that was rotated out from under it.
//!
//! When a credential can no longer be refreshed, the request fails. The stored
//! credential is left alone: logging back in is an explicit user action, never
//! something a failing request triggers.

use std::sync::Arc;

use error_stack::{Report, ResultExt as _};

use crate::access_token::{AccessToken, AccessTokenError, AccessTokenProvider};
use crate::auth_provider::{RefreshFailure, SubscriptionAuthProvider};
use crate::credential::{OAuthCredential, StoredCredential};
use crate::credential_store::CredentialStoreService;
use crate::provider_id::AuthProviderId;

/// Produces access tokens for one subscription provider.
#[derive(derive_more::Debug)]
pub struct OAuthAccessTokenProvider {
    provider: Arc<dyn SubscriptionAuthProvider>,
    store: CredentialStoreService,
}

impl OAuthAccessTokenProvider {
    /// Creates a token provider for `provider`, backed by `store`.
    #[must_use]
    pub fn new(provider: Arc<dyn SubscriptionAuthProvider>, store: CredentialStoreService) -> Self {
        Self { provider, store }
    }

    fn id(&self) -> AuthProviderId {
        self.provider.id()
    }

    async fn stored_credential(&self) -> Result<OAuthCredential, Report<AccessTokenError>> {
        self.store
            .read(self.id())
            .await
            .change_context(AccessTokenError::Storage)?
            .map(|stored| stored.as_oauth().clone())
            .ok_or_else(|| Report::new(AccessTokenError::NotAuthenticated))
    }
}

#[async_trait::async_trait]
impl AccessTokenProvider for OAuthAccessTokenProvider {
    fn name(&self) -> &'static str {
        "oauth"
    }

    async fn access_token(&self) -> Result<AccessToken, Report<AccessTokenError>> {
        let credential = self.stored_credential().await?;
        if !credential.is_expired() {
            return Ok(token_from(&credential));
        }

        let mut transaction = self
            .store
            .transaction()
            .await
            .change_context(AccessTokenError::Storage)?;

        // The storage lock spans refresh and persistence across processes, and
        // serializes logout/re-login with refresh-token rotation.
        let credential = transaction
            .read(self.id())
            .change_context(AccessTokenError::Storage)?
            .map(|stored| stored.as_oauth().clone())
            .ok_or_else(|| Report::new(AccessTokenError::NotAuthenticated))?;
        if !credential.is_expired() {
            return Ok(token_from(&credential));
        }

        let refreshed = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            self.provider.refresh(&credential),
        )
        .await
        .change_context(AccessTokenError::Provider)
        .attach("credential refresh timed out")?
        .map_err(map_refresh_failure)?;

        transaction
            .write(self.id(), StoredCredential::Oauth(refreshed.clone()))
            .change_context(AccessTokenError::Storage)?;

        Ok(token_from(&refreshed))
    }
}

/// Projects a credential onto the request-time token pair.
fn token_from(credential: &OAuthCredential) -> AccessToken {
    AccessToken {
        token: credential.access_token.clone(),
        account_id: credential.account_id.clone(),
    }
}

/// Maps a refresh failure onto the reason the request cannot proceed.
fn map_refresh_failure(failure: Report<RefreshFailure>) -> Report<AccessTokenError> {
    let target = match failure.downcast_ref::<RefreshFailure>() {
        Some(RefreshFailure::Rejected) => AccessTokenError::Expired,
        _ => AccessTokenError::Provider,
    };
    failure.change_context(target)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::auth_provider::LoginFailure;
    use crate::credential_store::InMemoryCredentialStore;
    use crate::interaction::AuthInteraction;
    use crate::login_method::LoginMethod;

    /// A provider whose refresh outcome is scripted by the test.
    #[derive(Debug)]
    struct ScriptedProvider {
        refreshes: AtomicUsize,
        outcome: RefreshOutcome,
    }

    #[derive(Debug, Clone, Copy)]
    enum RefreshOutcome {
        Rotates,
        Rejects,
        Unreachable,
    }

    impl ScriptedProvider {
        fn new(outcome: RefreshOutcome) -> Arc<Self> {
            Arc::new(Self {
                refreshes: AtomicUsize::new(0),
                outcome,
            })
        }
    }

    #[async_trait::async_trait]
    impl SubscriptionAuthProvider for ScriptedProvider {
        fn id(&self) -> AuthProviderId {
            AuthProviderId::OpenAiCodex
        }

        fn methods(&self) -> &'static [LoginMethod] {
            &[LoginMethod::Browser]
        }

        async fn login(
            &self,
            _method: LoginMethod,
            _interaction: Arc<dyn AuthInteraction>,
        ) -> Result<OAuthCredential, Report<LoginFailure>> {
            Err(Report::new(LoginFailure::Provider))
        }

        async fn refresh(
            &self,
            _credential: &OAuthCredential,
        ) -> Result<OAuthCredential, Report<RefreshFailure>> {
            self.refreshes.fetch_add(1, Ordering::SeqCst);
            // Yield so concurrent callers genuinely overlap.
            tokio::task::yield_now().await;
            match self.outcome {
                RefreshOutcome::Rotates => Ok(credential("rotated", far_future())),
                RefreshOutcome::Rejects => Err(Report::new(RefreshFailure::Rejected)),
                RefreshOutcome::Unreachable => Err(Report::new(RefreshFailure::Provider)),
            }
        }
    }

    fn far_future() -> i64 {
        jiff::Timestamp::now().as_millisecond() + 3_600_000
    }

    fn long_past() -> i64 {
        jiff::Timestamp::now().as_millisecond() - 3_600_000
    }

    fn credential(access: &str, expires_at_ms: i64) -> OAuthCredential {
        OAuthCredential {
            access_token: access.to_owned(),
            refresh_token: "refresh".to_owned(),
            expires_at_ms,
            account_id: "acct-1".to_owned(),
        }
    }

    async fn store_with(credential: Option<OAuthCredential>) -> CredentialStoreService {
        let store = CredentialStoreService::new(Arc::new(InMemoryCredentialStore::new()));
        if let Some(credential) = credential {
            store
                .write(
                    AuthProviderId::OpenAiCodex,
                    StoredCredential::Oauth(credential),
                )
                .await
                .expect("seed credential");
        }
        store
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn a_live_credential_is_used_without_refreshing() {
        // Given a stored credential that has not expired.
        let provider = ScriptedProvider::new(RefreshOutcome::Rotates);
        let store = store_with(Some(credential("live", far_future()))).await;
        let tokens = OAuthAccessTokenProvider::new(provider.clone(), store);

        // When asking for an access token.
        let token = tokens.access_token().await.expect("token");

        // Then the stored token is used as-is.
        assert_eq!(token.token, "live");
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn an_expired_credential_is_refreshed() {
        // Given a stored credential that has expired.
        let provider = ScriptedProvider::new(RefreshOutcome::Rotates);
        let store = store_with(Some(credential("stale", long_past()))).await;
        let tokens = OAuthAccessTokenProvider::new(provider, store);

        // When asking for an access token.
        let token = tokens.access_token().await.expect("token");

        // Then the refreshed token is returned.
        assert_eq!(token.token, "rotated");
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn a_rotated_credential_is_persisted() {
        // Given a stored credential that has expired.
        let provider = ScriptedProvider::new(RefreshOutcome::Rotates);
        let store = store_with(Some(credential("stale", long_past()))).await;
        let tokens = OAuthAccessTokenProvider::new(provider, store.clone());

        // When a request triggers a refresh.
        tokens.access_token().await.expect("token");

        // Then the rotated credential is what remains in storage.
        let stored = store
            .read(AuthProviderId::OpenAiCodex)
            .await
            .expect("read")
            .expect("credential present");
        assert_eq!(stored.as_oauth().access_token, "rotated");
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn concurrent_requests_refresh_only_once() {
        // Given an expired credential and several simultaneous requests.
        let provider = ScriptedProvider::new(RefreshOutcome::Rotates);
        let store = store_with(Some(credential("stale", long_past()))).await;
        let tokens = Arc::new(OAuthAccessTokenProvider::new(provider.clone(), store));

        // When all of them ask for an access token at once.
        let requests: Vec<_> = std::iter::repeat_with(|| {
            let tokens = tokens.clone();
            tokio::spawn(async move { tokens.access_token().await.map(|token| token.token) })
        })
        .take(4)
        .collect();
        for request in requests {
            let token = request.await.expect("task").expect("token");
            assert_eq!(token, "rotated");
        }

        // Then the provider was asked to refresh exactly once.
        assert_eq!(provider.refreshes.load(Ordering::SeqCst), 1);
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn a_missing_credential_reports_that_login_is_needed() {
        // Given no stored credential.
        let provider = ScriptedProvider::new(RefreshOutcome::Rotates);
        let store = store_with(None).await;
        let tokens = OAuthAccessTokenProvider::new(provider, store);

        // When asking for an access token.
        let error = tokens.access_token().await.expect_err("no credential");

        // Then the caller is told authentication is missing.
        assert!(matches!(
            error.downcast_ref::<AccessTokenError>(),
            Some(AccessTokenError::NotAuthenticated)
        ));
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn a_rejected_refresh_reports_expired_credentials() {
        // Given an expired credential the provider will not renew.
        let provider = ScriptedProvider::new(RefreshOutcome::Rejects);
        let store = store_with(Some(credential("stale", long_past()))).await;
        let tokens = OAuthAccessTokenProvider::new(provider, store);

        // When asking for an access token.
        let error = tokens.access_token().await.expect_err("refresh rejected");

        // Then the caller is told the stored credentials are no longer usable.
        assert!(matches!(
            error.downcast_ref::<AccessTokenError>(),
            Some(AccessTokenError::Expired)
        ));
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn a_rejected_refresh_leaves_the_stored_credential_alone() {
        // Given an expired credential the provider will not renew.
        let provider = ScriptedProvider::new(RefreshOutcome::Rejects);
        let store = store_with(Some(credential("stale", long_past()))).await;
        let tokens = OAuthAccessTokenProvider::new(provider, store.clone());

        // When a request fails on refresh.
        let _ = tokens.access_token().await;

        // Then logging back in stays an explicit action: the record survives.
        let stored = store.read(AuthProviderId::OpenAiCodex).await.expect("read");
        assert!(
            stored.is_some(),
            "a failed refresh must not silently log the user out"
        );
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn an_unreachable_provider_reports_a_provider_error() {
        // Given an expired credential and an unreachable provider.
        let provider = ScriptedProvider::new(RefreshOutcome::Unreachable);
        let store = store_with(Some(credential("stale", long_past()))).await;
        let tokens = OAuthAccessTokenProvider::new(provider, store);

        // When asking for an access token.
        let error = tokens
            .access_token()
            .await
            .expect_err("provider unreachable");

        // Then the failure is reported as a provider problem, not a logout.
        assert!(matches!(
            error.downcast_ref::<AccessTokenError>(),
            Some(AccessTokenError::Provider)
        ));
    }
}
