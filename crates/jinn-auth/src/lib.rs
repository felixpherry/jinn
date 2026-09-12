//! Subscription authentication — logging in with an account you already pay for.
//!
//! Some model providers let an existing consumer subscription (a ChatGPT plan,
//! for example) authorize API-style requests. This crate is the shared
//! machinery for that: provider identity, the login methods a provider offers,
//! credential storage, automatic token refresh, and cooperative cancellation.
//!
//! The split is deliberate. Everything a second subscription provider would
//! reuse lives in the shared modules; everything specific to one provider
//! lives in its adapter (today, [`openai_codex`]). Adding a provider means
//! writing an adapter and registering it with [`AuthService`] — the user
//! interface, storage, and refresh machinery stay as they are.
//!
//! Credentials are secrets. Their [`std::fmt::Debug`] output is redacted, they
//! are stored in an owner-only file of their own, and they never travel
//! through logs, chat history, or model context.

/// Installs the process-wide rustls crypto provider (ring) in this crate's
/// test binary. reqwest is built with `rustls-no-provider` (see the workspace
/// `Cargo.toml`), so without a default provider every `reqwest::Client` panics
/// with "No provider set" at construction. Test binaries never run `main()`.
/// `install_default` errors on the second call; the result is deliberately
/// ignored.
#[cfg(test)]
#[ctor::ctor]
fn install_rustls_provider_for_tests() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub mod access_token;
pub mod auth_provider;
pub mod auth_service;
pub mod browser_launcher;
pub mod credential;
pub mod credential_presence;
pub mod credential_store;
pub mod fake;
pub mod interaction;
pub mod login_method;
pub mod oauth;
pub mod openai_codex;
pub mod provider_id;
pub mod token_provider;

pub use access_token::{AccessToken, AccessTokenError, AccessTokenProvider};
pub use auth_provider::{LoginFailure, RefreshFailure, SubscriptionAuthProvider};
pub use auth_service::{AuthOperationFailure, AuthService};
pub use browser_launcher::{
    BrowserLaunchError, BrowserLauncher, BrowserLauncherService, RecordingBrowserLauncher,
    SystemBrowserLauncher,
};
pub use credential::{OAuthCredential, StoredCredential};
pub use credential_presence::CredentialPresence;
pub use credential_store::{
    CredentialStore, CredentialStoreError, CredentialStoreService, FilesystemCredentialStore,
    InMemoryCredentialStore,
};
pub use fake::{
    FakeAuthInteraction, FakeLoginOutcome, FakeSubscriptionAuthProvider, fake_auth_service,
    fake_auth_service_logged_in,
};
pub use interaction::{AuthCancelled, AuthEvent, AuthInteraction, CancelSignal};
pub use login_method::LoginMethod;
pub use openai_codex::{CodexAuthEndpoints, OpenAiCodexAuth};
pub use provider_id::{AuthProviderId, OPENAI_CODEX_ID};
