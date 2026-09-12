//! The shared entry point for subscription authentication.
//!
//! [`AuthService`] is what the rest of the application talks to. It knows the
//! registered providers, owns their credential storage, publishes which ones
//! are configured, and hands out request-time token providers. Adding another
//! subscription provider means registering one more adapter here — no other
//! caller changes.

use std::collections::BTreeMap;
use std::sync::Arc;

use error_stack::{Report, ResultExt as _};
use wherror::Error;

use crate::access_token::AccessTokenProvider;
use crate::auth_provider::{LoginFailure, SubscriptionAuthProvider};
use crate::credential::StoredCredential;
use crate::credential_presence::CredentialPresence;
use crate::credential_store::CredentialStoreService;
use crate::interaction::AuthInteraction;
use crate::login_method::LoginMethod;
use crate::provider_id::AuthProviderId;
use crate::token_provider::OAuthAccessTokenProvider;

/// Why an authentication operation did not complete.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AuthOperationFailure {
    /// No adapter is registered for the requested provider.
    #[error("unknown subscription provider")]
    UnknownProvider,
    /// The attempt was cancelled before it finished.
    #[error("login cancelled")]
    Cancelled,
    /// The provider refused the authorization.
    #[error("authorization was denied")]
    Denied,
    /// The authorization window elapsed before the user finished.
    #[error("authorization timed out")]
    TimedOut,
    /// The provider could not be reached, or replied unusably.
    #[error("provider error")]
    Provider,
    /// Credentials could not be read or written.
    #[error("credential storage failure")]
    Storage,
}

/// Registered subscription providers plus the storage behind them.
#[derive(Clone, Debug)]
pub struct AuthService {
    inner: Arc<AuthServiceInner>,
}

#[derive(Debug)]
struct AuthServiceInner {
    store: CredentialStoreService,
    providers: BTreeMap<AuthProviderId, Arc<dyn SubscriptionAuthProvider>>,
    token_providers: BTreeMap<AuthProviderId, Arc<dyn AccessTokenProvider>>,
    presence: CredentialPresence,
}

impl AuthService {
    /// Registers `providers` against `store`.
    #[must_use]
    pub fn new(
        store: CredentialStoreService,
        providers: Vec<Arc<dyn SubscriptionAuthProvider>>,
    ) -> Self {
        let providers: BTreeMap<AuthProviderId, Arc<dyn SubscriptionAuthProvider>> = providers
            .into_iter()
            .map(|provider| (provider.id(), provider))
            .collect();
        let token_providers = providers
            .iter()
            .map(|(id, provider)| {
                let tokens: Arc<dyn AccessTokenProvider> = Arc::new(OAuthAccessTokenProvider::new(
                    provider.clone(),
                    store.clone(),
                ));
                (*id, tokens)
            })
            .collect();

        Self {
            inner: Arc::new(AuthServiceInner {
                store,
                providers,
                token_providers,
                presence: CredentialPresence::new(),
            }),
        }
    }

    /// The providers the login picker offers, in presentation order.
    #[must_use]
    pub fn provider_ids(&self) -> Vec<AuthProviderId> {
        AuthProviderId::ALL
            .into_iter()
            .filter(|id| self.inner.providers.contains_key(id))
            .collect()
    }

    /// The login methods `provider` supports, or an empty slice when it is not
    /// registered.
    #[must_use]
    pub fn methods(&self, provider: AuthProviderId) -> &'static [LoginMethod] {
        self.inner
            .providers
            .get(&provider)
            .map_or(&[], |adapter| adapter.methods())
    }

    /// The shared snapshot of which providers have stored credentials.
    #[must_use]
    pub fn presence(&self) -> &CredentialPresence {
        &self.inner.presence
    }

    /// Whether `provider` currently has a stored credential.
    #[must_use]
    pub fn has_credentials(&self, provider: AuthProviderId) -> bool {
        self.inner.presence.has_credentials(provider)
    }

    /// The request-time token provider for `provider`.
    #[must_use]
    pub fn access_token_provider(
        &self,
        provider: AuthProviderId,
    ) -> Option<Arc<dyn AccessTokenProvider>> {
        self.inner.token_providers.get(&provider).cloned()
    }

    /// Republishes the credential snapshot from storage.
    ///
    /// Called at startup so a login performed in an earlier run is available
    /// immediately, and after any change to stored credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if credential storage cannot be read.
    pub async fn refresh_presence(&self) -> Result<(), Report<AuthOperationFailure>> {
        let stored = self
            .inner
            .store
            .list()
            .await
            .change_context(AuthOperationFailure::Storage)?;
        self.inner.presence.publish(stored);
        Ok(())
    }

    /// Runs an interactive login and, on success, persists the credential.
    ///
    /// Storage is written only after the provider returns a credential, so a
    /// failed or cancelled attempt leaves any previous login untouched.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider is unknown, the login does not
    /// complete, or the credential cannot be stored.
    pub async fn login(
        &self,
        provider: AuthProviderId,
        method: LoginMethod,
        interaction: Arc<dyn AuthInteraction>,
    ) -> Result<(), Report<AuthOperationFailure>> {
        let adapter = self
            .inner
            .providers
            .get(&provider)
            .ok_or_else(|| Report::new(AuthOperationFailure::UnknownProvider))?;

        let cancel = interaction.cancel_signal().clone();
        let credential = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(Report::new(AuthOperationFailure::Cancelled)),
            result = adapter.login(method, interaction) => result.map_err(map_login_failure)?,
        };

        {
            let mut transaction = tokio::select! {
                biased;
                () = cancel.cancelled() => return Err(Report::new(AuthOperationFailure::Cancelled)),
                result = self.inner.store.transaction() => result.change_context(AuthOperationFailure::Storage)?,
            };
            cancel
                .check()
                .change_context(AuthOperationFailure::Cancelled)?;
            transaction
                .write(provider, StoredCredential::Oauth(credential))
                .change_context(AuthOperationFailure::Storage)?;
        }

        self.refresh_presence().await
    }

    /// Removes the stored credential for `provider`.
    ///
    /// # Errors
    ///
    /// Returns an error if credential storage cannot be written.
    pub async fn logout(
        &self,
        provider: AuthProviderId,
    ) -> Result<(), Report<AuthOperationFailure>> {
        self.inner
            .store
            .delete(provider)
            .await
            .change_context(AuthOperationFailure::Storage)?;
        self.refresh_presence().await
    }
}

/// Maps a provider-level login failure onto the shared operation failure.
fn map_login_failure(failure: Report<LoginFailure>) -> Report<AuthOperationFailure> {
    let target = match failure.downcast_ref::<LoginFailure>() {
        Some(LoginFailure::Cancelled) => AuthOperationFailure::Cancelled,
        Some(LoginFailure::Denied) => AuthOperationFailure::Denied,
        Some(LoginFailure::TimedOut) => AuthOperationFailure::TimedOut,
        _ => AuthOperationFailure::Provider,
    };
    failure.change_context(target)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;
    use crate::auth_provider::RefreshFailure;
    use crate::credential::OAuthCredential;
    use crate::credential_store::InMemoryCredentialStore;
    use crate::interaction::{AuthCancelled, AuthEvent, CancelSignal};

    #[derive(Debug)]
    struct ScriptedProvider {
        outcome: Result<&'static str, LoginFailure>,
        cancel_before_return: bool,
    }

    impl ScriptedProvider {
        fn succeeding() -> Arc<Self> {
            Arc::new(Self {
                outcome: Ok("granted"),
                cancel_before_return: false,
            })
        }

        fn failing(failure: LoginFailure) -> Arc<Self> {
            Arc::new(Self {
                outcome: Err(failure),
                cancel_before_return: false,
            })
        }
    }

    #[async_trait::async_trait]
    impl SubscriptionAuthProvider for ScriptedProvider {
        fn id(&self) -> AuthProviderId {
            AuthProviderId::OpenAiCodex
        }

        fn methods(&self) -> &'static [LoginMethod] {
            &[LoginMethod::Browser, LoginMethod::DeviceCode]
        }

        async fn login(
            &self,
            _method: LoginMethod,
            interaction: Arc<dyn AuthInteraction>,
        ) -> Result<OAuthCredential, Report<LoginFailure>> {
            if self.cancel_before_return {
                interaction.cancel_signal().cancel();
            }
            match &self.outcome {
                Ok(access) => Ok(OAuthCredential {
                    access_token: (*access).to_owned(),
                    refresh_token: "refresh".to_owned(),
                    expires_at_ms: jiff::Timestamp::now().as_millisecond() + 3_600_000,
                    account_id: "acct-1".to_owned(),
                }),
                Err(LoginFailure::Cancelled) => Err(Report::new(LoginFailure::Cancelled)),
                Err(LoginFailure::Denied) => Err(Report::new(LoginFailure::Denied)),
                Err(LoginFailure::TimedOut) => Err(Report::new(LoginFailure::TimedOut)),
                Err(LoginFailure::Provider) => Err(Report::new(LoginFailure::Provider)),
            }
        }

        async fn refresh(
            &self,
            credential: &OAuthCredential,
        ) -> Result<OAuthCredential, Report<RefreshFailure>> {
            Ok(credential.clone())
        }
    }

    #[derive(Debug)]
    struct SilentInteraction {
        cancel: CancelSignal,
    }

    impl SilentInteraction {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                cancel: CancelSignal::new(),
            })
        }
    }

    #[async_trait::async_trait]
    impl AuthInteraction for SilentInteraction {
        fn notify(&self, _event: AuthEvent) {}

        async fn manual_code(&self) -> Result<String, AuthCancelled> {
            std::future::pending().await
        }

        fn cancel_signal(&self) -> &CancelSignal {
            &self.cancel
        }
    }

    fn service(provider: Arc<dyn SubscriptionAuthProvider>) -> AuthService {
        AuthService::new(
            CredentialStoreService::new(Arc::new(InMemoryCredentialStore::new())),
            vec![provider],
        )
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn a_successful_login_marks_the_provider_configured() {
        // Given a registered provider with no stored credential.
        let service = service(ScriptedProvider::succeeding());

        // When the user completes a login.
        service
            .login(
                AuthProviderId::OpenAiCodex,
                LoginMethod::Browser,
                SilentInteraction::new(),
            )
            .await
            .expect("login succeeds");

        // Then the provider reports stored credentials.
        assert!(service.has_credentials(AuthProviderId::OpenAiCodex));
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn a_cancelled_login_leaves_the_previous_account_in_place() {
        // Given a provider that already has a stored credential.
        let store = CredentialStoreService::new(Arc::new(InMemoryCredentialStore::new()));
        let succeeding = AuthService::new(store.clone(), vec![ScriptedProvider::succeeding()]);
        succeeding
            .login(
                AuthProviderId::OpenAiCodex,
                LoginMethod::Browser,
                SilentInteraction::new(),
            )
            .await
            .expect("first login succeeds");

        // When a re-login is cancelled.
        let cancelling = AuthService::new(
            store.clone(),
            vec![ScriptedProvider::failing(LoginFailure::Cancelled)],
        );
        let result = cancelling
            .login(
                AuthProviderId::OpenAiCodex,
                LoginMethod::Browser,
                SilentInteraction::new(),
            )
            .await;

        // Then the original credential is still stored.
        assert!(result.is_err(), "the cancelled login must not succeed");
        let stored = store
            .read(AuthProviderId::OpenAiCodex)
            .await
            .expect("read")
            .expect("credential present");
        assert_eq!(stored.as_oauth().access_token, "granted");
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn a_late_success_after_cancellation_preserves_the_previous_account() {
        // Given an adapter that returns success after the attempt was cancelled.
        let store = CredentialStoreService::new(Arc::new(InMemoryCredentialStore::new()));
        let previous = StoredCredential::Oauth(OAuthCredential {
            access_token: "previous".to_owned(),
            refresh_token: "previous-refresh".to_owned(),
            expires_at_ms: 0,
            account_id: "previous-account".to_owned(),
        });
        store
            .write(AuthProviderId::OpenAiCodex, previous.clone())
            .await
            .expect("seed");
        let service = AuthService::new(
            store.clone(),
            vec![Arc::new(ScriptedProvider {
                outcome: Ok("late-success"),
                cancel_before_return: true,
            })],
        );

        // When a non-cooperative adapter completes the cancelled re-login.
        let _ = service
            .login(
                AuthProviderId::OpenAiCodex,
                LoginMethod::Browser,
                SilentInteraction::new(),
            )
            .await;

        // Then no late credential replaces the previous account.
        assert_eq!(
            store.read(AuthProviderId::OpenAiCodex).await.expect("read"),
            Some(previous)
        );
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn logout_clears_the_stored_credential() {
        // Given a logged-in provider.
        let service = service(ScriptedProvider::succeeding());
        service
            .login(
                AuthProviderId::OpenAiCodex,
                LoginMethod::Browser,
                SilentInteraction::new(),
            )
            .await
            .expect("login succeeds");

        // When the user logs out.
        service
            .logout(AuthProviderId::OpenAiCodex)
            .await
            .expect("logout succeeds");

        // Then the provider no longer reports stored credentials.
        assert!(!service.has_credentials(AuthProviderId::OpenAiCodex));
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn restoring_presence_finds_credentials_from_a_previous_run() {
        // Given storage seeded as an earlier run would have left it.
        let store = CredentialStoreService::new(Arc::new(InMemoryCredentialStore::new()));
        store
            .write(
                AuthProviderId::OpenAiCodex,
                StoredCredential::Oauth(OAuthCredential {
                    access_token: "persisted".to_owned(),
                    refresh_token: "refresh".to_owned(),
                    expires_at_ms: 0,
                    account_id: "acct-1".to_owned(),
                }),
            )
            .await
            .expect("seed credential");
        let service = AuthService::new(store, vec![ScriptedProvider::succeeding()]);

        // When the application restores its credential snapshot at startup.
        service.refresh_presence().await.expect("restore succeeds");

        // Then the provider is already configured.
        assert!(service.has_credentials(AuthProviderId::OpenAiCodex));
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn logging_into_an_unregistered_provider_is_rejected() {
        // Given a service with no registered providers.
        let service = AuthService::new(
            CredentialStoreService::new(Arc::new(InMemoryCredentialStore::new())),
            vec![],
        );

        // When a login is attempted.
        let error = service
            .login(
                AuthProviderId::OpenAiCodex,
                LoginMethod::Browser,
                SilentInteraction::new(),
            )
            .await
            .expect_err("unknown provider");

        // Then the attempt is rejected as unknown.
        assert!(matches!(
            error.downcast_ref::<AuthOperationFailure>(),
            Some(AuthOperationFailure::UnknownProvider)
        ));
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn provider_ids_lists_only_registered_providers() {
        // Given a service with one registered provider.
        let service = service(ScriptedProvider::succeeding());

        // When listing the providers the login picker should offer.
        // Then only the registered provider appears.
        assert_eq!(service.provider_ids(), vec![AuthProviderId::OpenAiCodex]);
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn a_denied_login_reports_denial() {
        // Given a provider that refuses the authorization.
        let service = service(ScriptedProvider::failing(LoginFailure::Denied));

        // When the user attempts a login.
        let error = service
            .login(
                AuthProviderId::OpenAiCodex,
                LoginMethod::DeviceCode,
                SilentInteraction::new(),
            )
            .await
            .expect_err("denied");

        // Then the denial reaches the caller.
        assert!(matches!(
            error.downcast_ref::<AuthOperationFailure>(),
            Some(AuthOperationFailure::Denied)
        ));
    }
}
