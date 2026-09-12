//! Test doubles for subscription authentication.
//!
//! These let other crates exercise login, logout, refresh, and provider
//! availability without contacting a real authorization service.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use error_stack::Report;
use parking_lot::Mutex;

use crate::auth_provider::{LoginFailure, RefreshFailure, SubscriptionAuthProvider};
use crate::auth_service::AuthService;
use crate::credential::{OAuthCredential, StoredCredential};
use crate::credential_store::{CredentialStoreService, InMemoryCredentialStore};
use crate::interaction::{AuthCancelled, AuthEvent, AuthInteraction, CancelSignal};
use crate::login_method::LoginMethod;
use crate::provider_id::AuthProviderId;

/// Builds a credential that is valid for the next hour.
#[must_use]
pub fn live_credential(access_token: &str) -> OAuthCredential {
    OAuthCredential {
        access_token: access_token.to_owned(),
        refresh_token: "fake-refresh".to_owned(),
        expires_at_ms: jiff::Timestamp::now().as_millisecond() + 3_600_000,
        account_id: "fake-account".to_owned(),
    }
}

/// What a [`FakeSubscriptionAuthProvider`] does when asked to log in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeLoginOutcome {
    /// Return a credential.
    Succeeds,
    /// Report that the attempt was cancelled.
    Cancelled,
    /// Report that the provider refused the authorization.
    Denied,
    /// Report that the provider could not be reached.
    Unreachable,
    /// Never resolve, as a login waiting on the user does.
    Pending,
}

/// A subscription provider whose behaviour is scripted by the test.
#[derive(Debug)]
pub struct FakeSubscriptionAuthProvider {
    id: AuthProviderId,
    outcome: Mutex<FakeLoginOutcome>,
    access_token: Mutex<String>,
    login_attempts: AtomicUsize,
}

impl FakeSubscriptionAuthProvider {
    /// Creates a provider that logs in successfully.
    #[must_use]
    pub fn succeeding(id: AuthProviderId) -> Arc<Self> {
        Self::with_outcome(id, FakeLoginOutcome::Succeeds)
    }

    /// Creates a provider with a scripted login outcome.
    #[must_use]
    pub fn with_outcome(id: AuthProviderId, outcome: FakeLoginOutcome) -> Arc<Self> {
        Arc::new(Self {
            id,
            outcome: Mutex::new(outcome),
            access_token: Mutex::new("fake-access".to_owned()),
            login_attempts: AtomicUsize::new(0),
        })
    }

    /// Changes what the next login does.
    pub fn set_outcome(&self, outcome: FakeLoginOutcome) {
        *self.outcome.lock() = outcome;
    }

    /// Changes the access token the next successful login returns.
    pub fn set_access_token(&self, access_token: &str) {
        access_token.clone_into(&mut self.access_token.lock());
    }

    /// How many logins have been attempted.
    #[must_use]
    pub fn login_attempts(&self) -> usize {
        self.login_attempts.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl SubscriptionAuthProvider for FakeSubscriptionAuthProvider {
    fn id(&self) -> AuthProviderId {
        self.id
    }

    fn methods(&self) -> &'static [LoginMethod] {
        &[LoginMethod::Browser, LoginMethod::DeviceCode]
    }

    async fn login(
        &self,
        _method: LoginMethod,
        interaction: Arc<dyn AuthInteraction>,
    ) -> Result<OAuthCredential, Report<LoginFailure>> {
        self.login_attempts.fetch_add(1, Ordering::SeqCst);
        let outcome = *self.outcome.lock();
        match outcome {
            FakeLoginOutcome::Succeeds => Ok(live_credential(&self.access_token.lock().clone())),
            FakeLoginOutcome::Cancelled => Err(Report::new(LoginFailure::Cancelled)),
            FakeLoginOutcome::Denied => Err(Report::new(LoginFailure::Denied)),
            FakeLoginOutcome::Unreachable => Err(Report::new(LoginFailure::Provider)),
            FakeLoginOutcome::Pending => {
                interaction.cancel_signal().cancelled().await;
                Err(Report::new(LoginFailure::Cancelled))
            }
        }
    }

    async fn refresh(
        &self,
        credential: &OAuthCredential,
    ) -> Result<OAuthCredential, Report<RefreshFailure>> {
        Ok(credential.clone())
    }
}

/// An interaction that records events and never supplies a pasted code.
#[derive(Debug, Default)]
pub struct FakeAuthInteraction {
    events: Mutex<Vec<AuthEvent>>,
    cancel: CancelSignal,
}

impl FakeAuthInteraction {
    /// Creates an interaction that records what a flow reports.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Every event reported so far, in order.
    #[must_use]
    pub fn events(&self) -> Vec<AuthEvent> {
        self.events.lock().clone()
    }
}

#[async_trait::async_trait]
impl AuthInteraction for FakeAuthInteraction {
    fn notify(&self, event: AuthEvent) {
        self.events.lock().push(event);
    }

    async fn manual_code(&self) -> Result<String, AuthCancelled> {
        std::future::pending().await
    }

    fn cancel_signal(&self) -> &CancelSignal {
        &self.cancel
    }
}

/// Builds an [`AuthService`] backed by in-memory storage and a fake provider.
#[must_use]
pub fn fake_auth_service() -> (AuthService, Arc<FakeSubscriptionAuthProvider>) {
    let provider = FakeSubscriptionAuthProvider::succeeding(AuthProviderId::OpenAiCodex);
    let store = CredentialStoreService::new(Arc::new(InMemoryCredentialStore::new()));
    (AuthService::new(store, vec![provider.clone()]), provider)
}

/// Builds an [`AuthService`] that already holds a credential for `provider`.
///
/// # Panics
///
/// Panics if the in-memory store rejects the seeded credential, which it
/// cannot.
#[must_use]
#[expect(
    clippy::unreachable,
    reason = "the in-memory store this helper builds cannot fail"
)]
pub async fn fake_auth_service_logged_in(
    provider: AuthProviderId,
) -> (AuthService, Arc<FakeSubscriptionAuthProvider>) {
    let adapter = FakeSubscriptionAuthProvider::succeeding(provider);
    let store = CredentialStoreService::new(Arc::new(InMemoryCredentialStore::new()));
    store
        .write(
            provider,
            StoredCredential::Oauth(live_credential("fake-access")),
        )
        .await
        .unwrap_or_else(|_| unreachable!("the in-memory store never fails"));
    let service = AuthService::new(store, vec![adapter.clone()]);
    service
        .refresh_presence()
        .await
        .unwrap_or_else(|_| unreachable!("the in-memory store never fails"));
    (service, adapter)
}
