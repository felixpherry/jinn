//! The provider-specific half of subscription authentication.
//!
//! Everything shared — the picker flow, credential storage, refresh
//! scheduling, cancellation — lives outside this trait. A new subscription
//! provider only has to describe its login methods and implement the two
//! network operations below.

use std::sync::Arc;

use error_stack::Report;
use wherror::Error;

use crate::credential::OAuthCredential;
use crate::interaction::AuthInteraction;
use crate::login_method::LoginMethod;
use crate::provider_id::AuthProviderId;

/// Raised when a login attempt does not produce a credential.
#[derive(Debug, Error)]
pub enum LoginFailure {
    /// The user (or the application) cancelled the attempt.
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
}

/// Raised when a stored credential cannot be exchanged for a fresh one.
#[derive(Debug, Error)]
pub enum RefreshFailure {
    /// The refresh token is no longer accepted; the user must log in again.
    #[error("stored credentials are no longer valid")]
    Rejected,
    /// The provider could not be reached, or replied unusably.
    #[error("provider error")]
    Provider,
}

/// A built-in subscription provider jinn can authenticate against.
#[async_trait::async_trait]
pub trait SubscriptionAuthProvider: Send + Sync + std::fmt::Debug {
    /// Which provider this adapter implements.
    fn id(&self) -> AuthProviderId;

    /// The login methods this provider supports, in presentation order.
    fn methods(&self) -> &'static [LoginMethod];

    /// Runs an interactive login and returns the resulting credential.
    ///
    /// The flow reports progress through `interaction` and must abandon its
    /// work — callback listeners, polling loops — as soon as the interaction's
    /// cancel signal fires.
    ///
    /// # Errors
    ///
    /// Returns an error if the user cancels, the provider denies or times out
    /// the authorization, or the provider cannot be reached.
    async fn login(
        &self,
        method: LoginMethod,
        interaction: Arc<dyn AuthInteraction>,
    ) -> Result<OAuthCredential, Report<LoginFailure>>;

    /// Exchanges a stored credential's refresh token for a fresh credential.
    ///
    /// # Errors
    ///
    /// Returns an error if the refresh token is rejected or the provider
    /// cannot be reached.
    async fn refresh(
        &self,
        credential: &OAuthCredential,
    ) -> Result<OAuthCredential, Report<RefreshFailure>>;
}
