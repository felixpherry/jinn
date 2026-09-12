//! Validators for the authentication intents.

use wherror::Error;

use crate::common::app_state::AppState;

/// Why a login picker cannot be opened.
#[derive(Debug, Error)]
#[error(debug)]
pub enum OpenAuthPickerError {
    /// Another picker is already open; authentication never stacks on top of
    /// one.
    AlreadyInPicker,
}

/// Validates opening one of the authentication pickers.
///
/// # Errors
///
/// Returns [`OpenAuthPickerError::AlreadyInPicker`] when a picker is open.
pub fn validate_open_auth_picker(state: &AppState) -> Result<(), OpenAuthPickerError> {
    if state.frontend.scope_stack.is_picker() {
        return Err(OpenAuthPickerError::AlreadyInPicker);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;
    use crate::common::app_state::FocusScope;
    use crate::protocol::PickerKind;

    #[rstest::rstest]
    fn opening_is_allowed_from_the_chat_view() {
        // Given no picker open.
        let state = AppState::default();

        // When validating the open.
        // Then it is allowed.
        assert!(validate_open_auth_picker(&state).is_ok());
    }

    #[rstest::rstest]
    fn opening_is_rejected_while_another_picker_is_open() {
        // Given the model picker already open.
        let mut state = AppState::default();
        state.frontend.scope_stack.push(FocusScope::Picker {
            kind: PickerKind::Provider,
        });

        // When validating the open.
        // Then it is rejected.
        assert!(matches!(
            validate_open_auth_picker(&state),
            Err(OpenAuthPickerError::AlreadyInPicker)
        ));
    }
}
