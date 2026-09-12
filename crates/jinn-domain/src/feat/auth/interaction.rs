//! Connecting a running login flow to the modal.
//!
//! A login flow runs asynchronously and reports progress through the
//! [`AuthInteraction`] trait. This implementation turns those reports into bus
//! events, so the auth actor stays the only writer of authentication state,
//! and resolves the "paste an authorization code" request when the user
//! submits the modal input.

use std::sync::Arc;

use jinn_auth::{AuthCancelled, AuthEvent, AuthInteraction, CancelSignal};
use parking_lot::Mutex;
use tokio::sync::oneshot;

use crate::common::bridge::Bridge;
use crate::feat::auth::protocol::event::AuthProgressReported;
use crate::feat::auth::state::AuthPhase;

/// Reports a login flow's progress onto the bus.
///
/// Each instance belongs to one attempt. `attempt` travels with every report
/// so the actor can discard reports from an attempt the user has since
/// cancelled or replaced.
#[derive(derive_more::Debug)]
pub struct BusAuthInteraction {
    attempt: u64,
    #[debug(skip)]
    bridge: Bridge,
    cancel: CancelSignal,
    #[debug(skip)]
    pasted_code: Mutex<Option<oneshot::Receiver<String>>>,
}

impl BusAuthInteraction {
    /// Creates the interaction for one attempt.
    ///
    /// Returns the interaction and the sender that delivers a pasted
    /// authorization code to it.
    #[must_use]
    pub fn new(
        attempt: u64,
        bridge: Bridge,
        cancel: CancelSignal,
    ) -> (Arc<Self>, oneshot::Sender<String>) {
        let (sender, receiver) = oneshot::channel();
        let interaction = Arc::new(Self {
            attempt,
            bridge,
            cancel,
            pasted_code: Mutex::new(Some(receiver)),
        });
        (interaction, sender)
    }
}

/// Translates a provider's progress report into what the modal should show.
#[must_use]
pub fn phase_for(event: AuthEvent) -> AuthPhase {
    match event {
        AuthEvent::Progress { message } => AuthPhase::Working { message },
        AuthEvent::AuthorizationUrl { url, instructions } => {
            AuthPhase::Authorizing { url, instructions }
        }
        AuthEvent::DeviceCode {
            verification_uri,
            user_code,
        } => AuthPhase::DeviceCode {
            verification_uri,
            user_code,
        },
    }
}

#[async_trait::async_trait]
impl AuthInteraction for BusAuthInteraction {
    fn notify(&self, event: AuthEvent) {
        let message = AuthProgressReported {
            attempt: self.attempt,
            phase: phase_for(event),
        };
        if self.bridge.send(Bridge::publish_closure(message)).is_err() {
            tracing::debug!("authentication progress dropped: the bus bridge is closed");
        }
    }

    async fn manual_code(&self) -> Result<String, AuthCancelled> {
        let receiver = self.pasted_code.lock().take();
        match receiver {
            Some(receiver) => receiver.await.map_err(|_dropped| AuthCancelled),
            // A second request would have no sender behind it; leave it
            // pending so it never resolves rather than reporting a spurious
            // cancellation.
            None => std::future::pending().await,
        }
    }

    fn cancel_signal(&self) -> &CancelSignal {
        &self.cancel
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;

    #[rstest::rstest]
    fn an_authorization_url_becomes_an_authorizing_phase() {
        // Given a provider reporting where to authorize.
        let event = AuthEvent::AuthorizationUrl {
            url: "https://auth.example/authorize".to_owned(),
            instructions: "open this".to_owned(),
        };

        // When translating it for the modal.
        let phase = phase_for(event);

        // Then the modal is told to show the URL.
        assert_eq!(
            phase,
            AuthPhase::Authorizing {
                url: "https://auth.example/authorize".to_owned(),
                instructions: "open this".to_owned(),
            }
        );
    }

    #[rstest::rstest]
    fn a_device_code_becomes_a_device_code_phase() {
        // Given a provider reporting a device code.
        let event = AuthEvent::DeviceCode {
            verification_uri: "https://auth.example/device".to_owned(),
            user_code: "ABCD-1234".to_owned(),
        };

        // When translating it for the modal.
        let phase = phase_for(event);

        // Then the modal is told to show where to go and what to type.
        assert_eq!(
            phase,
            AuthPhase::DeviceCode {
                verification_uri: "https://auth.example/device".to_owned(),
                user_code: "ABCD-1234".to_owned(),
            }
        );
    }

    #[rstest::rstest]
    fn progress_text_becomes_a_working_phase() {
        // Given a provider reporting what it is doing.
        let event = AuthEvent::Progress {
            message: "Exchanging authorization code…".to_owned(),
        };

        // When translating it for the modal.
        let phase = phase_for(event);

        // Then the modal shows the progress note.
        assert_eq!(
            phase,
            AuthPhase::Working {
                message: "Exchanging authorization code…".to_owned(),
            }
        );
    }
}
