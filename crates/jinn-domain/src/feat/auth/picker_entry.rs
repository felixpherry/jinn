//! Rows for the authentication pickers.
//!
//! The login picker lists the subscription providers jinn supports and
//! whether each already has stored credentials. The method picker lists the
//! login methods the chosen provider offers. The logout picker reuses the
//! provider row, listing only configured accounts.

use std::ops::Range;

use jinn_auth::{AuthProviderId, LoginMethod};
use jinn_selection_widget::{PickerItem, highlight_text_with_bg};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::feat::picker::style::selected_style;
use crate::feat::theme::Theme;

/// A subscription provider row.
#[derive(Debug, Clone)]
pub struct AuthProviderEntry {
    /// Which provider this row selects.
    pub provider: AuthProviderId,
    /// Display name shown in the row.
    pub name: String,
    /// Whether credentials for this provider are stored locally.
    ///
    /// Stored means a credential is on disk, not that a fresh request would
    /// succeed.
    pub is_configured: bool,
    /// The login methods this provider supports, in presentation order.
    ///
    /// Carried on the row so the method picker can be built without assuming
    /// every provider offers the same set.
    pub methods: Vec<LoginMethod>,
    /// Theme for rendering.
    pub theme: Theme,
}

impl AuthProviderEntry {
    /// Builds the row for `provider`.
    #[must_use]
    pub fn new(
        provider: AuthProviderId,
        is_configured: bool,
        methods: Vec<LoginMethod>,
        theme: Theme,
    ) -> Self {
        Self {
            provider,
            name: provider.display_name().to_owned(),
            is_configured,
            methods,
            theme,
        }
    }

    /// The status word shown beside the provider name.
    #[must_use]
    pub fn status_label(&self) -> &'static str {
        if self.is_configured {
            "stored"
        } else {
            "unconfigured"
        }
    }
}

impl PickerItem for AuthProviderEntry {
    fn display_label(&self) -> &str {
        &self.name
    }

    fn render_row(&self, is_selected: bool) -> Line<'static> {
        render_provider_row(self, is_selected, &[])
    }

    fn render_row_with_highlight(
        &self,
        is_selected: bool,
        match_indices: &[Range<usize>],
    ) -> Line<'static> {
        render_provider_row(self, is_selected, match_indices)
    }
}

/// Renders `marker name  (status)` for a provider row.
fn render_provider_row(
    entry: &AuthProviderEntry,
    is_selected: bool,
    match_indices: &[Range<usize>],
) -> Line<'static> {
    let marker = Span::styled(
        if entry.is_configured {
            "\u{25cf} "
        } else {
            "  "
        },
        if entry.is_configured {
            Style::default()
                .fg(entry.theme.picker_active_marker)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        },
    );
    let label_style = selected_style(is_selected, &entry.theme);

    let mut spans = vec![marker];
    if match_indices.is_empty() {
        spans.push(Span::styled(entry.name.clone(), label_style));
    } else {
        spans.extend(highlight_text_with_bg(
            &entry.name,
            label_style,
            match_indices,
            entry.theme.picker_highlight_bg,
        ));
    }
    spans.push(Span::styled(
        format!("  ({})", entry.status_label()),
        Style::default().fg(entry.theme.muted_text),
    ));
    Line::from(spans)
}

/// A login-method row.
#[derive(Debug, Clone)]
pub struct AuthMethodEntry {
    /// Which provider this row logs into.
    pub provider: AuthProviderId,
    /// Which method this row selects.
    pub method: LoginMethod,
    /// Row label.
    pub label: String,
    /// Theme for rendering.
    pub theme: Theme,
}

impl AuthMethodEntry {
    /// Builds the row for `method`.
    #[must_use]
    pub fn new(provider: AuthProviderId, method: LoginMethod, theme: Theme) -> Self {
        Self {
            provider,
            method,
            label: method.label().to_owned(),
            theme,
        }
    }
}

impl PickerItem for AuthMethodEntry {
    fn display_label(&self) -> &str {
        &self.label
    }

    fn render_row(&self, is_selected: bool) -> Line<'static> {
        render_method_row(self, is_selected, &[])
    }

    fn render_row_with_highlight(
        &self,
        is_selected: bool,
        match_indices: &[Range<usize>],
    ) -> Line<'static> {
        render_method_row(self, is_selected, match_indices)
    }
}

/// Renders `label — description` for a method row.
fn render_method_row(
    entry: &AuthMethodEntry,
    is_selected: bool,
    match_indices: &[Range<usize>],
) -> Line<'static> {
    let label_style = selected_style(is_selected, &entry.theme);
    let mut spans = vec![Span::styled("  ".to_owned(), Style::default())];
    if match_indices.is_empty() {
        spans.push(Span::styled(entry.label.clone(), label_style));
    } else {
        spans.extend(highlight_text_with_bg(
            &entry.label,
            label_style,
            match_indices,
            entry.theme.picker_highlight_bg,
        ));
    }
    spans.push(Span::styled(
        format!("  {}", entry.method.description()),
        Style::default().fg(entry.theme.muted_text),
    ));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;
    use crate::feat::theme::default_theme;

    #[rstest::rstest]
    fn a_provider_without_credentials_reads_as_unconfigured() {
        // Given a provider row with no stored credentials.
        let entry = AuthProviderEntry::new(
            AuthProviderId::OpenAiCodex,
            false,
            vec![LoginMethod::Browser],
            default_theme(),
        );

        // When reading its status.
        // Then it reports as unconfigured.
        assert_eq!(entry.status_label(), "unconfigured");
    }

    #[rstest::rstest]
    fn a_provider_with_credentials_reads_as_stored() {
        // Given a provider row with stored credentials.
        let entry = AuthProviderEntry::new(
            AuthProviderId::OpenAiCodex,
            true,
            vec![LoginMethod::Browser],
            default_theme(),
        );

        // When reading its status.
        // Then it reports as stored.
        assert_eq!(entry.status_label(), "stored");
    }

    #[rstest::rstest]
    fn a_provider_row_is_searchable_by_its_display_name() {
        // Given the OpenAI Codex provider row.
        let entry = AuthProviderEntry::new(
            AuthProviderId::OpenAiCodex,
            false,
            vec![LoginMethod::Browser],
            default_theme(),
        );

        // When reading the text the picker filters on.
        // Then it is the provider's display name.
        assert_eq!(entry.display_label(), "OpenAI Codex");
    }

    #[rstest::rstest]
    #[case(LoginMethod::Browser, "Browser login")]
    #[case(LoginMethod::DeviceCode, "Device code login")]
    fn a_method_row_is_searchable_by_its_label(
        #[case] method: LoginMethod,
        #[case] expected: &str,
    ) {
        // Given a login-method row.
        let entry = AuthMethodEntry::new(AuthProviderId::OpenAiCodex, method, default_theme());

        // When reading the text the picker filters on.
        // Then it is the method's label.
        assert_eq!(entry.display_label(), expected);
    }
}
