//! Contract tests for OpenAI Codex authentication against a local mock server.
//!
//! These exercise the adapter's public behaviour — browser callback and manual
//! completion, device-code states, and token refresh — without any live
//! credential or real account authorization.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

mod support;

use std::sync::Arc;

use jinn_auth::auth_provider::{LoginFailure, RefreshFailure, SubscriptionAuthProvider};
use jinn_auth::oauth::device_code::InstantSleeper;
use jinn_auth::openai_codex::endpoints::CALLBACK_PATH;
use jinn_auth::{
    AuthInteraction, BrowserLauncherService, CodexAuthEndpoints, LoginMethod, OAuthCredential,
    OpenAiCodexAuth, RecordingBrowserLauncher,
};
use support::{TestInteraction, access_token_for, query_param};

/// Installs the process-wide rustls crypto provider for this test binary.
#[ctor::ctor]
fn install_rustls_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Builds an adapter pointed at `server`, with a browser callback on `port`.
fn adapter(server: &mockito::ServerGuard, port: u16) -> OpenAiCodexAuth {
    let endpoints = CodexAuthEndpoints {
        auth_base_url: server.url(),
        redirect_uri: format!("http://localhost:{port}{CALLBACK_PATH}"),
        callback_bind_addr: format!("127.0.0.1:{port}"),
    };
    OpenAiCodexAuth::with_endpoints(
        BrowserLauncherService::new(Arc::new(RecordingBrowserLauncher::new())),
        endpoints,
    )
    .with_sleeper(Arc::new(InstantSleeper::new()))
}

/// Builds an adapter whose callback port is already taken, forcing the manual
/// path, and reports whether the browser launch succeeded.
fn adapter_without_callback(
    server: &mockito::ServerGuard,
    launcher: Arc<RecordingBrowserLauncher>,
) -> OpenAiCodexAuth {
    let endpoints = CodexAuthEndpoints {
        auth_base_url: server.url(),
        redirect_uri: "http://localhost:1/auth/callback".to_owned(),
        // Port 0 with an unbindable host: the listener never comes up.
        callback_bind_addr: "127.0.0.1:1".to_owned(),
    };
    OpenAiCodexAuth::with_endpoints(BrowserLauncherService::new(launcher), endpoints)
        .with_sleeper(Arc::new(InstantSleeper::new()))
}

/// A successful token response body.
fn token_body(access_token: &str) -> String {
    serde_json::json!({
        "access_token": access_token,
        "refresh_token": "refresh-token",
        "expires_in": 3600,
    })
    .to_string()
}

/// Performs the browser redirect the provider would perform.
async fn deliver_callback(port: u16, code: &str, state: &str) {
    let url = format!("http://127.0.0.1:{port}{CALLBACK_PATH}?code={code}&state={state}");
    let _ = reqwest::Client::new().get(url).send().await;
}

fn credential_of(
    result: Result<OAuthCredential, error_stack::Report<LoginFailure>>,
) -> OAuthCredential {
    result.expect("login completes")
}

// ---------------------------------------------------------------------------
// Browser login
// ---------------------------------------------------------------------------

#[rstest::rstest]
#[tokio::test]
async fn browser_login_completes_through_the_local_callback() {
    // Given a provider that exchanges an authorization code for tokens.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/oauth/token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(token_body(&access_token_for("acct-callback")))
        .create_async()
        .await;
    let auth = adapter(&server, 14571);
    let interaction = TestInteraction::new();

    // When the browser redirects back to the local callback.
    let login = {
        let interaction = interaction.clone();
        tokio::spawn(async move { auth.login(LoginMethod::Browser, interaction).await })
    };
    let url = interaction.authorization_url().await;
    let state = query_param(&url, "state").expect("state parameter");
    deliver_callback(14571, "the-code", &state).await;

    // Then the login yields a credential for the authorized account.
    let credential = credential_of(login.await.expect("task"));
    assert_eq!(credential.account_id, "acct-callback");
}

#[rstest::rstest]
#[tokio::test]
async fn browser_login_publishes_the_authorization_url() {
    // Given a browser login in progress.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/oauth/token")
        .with_status(200)
        .with_body(token_body(&access_token_for("acct-1")))
        .create_async()
        .await;
    let auth = adapter(&server, 14572);
    let interaction = TestInteraction::new();
    let login = {
        let interaction = interaction.clone();
        tokio::spawn(async move { auth.login(LoginMethod::Browser, interaction).await })
    };

    // When the flow reports where the user should authorize.
    let url = interaction.authorization_url().await;

    // Then the URL carries the PKCE challenge method the provider requires.
    assert_eq!(
        query_param(&url, "code_challenge_method").as_deref(),
        Some("S256")
    );
    interaction.cancel_signal().cancel();
    let _ = login.await;
}

#[rstest::rstest]
#[tokio::test]
async fn browser_login_completes_from_a_pasted_redirect_url() {
    // Given a machine where the local callback cannot be bound.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/oauth/token")
        .with_status(200)
        .with_body(token_body(&access_token_for("acct-pasted")))
        .create_async()
        .await;
    let launcher = Arc::new(RecordingBrowserLauncher::new());
    let auth = adapter_without_callback(&server, launcher);
    let (interaction, paste) = TestInteraction::with_manual_code();

    // When the user pastes the redirect URL into the modal.
    let login = {
        let interaction = interaction.clone();
        tokio::spawn(async move { auth.login(LoginMethod::Browser, interaction).await })
    };
    let url = interaction.authorization_url().await;
    let state = query_param(&url, "state").expect("state parameter");
    paste
        .send(format!(
            "http://localhost:1455/auth/callback?code=pasted-code&state={state}"
        ))
        .expect("paste delivered");

    // Then the login yields a credential for the authorized account.
    let credential = credential_of(login.await.expect("task"));
    assert_eq!(credential.account_id, "acct-pasted");
}

#[rstest::rstest]
#[tokio::test]
async fn a_failed_browser_launch_still_offers_a_usable_login() {
    // Given a machine with no browser to launch.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/oauth/token")
        .with_status(200)
        .with_body(token_body(&access_token_for("acct-no-browser")))
        .create_async()
        .await;
    let auth = adapter_without_callback(&server, Arc::new(RecordingBrowserLauncher::failing()));
    let (interaction, paste) = TestInteraction::with_manual_code();

    // When the user pastes the authorization code instead.
    let login = {
        let interaction = interaction.clone();
        tokio::spawn(async move { auth.login(LoginMethod::Browser, interaction).await })
    };
    interaction.authorization_url().await;
    paste
        .send("pasted-code".to_owned())
        .expect("paste delivered");

    // Then the login still completes.
    let credential = credential_of(login.await.expect("task"));
    assert_eq!(credential.account_id, "acct-no-browser");
}

#[rstest::rstest]
#[tokio::test]
async fn a_pasted_result_from_another_attempt_is_rejected() {
    // Given a browser login in progress.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/oauth/token")
        .with_status(200)
        .with_body(token_body(&access_token_for("acct-1")))
        .create_async()
        .await;
    let auth = adapter_without_callback(&server, Arc::new(RecordingBrowserLauncher::new()));
    let (interaction, paste) = TestInteraction::with_manual_code();
    let login = {
        let interaction = interaction.clone();
        tokio::spawn(async move { auth.login(LoginMethod::Browser, interaction).await })
    };
    interaction.authorization_url().await;

    // When the pasted result carries a state from a different attempt.
    paste
        .send("http://localhost:1455/auth/callback?code=abc&state=someone-else".to_owned())
        .expect("paste delivered");

    // Then the login fails rather than accepting the foreign result.
    let result = login.await.expect("task");
    assert!(result.is_err(), "a mismatched state must not authorize");
}

#[rstest::rstest]
#[tokio::test]
async fn cancelling_a_browser_login_reports_cancellation() {
    // Given a browser login waiting for authorization.
    let server = mockito::Server::new_async().await;
    let auth = adapter(&server, 14573);
    let interaction = TestInteraction::new();
    let login = {
        let interaction = interaction.clone();
        tokio::spawn(async move { auth.login(LoginMethod::Browser, interaction).await })
    };
    interaction.authorization_url().await;

    // When the user cancels.
    interaction.cancel_signal().cancel();

    // Then the attempt reports cancellation.
    let error = login.await.expect("task").expect_err("cancelled");
    assert!(matches!(
        error.downcast_ref::<LoginFailure>(),
        Some(LoginFailure::Cancelled)
    ));
}

// ---------------------------------------------------------------------------
// Device-code login
// ---------------------------------------------------------------------------

#[rstest::rstest]
#[tokio::test]
async fn device_code_login_completes_after_the_user_authorizes() {
    // Given a device authorization that becomes authorized on the second poll.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/api/accounts/deviceauth/usercode")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "device_auth_id": "device-1",
                "user_code": "ABCD-1234",
                "interval": 1,
            })
            .to_string(),
        )
        .create_async()
        .await;
    server
        .mock("POST", "/api/accounts/deviceauth/token")
        .with_status(403)
        .create_async()
        .await;
    server
        .mock("POST", "/api/accounts/deviceauth/token")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "authorization_code": "device-code",
                "code_verifier": "device-verifier",
            })
            .to_string(),
        )
        .create_async()
        .await;
    server
        .mock("POST", "/oauth/token")
        .with_status(200)
        .with_body(token_body(&access_token_for("acct-device")))
        .create_async()
        .await;
    let auth = adapter(&server, 14574);

    // When the device-code login runs.
    let credential = credential_of(
        auth.login(LoginMethod::DeviceCode, TestInteraction::new())
            .await,
    );

    // Then it yields a credential for the authorized account.
    assert_eq!(credential.account_id, "acct-device");
}

#[rstest::rstest]
#[tokio::test]
async fn device_code_login_publishes_the_verification_code() {
    // Given a device authorization that never completes.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/api/accounts/deviceauth/usercode")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "device_auth_id": "device-1",
                "user_code": "ABCD-1234",
                "interval": 1,
            })
            .to_string(),
        )
        .create_async()
        .await;
    server
        .mock("POST", "/api/accounts/deviceauth/token")
        .with_status(403)
        .expect_at_least(1)
        .create_async()
        .await;
    let auth = adapter(&server, 14575);
    let interaction = TestInteraction::new();

    // When the flow starts and reports the code.
    let login = {
        let interaction = interaction.clone();
        tokio::spawn(async move { auth.login(LoginMethod::DeviceCode, interaction).await })
    };
    let (verification_uri, user_code) = interaction.device_code().await;

    // Then the user sees where to go and what to type.
    assert_eq!(user_code, "ABCD-1234");
    assert!(
        verification_uri.ends_with("/codex/device"),
        "verification URI must point at the device page: {verification_uri}"
    );
    interaction.cancel_signal().cancel();
    let _ = login.await;
}

#[rstest::rstest]
#[tokio::test]
async fn a_denied_device_authorization_reports_denial() {
    // Given a device authorization the user declines.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/api/accounts/deviceauth/usercode")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "device_auth_id": "device-1",
                "user_code": "ABCD-1234",
                "interval": 1,
            })
            .to_string(),
        )
        .create_async()
        .await;
    server
        .mock("POST", "/api/accounts/deviceauth/token")
        .with_status(400)
        .with_body(serde_json::json!({ "error": "access_denied" }).to_string())
        .create_async()
        .await;
    let auth = adapter(&server, 14576);

    // When the device-code login runs.
    let error = auth
        .login(LoginMethod::DeviceCode, TestInteraction::new())
        .await
        .expect_err("denied");

    // Then the denial reaches the caller.
    assert!(matches!(
        error.downcast_ref::<LoginFailure>(),
        Some(LoginFailure::Denied)
    ));
}

#[rstest::rstest]
#[tokio::test]
async fn an_unavailable_device_endpoint_reports_a_provider_error() {
    // Given a deployment with device-code login disabled.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/api/accounts/deviceauth/usercode")
        .with_status(404)
        .create_async()
        .await;
    let auth = adapter(&server, 14577);

    // When the device-code login runs.
    let error = auth
        .login(LoginMethod::DeviceCode, TestInteraction::new())
        .await
        .expect_err("unavailable");

    // Then the failure is reported as a provider problem.
    assert!(matches!(
        error.downcast_ref::<LoginFailure>(),
        Some(LoginFailure::Provider)
    ));
}

// ---------------------------------------------------------------------------
// Refresh
// ---------------------------------------------------------------------------

fn stored_credential() -> OAuthCredential {
    OAuthCredential {
        access_token: access_token_for("acct-1"),
        refresh_token: "old-refresh".to_owned(),
        expires_at_ms: 0,
        account_id: "acct-1".to_owned(),
    }
}

#[rstest::rstest]
#[tokio::test]
async fn refresh_returns_the_rotated_credential() {
    // Given a provider that rotates both tokens on refresh.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/oauth/token")
        .match_body(mockito::Matcher::Regex(
            "grant_type=refresh_token".to_owned(),
        ))
        .with_status(200)
        .with_body(
            serde_json::json!({
                "access_token": access_token_for("acct-1"),
                "refresh_token": "new-refresh",
                "expires_in": 3600,
            })
            .to_string(),
        )
        .create_async()
        .await;
    let auth = adapter(&server, 14578);

    // When refreshing a stored credential.
    let refreshed = auth.refresh(&stored_credential()).await.expect("refresh");

    // Then the rotated refresh token comes back for persistence.
    assert_eq!(refreshed.refresh_token, "new-refresh");
}

#[rstest::rstest]
#[tokio::test]
async fn a_rejected_refresh_token_reports_rejection() {
    // Given a provider that no longer accepts the stored refresh token.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/oauth/token")
        .with_status(400)
        .with_body(serde_json::json!({ "error": "invalid_grant" }).to_string())
        .create_async()
        .await;
    let auth = adapter(&server, 14579);

    // When refreshing.
    let error = auth
        .refresh(&stored_credential())
        .await
        .expect_err("rejected");

    // Then the caller learns the credential must be replaced by a new login.
    assert!(matches!(
        error.downcast_ref::<RefreshFailure>(),
        Some(RefreshFailure::Rejected)
    ));
}

#[rstest::rstest]
#[tokio::test]
async fn a_server_error_during_refresh_is_not_treated_as_a_logout() {
    // Given an authorization service that is temporarily failing.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/oauth/token")
        .with_status(500)
        .create_async()
        .await;
    let auth = adapter(&server, 14580);

    // When refreshing.
    let error = auth
        .refresh(&stored_credential())
        .await
        .expect_err("server error");

    // Then it is reported as a provider problem, not a rejected credential.
    assert!(matches!(
        error.downcast_ref::<RefreshFailure>(),
        Some(RefreshFailure::Provider)
    ));
}

#[rstest::rstest]
#[tokio::test]
async fn a_token_response_without_an_account_claim_is_rejected() {
    // Given a token response whose access token carries no account id.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/oauth/token")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "access_token": "opaque-token",
                "refresh_token": "refresh",
                "expires_in": 3600,
            })
            .to_string(),
        )
        .create_async()
        .await;
    let auth = adapter(&server, 14581);

    // When refreshing.
    let error = auth
        .refresh(&stored_credential())
        .await
        .expect_err("unusable response");

    // Then the unusable response is reported rather than stored.
    assert!(matches!(
        error.downcast_ref::<RefreshFailure>(),
        Some(RefreshFailure::Provider)
    ));
}
