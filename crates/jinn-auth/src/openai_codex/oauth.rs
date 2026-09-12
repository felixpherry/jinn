//! The OpenAI Codex login and refresh adapter.
//!
//! Implements both login methods against the ChatGPT authorization service:
//! a browser flow using OAuth with PKCE (with a local callback and a pasted
//! fallback), and a device-code flow for headless machines. Everything else —
//! storage, refresh scheduling, the picker — is shared infrastructure.

use std::sync::Arc;
use std::time::Duration;

use error_stack::{Report, ResultExt as _};

use crate::auth_provider::{LoginFailure, RefreshFailure, SubscriptionAuthProvider};
use crate::browser_launcher::BrowserLauncherService;
use crate::credential::OAuthCredential;
use crate::interaction::{AuthEvent, AuthInteraction};
use crate::login_method::LoginMethod;
use crate::oauth::callback_server::{CallbackError, CallbackListener};
use crate::oauth::device_code::{
    DeviceCodeFailure, DeviceCodePoll, DeviceCodeSchedule, PollSleeper, TokioSleeper,
    poll_device_code,
};
use crate::oauth::pkce;
use crate::openai_codex::account::account_id_from_access_token;
use crate::openai_codex::authorization_input;
use crate::openai_codex::endpoints::{
    CALLBACK_PATH, CLIENT_ID, CodexAuthEndpoints, ORIGINATOR, SCOPE,
};
use crate::provider_id::AuthProviderId;

/// How long a device-code authorization stays valid.
const DEVICE_CODE_LIFETIME: Duration = Duration::from_mins(15);

/// Login methods this provider supports, in presentation order.
const METHODS: &[LoginMethod] = &[LoginMethod::Browser, LoginMethod::DeviceCode];

/// OpenAI Codex subscription authentication.
#[derive(derive_more::Debug)]
pub struct OpenAiCodexAuth {
    #[debug(skip)]
    client: Option<reqwest::Client>,
    endpoints: CodexAuthEndpoints,
    browser: BrowserLauncherService,
    #[debug(skip)]
    sleeper: Arc<dyn PollSleeper>,
}

impl OpenAiCodexAuth {
    /// Creates the adapter against the production authorization service.
    #[must_use]
    pub fn new(browser: BrowserLauncherService) -> Self {
        Self::with_endpoints(browser, CodexAuthEndpoints::default())
    }

    /// Creates the adapter against a specific authorization deployment.
    #[must_use]
    pub fn with_endpoints(browser: BrowserLauncherService, endpoints: CodexAuthEndpoints) -> Self {
        Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(30))
                .build()
                .ok(),
            endpoints,
            browser,
            sleeper: Arc::new(TokioSleeper),
        }
    }

    /// Replaces the polling sleeper, so tests can run the device-code loop
    /// without real delays.
    #[must_use]
    pub fn with_sleeper(mut self, sleeper: Arc<dyn PollSleeper>) -> Self {
        self.sleeper = sleeper;
        self
    }

    fn client(&self) -> Result<&reqwest::Client, Report<LoginFailure>> {
        self.client.as_ref().ok_or_else(|| {
            Report::new(LoginFailure::Provider).attach("could not initialize secure HTTP client")
        })
    }

    /// Builds the authorization URL for a browser login.
    fn authorization_url(&self, challenge: &str, state: &str) -> String {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("response_type", "code")
            .append_pair("client_id", CLIENT_ID)
            .append_pair("redirect_uri", &self.endpoints.redirect_uri)
            .append_pair("scope", SCOPE)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", state)
            .append_pair("id_token_add_organizations", "true")
            .append_pair("codex_cli_simplified_flow", "true")
            .append_pair("originator", ORIGINATOR)
            .finish();
        format!("{}?{query}", self.endpoints.authorize_url())
    }

    /// Runs the browser login: authorize in a browser, complete via the local
    /// callback or a pasted code.
    async fn login_browser(
        &self,
        interaction: Arc<dyn AuthInteraction>,
    ) -> Result<OAuthCredential, Report<LoginFailure>> {
        let pkce = pkce::generate();
        let state = pkce::random_state();
        let url = self.authorization_url(&pkce.challenge, &state);

        // Bind before opening the browser so a fast authorization cannot beat
        // the listener into existence. A failure here is survivable: the user
        // can still paste the result.
        let listener = CallbackListener::bind(&self.endpoints.callback_bind_addr)
            .await
            .ok();

        let opened = self.browser.open(&url).is_ok();
        interaction.notify(AuthEvent::AuthorizationUrl {
            url,
            instructions: browser_instructions(opened, listener.is_some()),
        });

        let cancel = interaction.cancel_signal().clone();
        let code = {
            let manual = interaction.manual_code();
            match listener {
                Some(listener) => {
                    let callback = listener.accept_code(CALLBACK_PATH, &state, &cancel);
                    tokio::select! {
                        received = callback => received.map_err(map_callback_error)?,
                        pasted = manual => verified_code(&pasted.change_context(LoginFailure::Cancelled)?, &state)?,
                        () = cancel.cancelled() => return Err(Report::new(LoginFailure::Cancelled)),
                    }
                }
                None => {
                    tokio::select! {
                        pasted = manual => verified_code(&pasted.change_context(LoginFailure::Cancelled)?, &state)?,
                        () = cancel.cancelled() => return Err(Report::new(LoginFailure::Cancelled)),
                    }
                }
            }
        };

        interaction.notify(AuthEvent::Progress {
            message: "Exchanging authorization code…".to_owned(),
        });
        self.exchange_code(&code, &pkce.verifier, &self.endpoints.redirect_uri)
            .await
    }

    /// Runs the device-code login: show a code, poll until it is authorized.
    async fn login_device_code(
        &self,
        interaction: Arc<dyn AuthInteraction>,
    ) -> Result<OAuthCredential, Report<LoginFailure>> {
        let device = self.start_device_authorization().await?;

        interaction.notify(AuthEvent::DeviceCode {
            verification_uri: self.endpoints.device_verification_uri(),
            user_code: device.user_code.clone(),
        });

        let schedule = DeviceCodeSchedule {
            interval: Duration::from_secs(device.interval_seconds),
            expires_in: DEVICE_CODE_LIFETIME,
        };
        let cancel = interaction.cancel_signal().clone();
        let authorized = poll_device_code(schedule, &cancel, self.sleeper.as_ref(), || {
            self.poll_device_authorization(&device)
        })
        .await
        .map_err(login_failure_from_device_failure)?;

        interaction.notify(AuthEvent::Progress {
            message: "Exchanging authorization code…".to_owned(),
        });
        self.exchange_code(
            &authorized.authorization_code,
            &authorized.code_verifier,
            &self.endpoints.device_redirect_uri(),
        )
        .await
    }

    /// Asks the provider for a device user code.
    async fn start_device_authorization(
        &self,
    ) -> Result<DeviceAuthorization, Report<LoginFailure>> {
        let response = self
            .client()?
            .post(self.endpoints.device_user_code_url())
            .json(&serde_json::json!({ "client_id": CLIENT_ID }))
            .send()
            .await
            .change_context(LoginFailure::Provider)
            .attach("could not reach the device authorization endpoint")?;

        let status = response.status();
        if !status.is_success() {
            return Err(Report::new(LoginFailure::Provider).attach(format!(
                "device code request failed with HTTP {}",
                status.as_u16()
            )));
        }

        let body: DeviceAuthorizationResponse = response
            .json()
            .await
            .change_context(LoginFailure::Provider)
            .attach("device authorization response was unreadable")?;

        let (Some(device_auth_id), Some(user_code)) = (body.device_auth_id, body.user_code) else {
            return Err(Report::new(LoginFailure::Provider)
                .attach("device authorization response was missing required fields"));
        };

        let interval_seconds = body
            .interval
            .and_then(|interval| interval.as_seconds())
            .filter(|_| !device_auth_id.is_empty() && !user_code.is_empty())
            .ok_or_else(|| {
                Report::new(LoginFailure::Provider).attach("invalid device authorization response")
            })?;
        Ok(DeviceAuthorization {
            device_auth_id,
            user_code,
            interval_seconds,
        })
    }

    /// Polls once for the outcome of a device authorization.
    async fn poll_device_authorization(
        &self,
        device: &DeviceAuthorization,
    ) -> DeviceCodePoll<AuthorizedDeviceCode> {
        let Ok(client) = self.client() else {
            return DeviceCodePoll::Failed(DeviceCodeFailure::Provider(
                "could not initialize secure HTTP client".to_owned(),
            ));
        };
        let response = client
            .post(self.endpoints.device_token_url())
            .json(&serde_json::json!({
                "device_auth_id": device.device_auth_id,
                "user_code": device.user_code,
            }))
            .send()
            .await;

        let response = match response {
            Ok(response) => response,
            Err(err) => {
                return DeviceCodePoll::Failed(DeviceCodeFailure::Provider(format!(
                    "could not reach the device token endpoint: {err}"
                )));
            }
        };

        let status = response.status();
        if status.is_success() {
            let body: Option<DeviceTokenResponse> = response.json().await.ok();
            return match body.and_then(DeviceTokenResponse::into_authorized) {
                Some(authorized) => DeviceCodePoll::Complete(authorized),
                None => DeviceCodePoll::Failed(DeviceCodeFailure::Provider(
                    "device token response was missing required fields".to_owned(),
                )),
            };
        }

        // The provider reports "not authorized yet" as a plain 403/404 on this
        // endpoint rather than as a structured pending error.
        if status.as_u16() == 403 || status.as_u16() == 404 {
            return DeviceCodePoll::Pending;
        }

        let body = response.text().await.unwrap_or_default();
        match device_error_code(&body).as_deref() {
            Some("deviceauth_authorization_pending" | "authorization_pending") => {
                DeviceCodePoll::Pending
            }
            Some("slow_down") => DeviceCodePoll::SlowDown {
                interval_seconds: None,
            },
            Some("access_denied") => DeviceCodePoll::Failed(DeviceCodeFailure::Denied),
            Some("expired_token") => DeviceCodePoll::Failed(DeviceCodeFailure::Expired),
            _ => DeviceCodePoll::Failed(DeviceCodeFailure::Provider(format!(
                "device authorization failed with HTTP {}",
                status.as_u16()
            ))),
        }
    }

    /// Exchanges an authorization code for a credential.
    async fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<OAuthCredential, Report<LoginFailure>> {
        let response = self
            .client()?
            .post(self.endpoints.token_url())
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", CLIENT_ID),
                ("code", code),
                ("code_verifier", verifier),
                ("redirect_uri", redirect_uri),
            ])
            .send()
            .await
            .change_context(LoginFailure::Provider)
            .attach("could not reach the token endpoint")?;

        let status = response.status();
        if !status.is_success() {
            return Err(Report::new(LoginFailure::Provider).attach(format!(
                "token exchange failed with HTTP {}",
                status.as_u16()
            )));
        }

        let body: TokenResponse = response
            .json()
            .await
            .change_context(LoginFailure::Provider)
            .attach("token response was unreadable")?;

        body.into_credential()
            .ok_or_else(|| Report::new(LoginFailure::Provider).attach(TOKEN_FIELDS_MISSING))
    }
}

/// Message used whenever a token response cannot be turned into a credential.
const TOKEN_FIELDS_MISSING: &str = "token response was missing required fields";

/// What to tell the user once the authorization page is ready.
fn browser_instructions(browser_opened: bool, callback_listening: bool) -> String {
    let opening = if browser_opened {
        "A browser window should have opened."
    } else {
        "Open this URL in a browser to continue."
    };
    let completion = if callback_listening {
        "jinn will detect the result automatically, or you can paste the authorization code or redirect URL below."
    } else {
        "Paste the authorization code or redirect URL below when you are done."
    };
    format!("{opening} {completion}")
}

/// Validates pasted input against the expected `state` and returns its code.
fn verified_code(pasted: &str, expected_state: &str) -> Result<String, Report<LoginFailure>> {
    let parsed = authorization_input::parse(pasted);
    if let Some(state) = parsed.state.as_deref()
        && state != expected_state
    {
        return Err(Report::new(LoginFailure::Provider)
            .attach("the pasted authorization result did not match this login attempt"));
    }
    parsed.code.ok_or_else(|| {
        Report::new(LoginFailure::Provider).attach("no authorization code was found")
    })
}

/// Maps a callback-listener failure onto a login failure.
fn map_callback_error(failure: Report<CallbackError>) -> Report<LoginFailure> {
    let target = match failure.downcast_ref::<CallbackError>() {
        Some(CallbackError::Cancelled) => LoginFailure::Cancelled,
        _ => LoginFailure::Provider,
    };
    failure.change_context(target)
}

/// Maps a device-code loop failure onto a login failure.
fn login_failure_from_device_failure(failure: DeviceCodeFailure) -> Report<LoginFailure> {
    match failure {
        DeviceCodeFailure::Denied => Report::new(LoginFailure::Denied),
        DeviceCodeFailure::Expired => Report::new(LoginFailure::TimedOut),
        DeviceCodeFailure::Cancelled => Report::new(LoginFailure::Cancelled),
        DeviceCodeFailure::Provider(message) => Report::new(LoginFailure::Provider).attach(message),
    }
}

/// Pulls an error code out of a device-authorization error body, tolerating
/// both the string and object shapes the provider uses.
fn device_error_code(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let error = value.get("error")?;
    error
        .as_str()
        .map(str::to_owned)
        .or_else(|| error.get("code")?.as_str().map(str::to_owned))
}

/// A device authorization awaiting the user.
#[derive(Debug, Clone)]
struct DeviceAuthorization {
    device_auth_id: String,
    user_code: String,
    interval_seconds: u64,
}

/// A completed device authorization.
#[derive(Debug, Clone)]
struct AuthorizedDeviceCode {
    authorization_code: String,
    code_verifier: String,
}

#[derive(Debug, serde::Deserialize)]
struct DeviceAuthorizationResponse {
    device_auth_id: Option<String>,
    user_code: Option<String>,
    interval: Option<IntervalField>,
}

/// The provider reports the poll interval as either a number or a string.
#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum IntervalField {
    Seconds(u64),
    Text(String),
}

impl IntervalField {
    fn as_seconds(&self) -> Option<u64> {
        match self {
            Self::Seconds(seconds) => Some(*seconds),
            Self::Text(text) => text.trim().parse().ok(),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct DeviceTokenResponse {
    authorization_code: Option<String>,
    code_verifier: Option<String>,
}

impl DeviceTokenResponse {
    fn into_authorized(self) -> Option<AuthorizedDeviceCode> {
        Some(AuthorizedDeviceCode {
            authorization_code: self.authorization_code.filter(|code| !code.is_empty())?,
            code_verifier: self.code_verifier.filter(|code| !code.is_empty())?,
        })
    }
}

#[derive(Debug, serde::Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

impl TokenResponse {
    fn into_credential(self) -> Option<OAuthCredential> {
        let access_token = self.access_token?;
        let refresh_token = self.refresh_token.filter(|token| !token.is_empty())?;
        let expires_in = self.expires_in.filter(|seconds| *seconds > 0)?;
        let account_id = account_id_from_access_token(&access_token)?;
        Some(OAuthCredential {
            expires_at_ms: jiff::Timestamp::now()
                .as_millisecond()
                .saturating_add(expires_in.saturating_mul(1000)),
            refresh_token,
            access_token,
            account_id,
        })
    }
}

#[async_trait::async_trait]
impl SubscriptionAuthProvider for OpenAiCodexAuth {
    fn id(&self) -> AuthProviderId {
        AuthProviderId::OpenAiCodex
    }

    fn methods(&self) -> &'static [LoginMethod] {
        METHODS
    }

    async fn login(
        &self,
        method: LoginMethod,
        interaction: Arc<dyn AuthInteraction>,
    ) -> Result<OAuthCredential, Report<LoginFailure>> {
        interaction
            .cancel_signal()
            .check()
            .map_err(|cancelled| Report::new(cancelled).change_context(LoginFailure::Cancelled))?;
        let cancel = interaction.cancel_signal().clone();
        tokio::select! {
            biased;
            () = cancel.cancelled() => Err(Report::new(LoginFailure::Cancelled)),
            result = async {
                match method {
                    LoginMethod::Browser => self.login_browser(interaction).await,
                    LoginMethod::DeviceCode => self.login_device_code(interaction).await,
                }
            } => result,
        }
    }

    async fn refresh(
        &self,
        credential: &OAuthCredential,
    ) -> Result<OAuthCredential, Report<RefreshFailure>> {
        let response = self
            .client()
            .change_context(RefreshFailure::Provider)?
            .post(self.endpoints.token_url())
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", CLIENT_ID),
                ("refresh_token", credential.refresh_token.as_str()),
            ])
            .send()
            .await
            .change_context(RefreshFailure::Provider)
            .attach("could not reach the token endpoint")?;

        let status = response.status();
        if status.as_u16() == 400 || status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(Report::new(RefreshFailure::Rejected)
                .attach("the provider rejected the stored refresh token"));
        }
        if !status.is_success() {
            return Err(Report::new(RefreshFailure::Provider).attach(format!(
                "token refresh failed with HTTP {}",
                status.as_u16()
            )));
        }

        let body: TokenResponse = response
            .json()
            .await
            .change_context(RefreshFailure::Provider)
            .attach("token response was unreadable")?;

        body.into_credential()
            .ok_or_else(|| Report::new(RefreshFailure::Provider).attach(TOKEN_FIELDS_MISSING))
    }
}
