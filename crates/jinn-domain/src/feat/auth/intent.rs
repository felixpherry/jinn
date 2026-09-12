//! Synchronous handling of the authentication intents.
//!
//! These functions validate the request, move the modal to its next step, and
//! return the commands the auth actor acts on. No network access or storage
//! happens here.

use crate::common::app_state::{AppState, FocusScope};
use crate::feat::auth::picker_entry::AuthMethodEntry;
use crate::feat::auth::protocol::command::{
    CancelLogin, LoadLoginPickerEntries, LoadLogoutPickerEntries, Logout, StartLogin,
    SubmitAuthorizationCode,
};
use crate::feat::auth::validator;
use crate::feat::ui::picker_states::PickerExt;
use crate::protocol::{IntentResult, PickerKind};

/// Opens the login provider picker.
pub fn handle_open_login_picker(state: &mut AppState) -> IntentResult {
    if validator::validate_open_auth_picker(state).is_err() {
        return IntentResult::empty();
    }
    state.frontend.auth_login_picker_mut().reset();
    state.frontend.scope_stack.push(FocusScope::Picker {
        kind: PickerKind::AuthLogin,
    });
    IntentResult::new_message(LoadLoginPickerEntries)
}

/// Opens the logout picker.
pub fn handle_open_logout_picker(state: &mut AppState) -> IntentResult {
    if validator::validate_open_auth_picker(state).is_err() {
        return IntentResult::empty();
    }
    state.frontend.auth_logout_picker_mut().reset();
    state.frontend.scope_stack.push(FocusScope::Picker {
        kind: PickerKind::AuthLogout,
    });
    IntentResult::new_message(LoadLogoutPickerEntries)
}

/// Moves from the chosen provider to its login-method picker.
///
/// The methods come from the selected row, so a provider that supports only
/// one of them lists only that one.
pub fn confirm_login_provider(state: &mut AppState) -> IntentResult {
    let Some(entry) = state.frontend.auth_login_picker().selected_item() else {
        return IntentResult::empty();
    };
    let provider = entry.provider;
    let theme = state.frontend.theme.clone();
    let methods: Vec<AuthMethodEntry> = entry
        .methods
        .iter()
        .map(|method| AuthMethodEntry::new(provider, *method, theme.clone()))
        .collect();

    state.frontend.auth_method_picker_mut().reset();
    state.frontend.auth_method_picker_mut().set_items(methods);
    state.frontend.scope_stack.push(FocusScope::Picker {
        kind: PickerKind::AuthMethod,
    });
    IntentResult::empty()
}

/// Starts the login with the chosen method and opens the progress modal.
pub fn confirm_login_method(state: &mut AppState) -> IntentResult {
    let Some(entry) = state.frontend.auth_method_picker().selected_item() else {
        return IntentResult::empty();
    };
    let (provider, method) = (entry.provider, entry.method);

    state.frontend.auth_progress_picker_mut().reset();
    state.frontend.scope_stack.push(FocusScope::Picker {
        kind: PickerKind::AuthProgress,
    });
    IntentResult::new_message(StartLogin { provider, method })
}

/// Handles Enter in the authentication modal.
///
/// After a failure, Enter retries the same method. Otherwise it submits
/// whatever the user typed as an authorization code. An empty input with no
/// failure to retry does nothing.
pub fn confirm_auth_progress(state: &mut AppState) -> IntentResult {
    if state.auth.failure_message().is_some() {
        let Some((provider, method)) = state.auth.active_attempt() else {
            return IntentResult::empty();
        };
        state.frontend.auth_progress_picker_mut().reset();
        return IntentResult::new_message(StartLogin { provider, method });
    }

    let input = state.frontend.auth_progress_picker().filter().to_owned();
    if input.trim().is_empty() {
        return IntentResult::empty();
    }
    state.frontend.auth_progress_picker_mut().clear_filter();
    IntentResult::new_message(SubmitAuthorizationCode { input })
}

/// Logs the selected provider out.
pub fn confirm_logout(state: &mut AppState) -> IntentResult {
    let Some(entry) = state.frontend.auth_logout_picker().selected_item() else {
        return IntentResult::empty();
    };
    let provider = entry.provider;
    state.frontend.scope_stack.clear_overlays();
    IntentResult::new_message(Logout { provider })
}

/// Abandons the active attempt and closes the modal.
///
/// Cancelling stops the attempt's callback listener and polling, and a result
/// arriving afterwards is ignored, so cancelling can never change the stored
/// account.
pub fn handle_cancel_authentication(state: &mut AppState) -> IntentResult {
    let was_active = state.auth.is_active();
    state.auth.clear();
    state.frontend.scope_stack.clear_overlays();
    if was_active {
        return IntentResult::new_message(CancelLogin);
    }
    IntentResult::empty()
}

/// Whether `kind` is one of the authentication modals.
#[must_use]
pub fn is_auth_picker(kind: PickerKind) -> bool {
    matches!(
        kind,
        PickerKind::AuthLogin
            | PickerKind::AuthMethod
            | PickerKind::AuthProgress
            | PickerKind::AuthLogout
    )
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
    use crate::feat::auth::picker_entry::AuthProviderEntry;
    use crate::feat::auth::state::AuthPhase;
    use jinn_auth::{AuthProviderId, LoginMethod};

    /// A state whose login picker holds one provider row.
    fn state_with_login_row(methods: Vec<LoginMethod>) -> AppState {
        let mut state = AppState::default();
        let theme = state.frontend.theme.clone();
        state
            .frontend
            .auth_login_picker_mut()
            .set_items(vec![AuthProviderEntry::new(
                AuthProviderId::OpenAiCodex,
                false,
                methods,
                theme,
            )]);
        state
    }

    /// Opens the login picker and walks it down to the progress modal.
    fn state_in_progress_modal(method: LoginMethod) -> AppState {
        let mut state = state_with_login_row(vec![method]);
        let _ = handle_open_login_picker(&mut state);
        let _ = confirm_login_provider(&mut state);
        let _ = confirm_login_method(&mut state);
        state
    }

    #[rstest::rstest]
    fn opening_the_login_picker_shows_it() {
        // Given the application with no picker open.
        let mut state = AppState::default();

        // When the user asks to log in.
        let result = handle_open_login_picker(&mut state);

        // Then the login picker is on screen, and its rows are requested.
        assert_eq!(
            state.frontend.scope_stack.picker_kind().copied(),
            Some(PickerKind::AuthLogin)
        );
        assert!(
            result.message_names[0].contains("LoadLoginPickerEntries"),
            "opening the login picker must load its rows: {:?}",
            result.message_names
        );
    }

    #[rstest::rstest]
    fn opening_the_logout_picker_shows_it() {
        // Given the application with no picker open.
        let mut state = AppState::default();

        // When the user asks to log out.
        let result = handle_open_logout_picker(&mut state);

        // Then the logout picker is on screen, and its rows are requested.
        assert_eq!(
            state.frontend.scope_stack.picker_kind().copied(),
            Some(PickerKind::AuthLogout)
        );
        assert!(
            result.message_names[0].contains("LoadLogoutPickerEntries"),
            "opening the logout picker must load its rows: {:?}",
            result.message_names
        );
    }

    #[rstest::rstest]
    #[case(vec![LoginMethod::Browser], 1)]
    #[case(vec![LoginMethod::Browser, LoginMethod::DeviceCode], 2)]
    fn the_method_picker_offers_only_what_the_provider_supports(
        #[case] methods: Vec<LoginMethod>,
        #[case] expected: usize,
    ) {
        // Given a login picker whose provider supports those methods.
        let mut state = state_with_login_row(methods);
        let _ = handle_open_login_picker(&mut state);

        // When the user chooses that provider.
        let _ = confirm_login_provider(&mut state);

        // Then the method picker lists exactly the supported methods.
        assert_eq!(
            state.frontend.scope_stack.picker_kind().copied(),
            Some(PickerKind::AuthMethod)
        );
        assert_eq!(state.frontend.auth_method_picker().items().len(), expected);
    }

    #[rstest::rstest]
    fn choosing_a_method_starts_that_login() {
        // Given the method picker, opened from the provider row.
        let mut state = state_with_login_row(vec![LoginMethod::DeviceCode]);
        let _ = handle_open_login_picker(&mut state);
        let _ = confirm_login_provider(&mut state);

        // When the user picks a method.
        let result = confirm_login_method(&mut state);

        // Then the modal opens and the login is asked to start.
        assert_eq!(
            state.frontend.scope_stack.picker_kind().copied(),
            Some(PickerKind::AuthProgress)
        );
        assert!(
            result.message_names[0].contains("StartLogin"),
            "choosing a method must start the login: {:?}",
            result.message_names
        );
    }

    #[rstest::rstest]
    fn enter_after_a_failure_retries_the_same_method() {
        // Given a login attempt that failed in the modal.
        let mut state = state_in_progress_modal(LoginMethod::Browser);
        state
            .auth
            .begin(AuthProviderId::OpenAiCodex, LoginMethod::Browser);
        state.auth.fail("the provider refused".to_owned());

        // When the user presses Enter.
        let result = confirm_auth_progress(&mut state);

        // Then the same login is started again.
        assert!(
            result.message_names[0].contains("StartLogin"),
            "Enter after a failure must retry: {:?}",
            result.message_names
        );
    }

    #[rstest::rstest]
    fn enter_submits_a_pasted_authorization_code() {
        // Given a login waiting on the user, with a code typed into the modal.
        let mut state = state_in_progress_modal(LoginMethod::Browser);
        state
            .auth
            .begin(AuthProviderId::OpenAiCodex, LoginMethod::Browser);
        state.auth.report(AuthPhase::Working {
            message: "waiting".to_owned(),
        });
        for ch in "pasted-code".chars() {
            state.frontend.auth_progress_picker_mut().insert_char(ch);
        }

        // When the user presses Enter.
        let result = confirm_auth_progress(&mut state);

        // Then the code is handed to the attempt.
        assert!(
            result.message_names[0].contains("SubmitAuthorizationCode"),
            "Enter must submit the pasted code: {:?}",
            result.message_names
        );
    }

    #[rstest::rstest]
    fn enter_with_nothing_typed_and_nothing_to_retry_does_nothing() {
        // Given a login in progress with an empty input.
        let mut state = state_in_progress_modal(LoginMethod::Browser);
        state
            .auth
            .begin(AuthProviderId::OpenAiCodex, LoginMethod::Browser);

        // When the user presses Enter.
        let result = confirm_auth_progress(&mut state);

        // Then nothing is asked of the attempt.
        assert!(
            result.message_names.is_empty(),
            "an empty input must not submit: {:?}",
            result.message_names
        );
    }

    #[rstest::rstest]
    fn escape_abandons_the_attempt_and_closes_the_modal() {
        // Given a login attempt in progress.
        let mut state = state_in_progress_modal(LoginMethod::Browser);
        state
            .auth
            .begin(AuthProviderId::OpenAiCodex, LoginMethod::Browser);

        // When the user presses Escape.
        let result = handle_cancel_authentication(&mut state);

        // Then the attempt is cancelled and the modal is gone.
        assert!(
            result.message_names[0].contains("CancelLogin"),
            "Escape must cancel the attempt: {:?}",
            result.message_names
        );
        assert!(!state.auth.is_active());
        assert_eq!(state.frontend.scope_stack.picker_kind().copied(), None);
    }

    #[rstest::rstest]
    fn escape_with_no_attempt_running_just_closes_the_picker() {
        // Given the login picker open, with no attempt started.
        let mut state = AppState::default();
        let _ = handle_open_login_picker(&mut state);

        // When the user presses Escape.
        let result = handle_cancel_authentication(&mut state);

        // Then the picker closes without cancelling anything.
        assert!(
            result.message_names.is_empty(),
            "there is no attempt to cancel: {:?}",
            result.message_names
        );
        assert_eq!(state.frontend.scope_stack.picker_kind().copied(), None);
    }

    #[rstest::rstest]
    fn confirming_logout_removes_that_providers_credentials() {
        // Given the logout picker listing a stored account.
        let mut state = AppState::default();
        let theme = state.frontend.theme.clone();
        let _ = handle_open_logout_picker(&mut state);
        state
            .frontend
            .auth_logout_picker_mut()
            .set_items(vec![AuthProviderEntry::new(
                AuthProviderId::OpenAiCodex,
                true,
                vec![LoginMethod::Browser],
                theme,
            )]);

        // When the user confirms the row.
        let result = confirm_logout(&mut state);

        // Then the credentials are asked to be removed and the picker closes.
        assert!(
            result.message_names[0].contains("Logout"),
            "confirming must log the provider out: {:?}",
            result.message_names
        );
        assert_eq!(state.frontend.scope_stack.picker_kind().copied(), None);
    }

    #[rstest::rstest]
    fn an_empty_logout_picker_confirms_nothing() {
        // Given the logout picker with no stored accounts.
        let mut state = AppState::default();
        let _ = handle_open_logout_picker(&mut state);

        // When the user presses Enter on the blank list.
        let result = confirm_logout(&mut state);

        // Then nothing is logged out.
        assert!(
            result.message_names.is_empty(),
            "an empty picker has nothing to log out: {:?}",
            result.message_names
        );
    }
}
