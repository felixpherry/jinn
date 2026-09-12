//! Authentication capsule: cap + view + extension traits, colocated.
//!
//! Write access to [`AuthState`] and to the authentication pickers is gated by
//! an unforgeable ZST token ([`AuthCap`]). The projection method
//! [`State::with_auth`] hands the cap-holder a narrow borrowed view
//! ([`AuthView`]).

#[cfg(test)]
use crate::common::app_state::AppState;
use crate::common::state::State;
use crate::feat::auth::picker_entry::{AuthMethodEntry, AuthProviderEntry};
use crate::feat::auth::state::{AuthPhase, AuthState};
use crate::feat::ui::frontend_state::FrontendState;
use crate::feat::ui::picker_states::PickerExt;
use crate::protocol::PickerKind;
use jinn_auth::{AuthProviderId, LoginMethod};

// ── The cap ──────────────────────────────────────────────────────────────────

/// Proof of authority to write `AuthState` and the auth-owned pickers.
/// Minted only via [`crate::common::tcaps::mint`].
#[derive(Clone, Copy)]
pub struct AuthCap(());

impl AuthCap {
    /// Private constructor scoped to the `tcaps/` subtree.
    ///
    /// MUST be `pub(in crate::common::tcaps)`, NOT `pub(crate)`. `pub(crate)`
    /// lets any module forge the cap.
    pub(in crate::common::tcaps) fn new() -> Self {
        Self(())
    }
}

// ── Per-struct narrow newtypes ───────────────────────────────────────────────

/// Narrow write-handle to `AuthState`.
pub struct AuthOps<'a>(&'a mut AuthState);

/// Narrow write-handle to the auth-owned pickers and the modal's scope.
pub struct FrontendAuthOps<'a>(&'a mut FrontendState);

// ── Composite facade ─────────────────────────────────────────────────────────

/// What an auth-writer sees: the authentication state plus the pickers and
/// modal scope that present it.
pub struct AuthView<'a> {
    /// Mutable authentication state.
    pub auth: AuthOps<'a>,
    /// Mutable auth-owned frontend pickers, plus the theme read needed to
    /// build their rows.
    pub frontend: FrontendAuthOps<'a>,
}

// ── Extension traits (the opt-in method menu) ───────────────────────────────

/// Write access to the active authentication attempt.
pub trait AuthFlowWrite {
    /// Starts a fresh attempt.
    fn begin_attempt(&mut self, provider: AuthProviderId, method: LoginMethod);
    /// Updates what the active attempt is showing.
    fn report_phase(&mut self, phase: AuthPhase);
    /// Shows a failure, keeping the attempt available for retry.
    fn fail_attempt(&mut self, message: String);
    /// Clears the modal.
    fn clear_attempt(&mut self);
    /// Whether an attempt is active.
    fn has_attempt(&self) -> bool;
}

/// Write access to the auth-owned pickers and the modal scope.
pub trait FrontendAuthWrite {
    /// Replaces the login picker's provider rows.
    fn set_login_picker_items(&mut self, items: Vec<AuthProviderEntry>);
    /// Replaces the logout picker's provider rows.
    fn set_logout_picker_items(&mut self, items: Vec<AuthProviderEntry>);
    /// Replaces the login-method picker's rows.
    fn set_auth_method_picker_items(&mut self, items: Vec<AuthMethodEntry>);
    /// Reads the frontend theme, for building picker rows.
    fn theme(&self) -> &crate::feat::theme::Theme;
    /// Closes the authentication modal if one is open.
    ///
    /// The auth actor is the only actor that writes the focus scope, and only
    /// to dismiss its own modal once a login has succeeded.
    fn close_auth_modal(&mut self);
}

impl AuthFlowWrite for AuthOps<'_> {
    fn begin_attempt(&mut self, provider: AuthProviderId, method: LoginMethod) {
        self.0.begin(provider, method);
    }

    fn report_phase(&mut self, phase: AuthPhase) {
        self.0.report(phase);
    }

    fn fail_attempt(&mut self, message: String) {
        self.0.fail(message);
    }

    fn clear_attempt(&mut self) {
        self.0.clear();
    }

    fn has_attempt(&self) -> bool {
        self.0.is_active()
    }
}

impl FrontendAuthWrite for FrontendAuthOps<'_> {
    fn set_login_picker_items(&mut self, items: Vec<AuthProviderEntry>) {
        self.0.auth_login_picker_mut().set_items(items);
    }

    fn set_logout_picker_items(&mut self, items: Vec<AuthProviderEntry>) {
        self.0.auth_logout_picker_mut().set_items(items);
    }

    fn set_auth_method_picker_items(&mut self, items: Vec<AuthMethodEntry>) {
        self.0.auth_method_picker_mut().set_items(items);
    }

    fn theme(&self) -> &crate::feat::theme::Theme {
        &self.0.theme
    }

    fn close_auth_modal(&mut self) {
        while matches!(
            self.0.scope_stack.picker_kind(),
            Some(
                PickerKind::AuthLogin
                    | PickerKind::AuthMethod
                    | PickerKind::AuthProgress
                    | PickerKind::AuthLogout
            )
        ) {
            self.0.scope_stack.pop();
        }
    }
}

// ── Projection method ────────────────────────────────────────────────────────

impl State {
    /// Write access to the authentication capsule, scoped via [`AuthView`].
    ///
    /// The cap is taken by reference to prove authority; it is not consumed.
    pub fn with_auth<R, F>(&self, _cap: &AuthCap, f: F) -> R
    where
        F: FnOnce(&mut AuthView<'_>) -> R,
    {
        let mut guard = self.write_lock();
        let app = &mut *guard;
        f(&mut AuthView {
            auth: AuthOps(&mut app.auth),
            frontend: FrontendAuthOps(&mut app.frontend),
        })
    }
}

// ── Test helper ─────────────────────────────────────────────────────────────

#[cfg(test)]
impl<'a> AuthView<'a> {
    /// Open an [`AuthView`] from raw [`AppState`] without a cap.
    ///
    /// Tests are trusted — they set up state and call the production code. The
    /// cap exists to enforce ownership at *actor* call sites, not test sites.
    pub(crate) fn from_app_state_for_test(app: &'a mut AppState) -> Self {
        AuthView {
            auth: AuthOps(&mut app.auth),
            frontend: FrontendAuthOps(&mut app.frontend),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        reason = "test code"
    )]
    use super::*;
    use crate::common::app_state::FocusScope;
    use crate::feat::auth::picker_entry::AuthProviderEntry;
    use jinn_auth::LoginMethod;

    /// A provider row for the pickers.
    fn row(is_configured: bool, theme: crate::feat::theme::Theme) -> AuthProviderEntry {
        AuthProviderEntry::new(
            AuthProviderId::OpenAiCodex,
            is_configured,
            vec![LoginMethod::Browser],
            theme,
        )
    }

    #[rstest::rstest]
    #[case(true)]
    #[case(false)]
    fn the_login_picker_carries_the_stored_status(#[case] is_configured: bool) {
        // Given an application with no login rows loaded.
        let mut app = AppState::default();

        // When the auth writer publishes a provider row.
        {
            let mut view = AuthView::from_app_state_for_test(&mut app);
            let theme = view.frontend.theme().clone();
            view.frontend
                .set_login_picker_items(vec![row(is_configured, theme)]);
        }

        // Then the row reports the provider's stored status.
        let items = app.frontend.auth_login_picker().items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].is_configured, is_configured);
    }

    #[rstest::rstest]
    fn closing_the_modal_dismisses_every_authentication_screen() {
        // Given the login picker, its method picker, and the modal stacked up.
        let mut app = AppState::default();
        for kind in [
            PickerKind::AuthLogin,
            PickerKind::AuthMethod,
            PickerKind::AuthProgress,
        ] {
            app.frontend.scope_stack.push(FocusScope::Picker { kind });
        }

        // When the auth writer closes the modal.
        AuthView::from_app_state_for_test(&mut app)
            .frontend
            .close_auth_modal();

        // Then the user is back where they started.
        assert_eq!(app.frontend.scope_stack.picker_kind(), None);
    }

    #[rstest::rstest]
    fn closing_the_modal_leaves_another_picker_alone() {
        // Given an ordinary picker open, with no authentication screen above it.
        let mut app = AppState::default();
        app.frontend.scope_stack.push(FocusScope::Picker {
            kind: PickerKind::Provider,
        });

        // When the auth writer closes its modal.
        AuthView::from_app_state_for_test(&mut app)
            .frontend
            .close_auth_modal();

        // Then the picker the user was in stays open.
        assert_eq!(
            app.frontend.scope_stack.picker_kind().copied(),
            Some(PickerKind::Provider)
        );
    }

    #[rstest::rstest]
    fn a_late_report_cannot_reopen_a_cancelled_attempt() {
        // Given an attempt the user cancelled.
        let mut app = AppState::default();
        {
            let mut view = AuthView::from_app_state_for_test(&mut app);
            view.auth
                .begin_attempt(AuthProviderId::OpenAiCodex, LoginMethod::Browser);
            view.auth.clear_attempt();
        }

        // When a progress report from that attempt arrives afterwards.
        {
            let mut view = AuthView::from_app_state_for_test(&mut app);
            view.auth.report_phase(AuthPhase::Working {
                message: "still going".to_owned(),
            });
        }

        // Then the modal stays closed.
        assert!(!app.auth.is_active());
    }
}
