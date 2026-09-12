//! What the authentication modal is currently showing.
//!
//! One attempt is active at a time. The state records which provider and
//! method it belongs to and what the user should see right now: an
//! authorization URL, a device code, a progress note, or an error.
//!
//! Nothing secret is kept here. Authorization codes and tokens never reach
//! application state, so they cannot leak into rendering, logs, or history.

use jinn_auth::{AuthProviderId, LoginMethod};
use serde::{Deserialize, Serialize};

/// What the active attempt is waiting on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthPhase {
    /// The attempt has begun but the provider has not reported anything yet.
    Starting,
    /// The user should authorize at `url`.
    Authorizing {
        /// The authorization page.
        url: String,
        /// What the user should do next.
        instructions: String,
    },
    /// The user should enter `user_code` at `verification_uri`.
    DeviceCode {
        /// Where the code is entered.
        verification_uri: String,
        /// The code the user types.
        user_code: String,
    },
    /// The attempt is working; `message` says on what.
    Working {
        /// What the attempt is currently doing.
        message: String,
    },
    /// The attempt failed; `message` explains why.
    Failed {
        /// Why the attempt failed.
        message: String,
    },
}

/// An authentication attempt in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthFlow {
    /// Which provider is being authenticated.
    pub provider: AuthProviderId,
    /// Which login method the user chose.
    pub method: LoginMethod,
    /// What the modal should show.
    pub phase: AuthPhase,
}

/// Authentication state — owned by the auth actor.
///
/// Written by `AuthActor` (authoritative progress and outcome) and by the
/// `IntentHandler` (which starts and cancels attempts from user input).
#[derive(Debug, Default)]
pub struct AuthState {
    /// The active attempt, if any.
    flow: Option<AuthFlow>,
}

impl AuthState {
    /// The active attempt, if any.
    #[must_use]
    pub fn flow(&self) -> Option<&AuthFlow> {
        self.flow.as_ref()
    }

    /// Whether an attempt is in progress or showing its outcome.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.flow.is_some()
    }

    /// The provider and method of the active attempt, for retrying it.
    #[must_use]
    pub fn active_attempt(&self) -> Option<(AuthProviderId, LoginMethod)> {
        self.flow.as_ref().map(|flow| (flow.provider, flow.method))
    }

    /// The failure message currently shown, if the attempt failed.
    #[must_use]
    pub fn failure_message(&self) -> Option<&str> {
        match self.flow.as_ref().map(|flow| &flow.phase) {
            Some(AuthPhase::Failed { message }) => Some(message),
            _ => None,
        }
    }

    /// Starts a fresh attempt, replacing anything the modal was showing.
    pub fn begin(&mut self, provider: AuthProviderId, method: LoginMethod) {
        self.flow = Some(AuthFlow {
            provider,
            method,
            phase: AuthPhase::Starting,
        });
    }

    /// Updates what the active attempt is showing.
    ///
    /// Ignored when no attempt is active, so a late report from a cancelled
    /// attempt cannot reopen the modal.
    pub fn report(&mut self, phase: AuthPhase) {
        if let Some(flow) = self.flow.as_mut() {
            flow.phase = phase;
        }
    }

    /// Shows a failure in the modal, keeping the attempt's provider and method
    /// so the user can retry.
    pub fn fail(&mut self, message: String) {
        self.report(AuthPhase::Failed { message });
    }

    /// Clears the modal.
    pub fn clear(&mut self) {
        self.flow = None;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;

    #[rstest::rstest]
    fn a_fresh_state_has_no_attempt() {
        // Given untouched authentication state.
        let state = AuthState::default();

        // When asking whether an attempt is active.
        // Then none is.
        assert!(!state.is_active());
    }

    #[rstest::rstest]
    fn beginning_an_attempt_records_its_provider_and_method() {
        // Given untouched authentication state.
        let mut state = AuthState::default();

        // When an attempt begins.
        state.begin(AuthProviderId::OpenAiCodex, LoginMethod::DeviceCode);

        // Then the attempt can be retried with the same choices.
        assert_eq!(
            state.active_attempt(),
            Some((AuthProviderId::OpenAiCodex, LoginMethod::DeviceCode))
        );
    }

    #[rstest::rstest]
    fn a_failure_is_shown_in_the_modal() {
        // Given an attempt in progress.
        let mut state = AuthState::default();
        state.begin(AuthProviderId::OpenAiCodex, LoginMethod::Browser);

        // When it fails.
        state.fail("the provider refused".to_owned());

        // Then the reason is available to show the user.
        assert_eq!(state.failure_message(), Some("the provider refused"));
    }

    #[rstest::rstest]
    fn a_failed_attempt_keeps_its_method_for_retry() {
        // Given an attempt that failed.
        let mut state = AuthState::default();
        state.begin(AuthProviderId::OpenAiCodex, LoginMethod::Browser);
        state.fail("the provider refused".to_owned());

        // When asking what to retry.
        // Then the original provider and method are still known.
        assert_eq!(
            state.active_attempt(),
            Some((AuthProviderId::OpenAiCodex, LoginMethod::Browser))
        );
    }

    #[rstest::rstest]
    fn a_report_after_clearing_does_not_reopen_the_modal() {
        // Given an attempt that was cancelled.
        let mut state = AuthState::default();
        state.begin(AuthProviderId::OpenAiCodex, LoginMethod::Browser);
        state.clear();

        // When a late progress report arrives.
        state.report(AuthPhase::Working {
            message: "still going".to_owned(),
        });

        // Then the modal stays closed.
        assert!(!state.is_active());
    }
}
