//! Shared doubles for the OpenAI Codex authentication contract tests.

use std::sync::Arc;

use base64::Engine as _;
use jinn_auth::{AuthCancelled, AuthEvent, AuthInteraction, CancelSignal};
use parking_lot::Mutex;
use tokio::sync::oneshot;

/// Records what a login flow reported and supplies a pasted code on demand.
#[derive(Debug)]
pub struct TestInteraction {
    events: Mutex<Vec<AuthEvent>>,
    manual: Mutex<Option<oneshot::Receiver<String>>>,
    cancel: CancelSignal,
}

impl TestInteraction {
    /// An interaction that never supplies a pasted code.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            events: Mutex::new(Vec::new()),
            manual: Mutex::new(None),
            cancel: CancelSignal::new(),
        })
    }

    /// An interaction whose pasted code arrives through the returned sender.
    pub fn with_manual_code() -> (Arc<Self>, oneshot::Sender<String>) {
        let (sender, receiver) = oneshot::channel();
        let interaction = Arc::new(Self {
            events: Mutex::new(Vec::new()),
            manual: Mutex::new(Some(receiver)),
            cancel: CancelSignal::new(),
        });
        (interaction, sender)
    }

    /// Every event reported so far, in order.
    pub fn events(&self) -> Vec<AuthEvent> {
        self.events.lock().clone()
    }

    /// Waits for the flow to publish an authorization URL and returns it.
    pub async fn authorization_url(&self) -> String {
        for _ in 0..200_u32 {
            let found = self.events().into_iter().find_map(|event| match event {
                AuthEvent::AuthorizationUrl { url, .. } => Some(url),
                _ => None,
            });
            if let Some(url) = found {
                return url;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("the login flow never published an authorization URL");
    }

    /// Waits for the flow to publish a device code and returns it.
    pub async fn device_code(&self) -> (String, String) {
        for _ in 0..200_u32 {
            let found = self.events().into_iter().find_map(|event| match event {
                AuthEvent::DeviceCode {
                    verification_uri,
                    user_code,
                } => Some((verification_uri, user_code)),
                _ => None,
            });
            if let Some(found) = found {
                return found;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("the login flow never published a device code");
    }
}

#[async_trait::async_trait]
impl AuthInteraction for TestInteraction {
    fn notify(&self, event: AuthEvent) {
        self.events.lock().push(event);
    }

    async fn manual_code(&self) -> Result<String, AuthCancelled> {
        let receiver = self.manual.lock().take();
        match receiver {
            Some(receiver) => receiver.await.map_err(|_dropped| AuthCancelled),
            None => std::future::pending().await,
        }
    }

    fn cancel_signal(&self) -> &CancelSignal {
        &self.cancel
    }
}

/// Builds an access token whose payload carries `account_id`.
pub fn access_token_for(account_id: &str) -> String {
    let encode = |value: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value);
    let payload = serde_json::json!({
        "https://api.openai.com/auth": { "chatgpt_account_id": account_id }
    });
    format!(
        "{}.{}.{}",
        encode(b"{\"alg\":\"none\"}"),
        encode(payload.to_string().as_bytes()),
        encode(b"signature")
    )
}

/// Reads a query parameter out of a URL.
pub fn query_param(url: &str, key: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    parsed
        .query_pairs()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.into_owned())
}
