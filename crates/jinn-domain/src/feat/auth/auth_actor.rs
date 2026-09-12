//! The single owner of subscription authentication.
//!
//! One actor covers the whole credential lifecycle: it lists providers for the
//! pickers, runs login attempts, persists their results, removes credentials
//! on logout, and keeps provider availability in step with what is stored.
//!
//! A login attempt runs as a spawned task carrying a cancellation signal and
//! an attempt number. Everything it reports travels back through the bus with
//! that number attached, so a result from an attempt the user cancelled — or
//! replaced with a newer one — is discarded rather than allowed to change the
//! stored account.

use jinn_auth::{AuthOperationFailure, AuthProviderId, CancelSignal, LoginMethod};
use kameo::prelude::{Actor, ActorRef, Context, Message};
use tokio::sync::oneshot;

use crate::common::actor_deps::{ActorDeps, BusPublish};
use crate::common::state::State;
use crate::common::tcaps::auth::{AuthCap, AuthFlowWrite, FrontendAuthWrite};
use crate::feat::auth::picker_entry::AuthProviderEntry;
use crate::feat::auth::protocol::command::{
    CancelLogin, LoadLoginPickerEntries, LoadLogoutPickerEntries, Logout, StartLogin,
    SubmitAuthorizationCode,
};
use crate::feat::auth::protocol::event::{
    AuthProgressReported, LoginAttemptFinished, SubscriptionCredentialsChanged,
};
use crate::feat::chat_input::protocol::command::PushChatEntry;
use crate::protocol::ChatEntry;

/// Owns subscription credentials and the login modal's contents.
pub struct AuthActor {
    deps: ActorDeps,
    state: State,
    cap: AuthCap,
    /// The attempt currently running, if any.
    active: Option<ActiveAttempt>,
    /// Number assigned to the next attempt.
    next_attempt: u64,
}

/// Dependencies for spawning an [`AuthActor`].
#[derive(Clone)]
pub struct AuthActorDeps {
    /// Universal actor dependencies (bus, services, etc.).
    pub deps: ActorDeps,
    /// Shared application state.
    pub state: State,
    /// Authentication write capability.
    pub cap: AuthCap,
}

/// A login attempt in flight.
struct ActiveAttempt {
    /// Number identifying this attempt in reports and results.
    id: u64,
    /// Stops the attempt's callback listener and polling.
    cancel: CancelSignal,
    /// Delivers a pasted authorization code to the attempt.
    pasted_code: Option<oneshot::Sender<String>>,
}

impl Actor for AuthActor {
    type Args = AuthActorDeps;
    type Error = kameo::error::Infallible;

    async fn on_start(args: Self::Args, actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        args.deps
            .subscribe(actor_ref.clone().recipient::<LoadLoginPickerEntries>())
            .await;
        args.deps
            .subscribe(actor_ref.clone().recipient::<LoadLogoutPickerEntries>())
            .await;
        args.deps
            .subscribe(actor_ref.clone().recipient::<StartLogin>())
            .await;
        args.deps
            .subscribe(actor_ref.clone().recipient::<CancelLogin>())
            .await;
        args.deps
            .subscribe(actor_ref.clone().recipient::<SubmitAuthorizationCode>())
            .await;
        args.deps
            .subscribe(actor_ref.clone().recipient::<Logout>())
            .await;
        args.deps
            .subscribe(actor_ref.clone().recipient::<AuthProgressReported>())
            .await;
        args.deps
            .subscribe(actor_ref.recipient::<LoginAttemptFinished>())
            .await;

        // Restore the credential snapshot so a login from an earlier run is
        // available immediately, without a fresh authorization.
        if let Err(err) = args.deps.services.auth.refresh_presence().await {
            tracing::warn!(error = ?err, "could not restore stored subscription credentials");
        }

        Ok(Self {
            deps: args.deps,
            state: args.state,
            cap: args.cap,
            active: None,
            next_attempt: 1,
        })
    }
}

impl BusPublish for AuthActor {
    fn bus(&self) -> &crate::common::services::bus_service::BusService {
        self.deps.bus()
    }
}

impl Message<LoadLoginPickerEntries> for AuthActor {
    type Reply = ();

    async fn handle(
        &mut self,
        _msg: LoadLoginPickerEntries,
        _ctx: &mut Context<Self, Self::Reply>,
    ) {
        let auth = self.deps.services.auth.clone();
        self.state.with_auth(&self.cap, |view| {
            let theme = view.frontend.theme().clone();
            let entries = auth
                .provider_ids()
                .into_iter()
                .map(|provider| {
                    AuthProviderEntry::new(
                        provider,
                        auth.has_credentials(provider),
                        auth.methods(provider).to_vec(),
                        theme.clone(),
                    )
                })
                .collect();
            view.frontend.set_login_picker_items(entries);
        });
    }
}

impl Message<LoadLogoutPickerEntries> for AuthActor {
    type Reply = ();

    async fn handle(
        &mut self,
        _msg: LoadLogoutPickerEntries,
        _ctx: &mut Context<Self, Self::Reply>,
    ) {
        let auth = self.deps.services.auth.clone();
        self.state.with_auth(&self.cap, |view| {
            let theme = view.frontend.theme().clone();
            let entries = auth
                .provider_ids()
                .into_iter()
                .filter(|provider| auth.has_credentials(*provider))
                .map(|provider| {
                    AuthProviderEntry::new(
                        provider,
                        true,
                        auth.methods(provider).to_vec(),
                        theme.clone(),
                    )
                })
                .collect();
            view.frontend.set_logout_picker_items(entries);
        });
    }
}

impl Message<StartLogin> for AuthActor {
    type Reply = ();

    async fn handle(&mut self, msg: StartLogin, _ctx: &mut Context<Self, Self::Reply>) {
        self.start_login(msg.provider, msg.method);
    }
}

impl Message<CancelLogin> for AuthActor {
    type Reply = ();

    async fn handle(&mut self, _msg: CancelLogin, _ctx: &mut Context<Self, Self::Reply>) {
        self.abandon_active_attempt();
        self.state.with_auth(&self.cap, |view| {
            view.auth.clear_attempt();
        });
    }
}

impl Message<SubmitAuthorizationCode> for AuthActor {
    type Reply = ();

    async fn handle(
        &mut self,
        msg: SubmitAuthorizationCode,
        _ctx: &mut Context<Self, Self::Reply>,
    ) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        let Some(sender) = active.pasted_code.take() else {
            return;
        };
        if sender.send(msg.input).is_err() {
            tracing::debug!("the login attempt was no longer waiting for a pasted code");
        }
    }
}

impl Message<AuthProgressReported> for AuthActor {
    type Reply = ();

    async fn handle(&mut self, msg: AuthProgressReported, _ctx: &mut Context<Self, Self::Reply>) {
        if !self.is_current_attempt(msg.attempt) {
            return;
        }
        self.state.with_auth(&self.cap, |view| {
            view.auth.report_phase(msg.phase);
        });
    }
}

impl Message<LoginAttemptFinished> for AuthActor {
    type Reply = ();

    async fn handle(&mut self, msg: LoginAttemptFinished, _ctx: &mut Context<Self, Self::Reply>) {
        if !self.is_current_attempt(msg.attempt) {
            return;
        }
        self.active = None;

        match msg.failure {
            Some(message) => self.state.with_auth(&self.cap, |view| {
                view.auth.fail_attempt(message);
            }),
            None => self.finish_successful_login(msg.provider).await,
        }
    }
}

impl Message<Logout> for AuthActor {
    type Reply = ();

    async fn handle(&mut self, msg: Logout, _ctx: &mut Context<Self, Self::Reply>) {
        let auth = self.deps.services.auth.clone();
        if let Err(err) = auth.logout(msg.provider).await {
            tracing::warn!(error = ?err, "could not remove stored credentials");
            return;
        }
        self.publish(SubscriptionCredentialsChanged {
            provider: msg.provider,
            is_configured: false,
        })
        .await;
        self.push_notice(format!("Logged out of {}.", msg.provider.display_name()))
            .await;
    }
}

impl AuthActor {
    /// Whether `attempt` is the attempt currently running.
    fn is_current_attempt(&self, attempt: u64) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.id == attempt)
    }

    /// Stops the running attempt, if any, and forgets it.
    fn abandon_active_attempt(&mut self) {
        if let Some(active) = self.active.take() {
            active.cancel.cancel();
        }
    }

    /// Cancels any running attempt and starts a new one.
    fn start_login(&mut self, provider: AuthProviderId, method: LoginMethod) {
        self.abandon_active_attempt();

        let attempt = self.next_attempt;
        self.next_attempt = self.next_attempt.saturating_add(1);

        let cancel = CancelSignal::new();
        let (interaction, pasted_code) = crate::feat::auth::interaction::BusAuthInteraction::new(
            attempt,
            self.deps.services.bridge.clone(),
            cancel.clone(),
        );

        self.state.with_auth(&self.cap, |view| {
            view.auth.begin_attempt(provider, method);
        });

        self.active = Some(ActiveAttempt {
            id: attempt,
            cancel,
            pasted_code: Some(pasted_code),
        });

        let auth = self.deps.services.auth.clone();
        let bus = self.deps.services.bus.clone();
        self.deps.services.handle.spawn(async move {
            let failure = match auth.login(provider, method, interaction).await {
                Ok(()) => None,
                Err(report) => Some(describe_failure(&report)),
            };
            bus.publish(LoginAttemptFinished {
                attempt,
                provider,
                method,
                failure,
            })
            .await;
        });
    }

    /// Closes the modal, refreshes availability, and notifies the user.
    async fn finish_successful_login(&mut self, provider: AuthProviderId) {
        self.state.with_auth(&self.cap, |view| {
            view.auth.clear_attempt();
            view.frontend.close_auth_modal();
        });

        self.publish(SubscriptionCredentialsChanged {
            provider,
            is_configured: true,
        })
        .await;

        self.push_notice(format!(
            "Signed in to {}. Open the model picker to select a {} model.",
            provider.display_name(),
            provider.display_name()
        ))
        .await;
    }

    /// Shows a local notice in the chat area.
    ///
    /// System entries are excluded from assembled model context by default, so
    /// account management never influences the conversation.
    async fn push_notice(&self, text: String) {
        let session_id = self.state.read().session.active_session_id().clone();
        self.publish(PushChatEntry {
            session_id,
            entry: ChatEntry::system(text),
        })
        .await;
    }
}

/// Turns a failed operation into text for the modal.
fn describe_failure(report: &error_stack::Report<AuthOperationFailure>) -> String {
    let reason = match report.downcast_ref::<AuthOperationFailure>() {
        Some(AuthOperationFailure::Cancelled) => "Login was cancelled.",
        Some(AuthOperationFailure::Denied) => "Authorization was denied.",
        Some(AuthOperationFailure::TimedOut) => "Authorization timed out.",
        Some(AuthOperationFailure::Storage) => "Credentials could not be saved.",
        Some(AuthOperationFailure::UnknownProvider) => "That provider is not supported.",
        _ => "The provider could not be reached.",
    };
    reason.to_owned()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;

    #[rstest::rstest]
    #[case(AuthOperationFailure::Denied, "Authorization was denied.")]
    #[case(AuthOperationFailure::TimedOut, "Authorization timed out.")]
    #[case(AuthOperationFailure::Cancelled, "Login was cancelled.")]
    #[case(AuthOperationFailure::Provider, "The provider could not be reached.")]
    fn a_failure_is_described_for_the_modal(
        #[case] failure: AuthOperationFailure,
        #[case] expected: &str,
    ) {
        // Given a failed authentication operation.
        let report = error_stack::Report::new(failure);

        // When describing it for the modal.
        // Then the user sees the matching explanation.
        assert_eq!(describe_failure(&report), expected);
    }
}
