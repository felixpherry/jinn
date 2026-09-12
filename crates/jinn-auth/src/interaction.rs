//! The conversation between a login flow and the user interface.
//!
//! A login flow never touches the terminal. It reports progress through
//! [`AuthEvent`]s and asks for a pasted authorization code through
//! [`AuthInteraction::manual_code`]. The UI decides how to present those.
//!
//! Cancellation is cooperative: the interaction carries a [`CancelSignal`] the
//! flow polls and awaits, so stopping a login also stops its callback listener
//! and its device-code polling.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Notify;
use wherror::Error;

/// Raised when an authentication attempt is cancelled.
#[derive(Debug, Error)]
#[error("authentication cancelled")]
pub struct AuthCancelled;

/// Progress reported by a login flow while it runs.
///
/// Events carry no secrets: an authorization URL is safe to display, whereas
/// the authorization code and the resulting tokens never appear here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthEvent {
    /// Free-form progress text, e.g. "waiting for authorization".
    Progress {
        /// What the flow is currently doing.
        message: String,
    },
    /// The user should visit `url` to authorize. `instructions` explains what
    /// to expect (for example, that a browser was opened automatically).
    AuthorizationUrl {
        /// The authorization page to open.
        url: String,
        /// What the user should do next.
        instructions: String,
    },
    /// The user should visit `verification_uri` and enter `user_code`.
    DeviceCode {
        /// The page where the code is entered.
        verification_uri: String,
        /// The short code the user types.
        user_code: String,
    },
}

/// A cooperative cancellation signal shared by a login flow and its owner.
///
/// Cloning shares one underlying flag, so cancelling through any handle stops
/// every task holding a clone.
#[derive(Debug, Clone, Default)]
pub struct CancelSignal {
    inner: Arc<CancelState>,
}

#[derive(Debug, Default)]
struct CancelState {
    cancelled: AtomicBool,
    notify: Notify,
}

impl CancelSignal {
    /// Creates a signal that has not been cancelled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancels the attempt and wakes everything waiting on it.
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }

    /// Whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// Resolves once cancellation has been requested.
    ///
    /// Safe to race against other futures with `tokio::select!`.
    pub async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            let notified = self.inner.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }

    /// Returns `Err` if cancellation has already been requested.
    ///
    /// # Errors
    ///
    /// Returns [`AuthCancelled`] when the attempt was cancelled.
    pub fn check(&self) -> Result<(), AuthCancelled> {
        if self.is_cancelled() {
            return Err(AuthCancelled);
        }
        Ok(())
    }
}

/// What a login flow may ask of the user interface.
///
/// Implementations are cheap to clone and safe to share across the tasks a
/// single login spawns.
#[async_trait::async_trait]
pub trait AuthInteraction: Send + Sync + std::fmt::Debug {
    /// Reports progress. Never blocks the flow.
    fn notify(&self, event: AuthEvent);

    /// Resolves with an authorization code or callback URL the user pasted.
    ///
    /// Flows race this against their callback listener, so it may never
    /// resolve; it must resolve (or stay pending) without side effects.
    async fn manual_code(&self) -> Result<String, AuthCancelled>;

    /// The cancellation signal for this attempt.
    fn cancel_signal(&self) -> &CancelSignal;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;

    #[rstest::rstest]
    fn a_fresh_signal_is_not_cancelled() {
        // Given a new cancellation signal.
        let signal = CancelSignal::new();

        // When checking it.
        // Then it reports as live.
        assert!(!signal.is_cancelled());
    }

    #[rstest::rstest]
    fn cancelling_one_handle_cancels_its_clones() {
        // Given a signal shared with a clone.
        let signal = CancelSignal::new();
        let clone = signal.clone();

        // When cancelling through the original.
        signal.cancel();

        // Then the clone observes the cancellation.
        assert!(clone.is_cancelled());
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn awaiting_cancellation_resolves_after_cancel() {
        // Given a live signal awaited by a task.
        let signal = CancelSignal::new();
        let waiter = {
            let signal = signal.clone();
            tokio::spawn(async move { signal.cancelled().await })
        };

        // When the signal is cancelled.
        signal.cancel();

        // Then the waiting task completes.
        waiter.await.expect("waiter completes");
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn awaiting_an_already_cancelled_signal_resolves_immediately() {
        // Given a signal cancelled before anything awaits it.
        let signal = CancelSignal::new();
        signal.cancel();

        // When awaiting cancellation.
        // Then it resolves without needing another cancel call.
        signal.cancelled().await;
    }
}
