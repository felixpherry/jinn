//! Rendering for the authentication modals.
//!
//! All four screens are the same Telescope-style popup: the login provider
//! list, the login-method list, the modal that carries an authorization
//! through to completion, and the logout list. The progress modal reuses the
//! popup's input line as the place to paste an authorization code.

use jinn_selection_widget::SelectionWidget;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::common::render_ctx::RenderCtx;
use crate::feat::auth::state::AuthPhase;
use crate::feat::theme::Theme;
use crate::feat::ui::picker_states::PickerExt;

/// Renders the login provider picker.
pub fn render_login_picker(frame: &mut Frame<'_>, area: Rect, ctx: &RenderCtx) {
    let state = ctx.state;
    let theme = &state.frontend.theme;
    let widget = SelectionWidget::new(state.frontend.auth_login_picker())
        .title(Line::from(" Log in "))
        .title_style(Style::default().fg(theme.popup_title))
        .footer(hint_line(theme, "Enter", "log in"));
    widget.render(frame, area);
}

/// Renders the login-method picker.
pub fn render_auth_method_picker(frame: &mut Frame<'_>, area: Rect, ctx: &RenderCtx) {
    let state = ctx.state;
    let theme = &state.frontend.theme;
    let widget = SelectionWidget::new(state.frontend.auth_method_picker())
        .title(Line::from(" Login method "))
        .title_style(Style::default().fg(theme.popup_title))
        .footer(hint_line(theme, "Enter", "start login"));
    widget.render(frame, area);
}

/// Renders the logout picker.
///
/// With no stored accounts the results area is simply blank — the ordinary
/// empty picker, with no special empty-state message.
pub fn render_logout_picker(frame: &mut Frame<'_>, area: Rect, ctx: &RenderCtx) {
    let state = ctx.state;
    let theme = &state.frontend.theme;
    let widget = SelectionWidget::new(state.frontend.auth_logout_picker())
        .title(Line::from(" Log out "))
        .title_style(Style::default().fg(theme.popup_title))
        .footer(hint_line(theme, "Enter", "log out"));
    widget.render(frame, area);
}

/// Renders the modal that carries an authorization through to completion.
///
/// The body shows what the user should do next — an authorization URL, a
/// device code, progress, or an error — while the input line takes a pasted
/// authorization code or redirect URL.
pub fn render_auth_progress(frame: &mut Frame<'_>, area: Rect, ctx: &RenderCtx) {
    let state = ctx.state;
    let theme = &state.frontend.theme;
    let title = state.auth.flow().map_or_else(
        || " Authenticating ".to_owned(),
        |flow| format!(" {} login ", flow.provider.display_name()),
    );
    let footer = if state.auth.failure_message().is_some() {
        hint_line(theme, "Enter", "retry")
    } else {
        hint_line(theme, "Enter", "submit code")
    };

    let widget = SelectionWidget::new(state.frontend.auth_progress_picker())
        .title(Line::from(title))
        .title_style(Style::default().fg(theme.popup_title))
        .body(progress_body(
            state.auth.flow().map(|flow| &flow.phase),
            theme,
        ))
        .footer(footer);
    widget.render(frame, area);
}

/// Builds the body lines for the authentication modal.
fn progress_body(phase: Option<&AuthPhase>, theme: &Theme) -> Vec<Line<'static>> {
    let muted = Style::default().fg(theme.muted_text);
    let primary = Style::default().fg(theme.primary_text);
    let accent = Style::default().fg(theme.accent_action);

    let Some(phase) = phase else {
        return vec![Line::styled("No authentication in progress.", muted)];
    };

    match phase {
        AuthPhase::Starting => vec![Line::styled("Starting authentication…", muted)],
        AuthPhase::Authorizing { url, instructions } => vec![
            Line::styled(instructions.clone(), muted),
            Line::from(""),
            Line::styled(url.clone(), accent),
            Line::from(""),
            Line::styled("Waiting for authorization…", muted),
        ],
        AuthPhase::DeviceCode {
            verification_uri,
            user_code,
        } => vec![
            Line::styled("Open this page and enter the code:", muted),
            Line::from(""),
            Line::styled(verification_uri.clone(), accent),
            Line::from(vec![Span::styled(
                user_code.clone(),
                primary.add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::styled("Waiting for authorization…", muted),
        ],
        AuthPhase::Working { message } => vec![Line::styled(message.clone(), muted)],
        AuthPhase::Failed { message } => vec![
            Line::styled(
                message.clone(),
                Style::default()
                    .fg(theme.error_text)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(""),
            Line::styled("Press Enter to try again, or Esc to close.", muted),
        ],
    }
}

/// Builds the one-line footer hint shared by the authentication modals.
fn hint_line(theme: &Theme, key: &str, action: &str) -> Line<'static> {
    let muted = Style::default().fg(theme.muted_text);
    let accent = Style::default().fg(theme.accent_action);
    Line::from(vec![
        Span::styled(format!("{key} "), accent),
        Span::styled(format!("{action} · "), muted),
        Span::styled("ESC ".to_owned(), accent),
        Span::styled("cancel".to_owned(), muted),
    ])
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
    use crate::common::app_state::AppState;
    use crate::feat::auth::picker_entry::{AuthMethodEntry, AuthProviderEntry};
    use crate::feat::theme::default_theme;
    use jinn_auth::{AuthProviderId, LoginMethod};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// Renders `render` against a test backend and returns the screen text.
    fn screen<F>(state: &AppState, render: F) -> String
    where
        F: FnOnce(&mut Frame<'_>, Rect, &RenderCtx),
    {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");
        terminal
            .draw(|frame| {
                let ctx = RenderCtx::new(state);
                render(frame, Rect::new(0, 0, 100, 30), &ctx);
            })
            .expect("draw");
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn provider_entry(is_configured: bool) -> AuthProviderEntry {
        AuthProviderEntry::new(
            AuthProviderId::OpenAiCodex,
            is_configured,
            vec![LoginMethod::Browser, LoginMethod::DeviceCode],
            default_theme(),
        )
    }

    #[rstest::rstest]
    #[test]
    fn the_login_picker_lists_the_supported_provider() {
        // Given a login picker holding the OpenAI Codex row.
        let mut state = AppState::default();
        state
            .frontend
            .auth_login_picker_mut()
            .set_items(vec![provider_entry(false)]);

        // When rendering it.
        let screen = screen(&state, render_login_picker);

        // Then the provider is listed by name.
        assert!(
            screen.contains("OpenAI Codex"),
            "the login picker must list the provider: {screen}"
        );
    }

    #[rstest::rstest]
    #[test]
    fn an_unconfigured_provider_row_says_so() {
        // Given a provider with no stored credentials.
        let mut state = AppState::default();
        state
            .frontend
            .auth_login_picker_mut()
            .set_items(vec![provider_entry(false)]);

        // When rendering the login picker.
        let screen = screen(&state, render_login_picker);

        // Then the row shows the unconfigured status.
        assert!(
            screen.contains("unconfigured"),
            "the row must show its credential status: {screen}"
        );
    }

    #[rstest::rstest]
    #[test]
    fn a_configured_provider_row_says_stored() {
        // Given a provider with stored credentials.
        let mut state = AppState::default();
        state
            .frontend
            .auth_login_picker_mut()
            .set_items(vec![provider_entry(true)]);

        // When rendering the login picker.
        let screen = screen(&state, render_login_picker);

        // Then the row shows the stored status.
        assert!(
            screen.contains("stored"),
            "the row must show its credential status: {screen}"
        );
    }

    #[rstest::rstest]
    #[test]
    fn the_method_picker_offers_browser_login() {
        // Given a method picker holding both methods.
        let mut state = AppState::default();
        state.frontend.auth_method_picker_mut().set_items(vec![
            AuthMethodEntry::new(
                AuthProviderId::OpenAiCodex,
                LoginMethod::Browser,
                default_theme(),
            ),
            AuthMethodEntry::new(
                AuthProviderId::OpenAiCodex,
                LoginMethod::DeviceCode,
                default_theme(),
            ),
        ]);

        // When rendering it.
        let screen = screen(&state, render_auth_method_picker);

        // Then browser login is offered.
        assert!(
            screen.contains("Browser login"),
            "browser login must be offered: {screen}"
        );
    }

    #[rstest::rstest]
    #[test]
    fn the_method_picker_offers_device_code_login() {
        // Given a method picker holding both methods.
        let mut state = AppState::default();
        state.frontend.auth_method_picker_mut().set_items(vec![
            AuthMethodEntry::new(
                AuthProviderId::OpenAiCodex,
                LoginMethod::Browser,
                default_theme(),
            ),
            AuthMethodEntry::new(
                AuthProviderId::OpenAiCodex,
                LoginMethod::DeviceCode,
                default_theme(),
            ),
        ]);

        // When rendering it.
        let screen = screen(&state, render_auth_method_picker);

        // Then device code login is offered.
        assert!(
            screen.contains("Device code login"),
            "device code login must be offered: {screen}"
        );
    }

    #[rstest::rstest]
    #[test]
    fn the_modal_shows_the_authorization_url() {
        // Given an attempt waiting on browser authorization.
        let mut state = AppState::default();
        state
            .auth
            .begin(AuthProviderId::OpenAiCodex, LoginMethod::Browser);
        state.auth.report(AuthPhase::Authorizing {
            url: "https://auth.example/authorize".to_owned(),
            instructions: "A browser window should have opened.".to_owned(),
        });

        // When rendering the modal.
        let screen = screen(&state, render_auth_progress);

        // Then the authorization URL is shown so it can be opened manually.
        assert!(
            screen.contains("https://auth.example/authorize"),
            "the authorization URL must be visible: {screen}"
        );
    }

    #[rstest::rstest]
    #[test]
    fn the_modal_shows_the_device_code() {
        // Given an attempt waiting on device authorization.
        let mut state = AppState::default();
        state
            .auth
            .begin(AuthProviderId::OpenAiCodex, LoginMethod::DeviceCode);
        state.auth.report(AuthPhase::DeviceCode {
            verification_uri: "https://auth.example/device".to_owned(),
            user_code: "ABCD-1234".to_owned(),
        });

        // When rendering the modal.
        let screen = screen(&state, render_auth_progress);

        // Then the code the user must type is shown.
        assert!(
            screen.contains("ABCD-1234"),
            "the device code must be visible: {screen}"
        );
    }

    #[rstest::rstest]
    #[test]
    fn the_modal_shows_the_verification_page() {
        // Given an attempt waiting on device authorization.
        let mut state = AppState::default();
        state
            .auth
            .begin(AuthProviderId::OpenAiCodex, LoginMethod::DeviceCode);
        state.auth.report(AuthPhase::DeviceCode {
            verification_uri: "https://auth.example/device".to_owned(),
            user_code: "ABCD-1234".to_owned(),
        });

        // When rendering the modal.
        let screen = screen(&state, render_auth_progress);

        // Then the page to visit is shown.
        assert!(
            screen.contains("https://auth.example/device"),
            "the verification page must be visible: {screen}"
        );
    }

    #[rstest::rstest]
    #[test]
    fn the_modal_shows_progress_while_waiting() {
        // Given an attempt reporting progress.
        let mut state = AppState::default();
        state
            .auth
            .begin(AuthProviderId::OpenAiCodex, LoginMethod::Browser);
        state.auth.report(AuthPhase::Working {
            message: "Exchanging authorization code".to_owned(),
        });

        // When rendering the modal.
        let screen = screen(&state, render_auth_progress);

        // Then the user can see the attempt is still active.
        assert!(
            screen.contains("Exchanging authorization code"),
            "progress must be visible: {screen}"
        );
    }

    #[rstest::rstest]
    #[test]
    fn the_modal_shows_a_failure_inline() {
        // Given an attempt that failed.
        let mut state = AppState::default();
        state
            .auth
            .begin(AuthProviderId::OpenAiCodex, LoginMethod::Browser);
        state.auth.fail("Authorization was denied.".to_owned());

        // When rendering the modal.
        let screen = screen(&state, render_auth_progress);

        // Then the error is shown without losing the authentication context.
        assert!(
            screen.contains("Authorization was denied."),
            "the failure must be shown inline: {screen}"
        );
    }

    #[rstest::rstest]
    #[test]
    fn the_modal_offers_retry_after_a_failure() {
        // Given an attempt that failed.
        let mut state = AppState::default();
        state
            .auth
            .begin(AuthProviderId::OpenAiCodex, LoginMethod::Browser);
        state.auth.fail("Authorization was denied.".to_owned());

        // When rendering the modal.
        let screen = screen(&state, render_auth_progress);

        // Then the footer offers a retry.
        assert!(
            screen.contains("retry"),
            "the footer must offer a retry: {screen}"
        );
    }

    #[rstest::rstest]
    #[test]
    fn an_empty_logout_picker_shows_no_explanatory_message() {
        // Given no stored accounts.
        let state = AppState::default();

        // When rendering the logout picker.
        let screen = screen(&state, render_logout_picker);

        // Then the results area is the ordinary blank list.
        assert!(
            !screen.contains("No accounts"),
            "the empty logout picker must not add an empty-state message: {screen}"
        );
    }

    #[rstest::rstest]
    #[test]
    fn the_logout_picker_lists_a_stored_account() {
        // Given one stored account.
        let mut state = AppState::default();
        state
            .frontend
            .auth_logout_picker_mut()
            .set_items(vec![provider_entry(true)]);

        // When rendering the logout picker.
        let screen = screen(&state, render_logout_picker);

        // Then the account is listed.
        assert!(
            screen.contains("OpenAI Codex"),
            "the stored account must be listed: {screen}"
        );
    }
}
