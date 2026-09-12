//! Selection widget - ratatui renderer for the picker popup.
//!
//! [`SelectionWidget`] renders a telescope-style popup overlay: a bordered block containing
//! a filter input row with a real cursor, a horizontal separator, scrollable result rows,
//! and an optional footer. Each result row is rendered via [`PickerItem::render_row`],
//! so the consumer controls all styling.
//!
//! The popup rectangle is computed by [`compute_popup_rect`] using configurable constants
//! for horizontal padding, minimum width, and maximum height fraction.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::{PickerItem, SelectionState};

/// Horizontal padding as a fraction of terminal width (10% each side).
pub const PICKER_H_PAD_FRAC: f32 = 0.10;
/// Minimum popup width in cells.
pub const PICKER_MIN_WIDTH: u16 = 30;
/// Maximum fraction of terminal height the picker popup may consume.
pub const PICKER_MAX_HEIGHT_FRAC: f32 = 0.75;

/// Theme-dependent colors for the selection widget.
///
/// Provides a minimal set of colors that the widget needs, decoupled from
/// the full `Theme` struct so this crate remains standalone.
#[derive(Debug, Clone)]
pub struct SelectionColors {
    /// Popup border color.
    pub border: Color,
    /// Filter input text color.
    pub filter_text: Color,
    /// Horizontal separator line color.
    pub separator: Color,
    /// Footer text color.
    pub footer: Color,
    /// Fuzzy match highlight background color.
    pub highlight_bg: Color,
}

impl Default for SelectionColors {
    fn default() -> Self {
        Self {
            border: Color::DarkGray,
            filter_text: Color::White,
            separator: Color::DarkGray,
            footer: Color::DarkGray,
            highlight_bg: Color::DarkGray,
        }
    }
}

/// Filter prompt displayed before the user's input text.
pub(crate) const PROMPT: &str = "> ";

/// Converts a sorted list of byte indices from fuzzy matching into sorted,
/// non-overlapping `Range<usize>` ranges.
///
/// Consecutive indices are merged into a single range. For example,
/// `[0, 1, 2, 5, 6]` becomes `[0..3, 5..7]`.
pub(crate) fn fuzzy_bytes_to_ranges(indices: &[usize]) -> Vec<std::ops::Range<usize>> {
    let mut iter = indices.iter().copied();
    let Some(first) = iter.next() else {
        return Vec::new();
    };

    let mut ranges = Vec::new();
    let mut start = first;
    let mut end = start + 1;
    for idx in iter {
        if idx == end {
            end = idx + 1;
        } else {
            ranges.push(start..end);
            start = idx;
            end = idx + 1;
        }
    }
    ranges.push(start..end);
    ranges
}

/// Computes the popup rectangle for the selection widget.
///
/// Uses ~20% total horizontal padding (10% each side) and positions the popup
/// in the top third of the terminal. Height scales with terminal size, capped
/// at [`PICKER_MAX_HEIGHT_FRAC`] of the terminal height.
#[must_use]
pub fn compute_popup_rect(area: Rect) -> Rect {
    let popup_width = ((f32::from(area.width) * (1.0 - 2.0 * PICKER_H_PAD_FRAC)).ceil() as u16)
        .max(PICKER_MIN_WIDTH)
        .min(area.width);

    // Layout: border(2) + input(1) + separator(1) + results(N) + footer(1)
    // Reserve at least 4 rows for the chrome, use up to 75% of terminal height.
    let max_body_rows = (f32::from(area.height) * PICKER_MAX_HEIGHT_FRAC).floor() as u16;
    let popup_height = (max_body_rows + 4).min(area.height);

    // Integer division is intentional - we're computing cell positions for centering.
    #[expect(clippy::integer_division, reason = "cell positions are integers")]
    let popup_x = area.width.saturating_sub(popup_width) / 2;
    #[expect(clippy::integer_division, reason = "cell positions are integers")]
    let popup_y = area.height.saturating_sub(popup_height) / 3; // bias toward top third

    Rect::new(popup_x, popup_y, popup_width, popup_height)
}

/// Configuration for rendering the selection popup.
///
/// Generic over any type implementing [`PickerItem`]. Use the builder pattern to
/// customize the title and footer, then call [`render`](SelectionWidget::render).
///
/// # Examples
///
/// See the tests in this crate for usage patterns.
pub struct SelectionWidget<'a, T>
where
    T: PickerItem,
{
    /// Title displayed in the popup border (e.g., `" Model "`).
    title: Line<'a>,
    /// The selection state to render.
    state: &'a SelectionState<T>,
    /// Optional footer lines rendered at the bottom of the popup, top to
    /// bottom. Use [`SelectionWidget::footer`] for a single line or
    /// [`SelectionWidget::footers`] for several.
    footers: Vec<Line<'a>>,
    /// Theme-dependent colors for border, text, separator, etc.
    colors: SelectionColors,
    /// Optional style override for the border title.
    title_style: Option<Style>,
    /// Lines to draw in the results area instead of the filtered items.
    ///
    /// Modals that reuse the picker chrome for something other than a list —
    /// showing authorization instructions above a text input, for example —
    /// set this so the popup keeps the familiar geometry, border, filter line
    /// and footer while presenting their own body.
    body: Option<Vec<Line<'a>>>,
}

impl<'a, T> SelectionWidget<'a, T>
where
    T: PickerItem,
{
    /// Creates a new widget rendering the given selection state.
    pub fn new(state: &'a SelectionState<T>) -> Self {
        Self {
            title: Line::from(""),
            body: None,
            state,
            footers: Vec::new(),
            colors: SelectionColors::default(),
            title_style: None,
        }
    }

    /// Sets the popup border title.
    #[must_use]
    pub fn title(mut self, title: Line<'a>) -> Self {
        self.title = title;
        self
    }

    /// Sets a single footer line rendered at the bottom of the popup.
    /// Replaces any previously set footers. For multiple footer lines use
    /// [`SelectionWidget::footers`].
    #[must_use]
    pub fn footer(mut self, footer: Line<'a>) -> Self {
        self.footers = vec![footer];
        self
    }

    /// Sets multiple footer lines rendered at the bottom of the popup, top
    /// to bottom. Each line occupies one terminal row; the popup allocates
    /// one row per footer line bottom-up (more footers -> fewer result rows).
    /// Replaces any previously set footers.
    #[must_use]
    pub fn footers(mut self, footers: Vec<Line<'a>>) -> Self {
        self.footers = footers;
        self
    }

    /// Sets the theme-dependent colors for the widget.
    #[must_use]
    pub fn colors(mut self, colors: SelectionColors) -> Self {
        self.colors = colors;
        self
    }

    /// Sets an optional style override for the popup border title.
    ///
    /// When set, this is applied to the `Block` via `Block::title_style()`,
    /// allowing the title color to differ from the border color.
    #[must_use]
    pub fn title_style(mut self, style: Style) -> Self {
        self.title_style = Some(style);
        self
    }

    /// Draws `lines` in the results area instead of the filtered item rows.
    #[must_use]
    pub fn body(mut self, lines: Vec<Line<'a>>) -> Self {
        self.body = Some(lines);
        self
    }

    /// Renders the selection popup within the given frame area.
    ///
    /// Computes the popup rectangle, draws the bordered block, filter input,
    /// separator, scrollable result rows, and optional footer.
    /// Sets the cursor position for the filter input.
    pub fn render(self, frame: &mut Frame<'_>, area: Rect) {
        let popup_area = compute_popup_rect(area);

        // Clear the popup area so content behind it doesn't show through.
        frame.render_widget(Clear, popup_area);

        // Bordered block with muted border.
        let block = {
            let mut b = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.colors.border))
                .title(self.title);
            if let Some(style) = self.title_style {
                b = b.title_style(style);
            }
            b
        };
        frame.render_widget(block, popup_area);

        // Layout: input line -> separator -> results -> N footer lines.
        let inner = {
            let b = Block::default().borders(Borders::ALL);
            b.inner(popup_area)
        };
        let footer_rows = self.footers.len() as u16;
        // Split off the footer block (bottom N rows) first, then carve the
        // top region into input / separator / results.
        let [top_area, footer_block] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(footer_rows)]).areas(inner);
        let [input_area, separator_area, results_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(top_area);

        // Filter input with real cursor.
        let filter_text = format!("{}{}", PROMPT, self.state.filter());
        let filter_paragraph =
            Paragraph::new(filter_text).style(Style::default().fg(self.colors.filter_text));
        frame.render_widget(filter_paragraph, input_area);
        let cursor_col = input_area.x + (PROMPT.len() + self.state.cursor_pos()) as u16;
        frame.set_cursor_position((cursor_col, input_area.y));

        // Separator line.
        let separator = "\u{2500}".repeat(separator_area.width as usize);
        let sep_paragraph =
            Paragraph::new(separator).style(Style::default().fg(self.colors.separator));
        frame.render_widget(sep_paragraph, separator_area);

        // Results area - caller-supplied body, or the windowed item list.
        if let Some(body) = self.body {
            frame.render_widget(Paragraph::new(body), results_area);
            render_footers(frame, footer_block, &self.footers, self.colors.footer);
            return;
        }

        let max_visible = results_area.height as usize;
        let scroll_offset = self.state.scroll_offset();
        let selection = self.state.selection();
        let mut result_lines = Vec::with_capacity(max_visible);

        for row in 0..max_visible {
            let entry_idx = scroll_offset + row;
            if let Some(item) = self.state.filtered_item(entry_idx) {
                let is_selected = entry_idx == selection;
                let match_indices = self.state.filtered_match_indices(entry_idx).unwrap_or(&[]);
                let ranges = fuzzy_bytes_to_ranges(match_indices);
                result_lines.push(item.render_row_with_highlight(is_selected, &ranges));
            } else {
                // Empty row to maintain fixed height.
                result_lines.push(Line::from(""));
            }
        }
        frame.render_widget(Paragraph::new(result_lines), results_area);

        // Footers: each line right-aligned in its own row, top to bottom.
        render_footers(frame, footer_block, &self.footers, self.colors.footer);
    }
}

/// Renders each footer line in its own row, top to bottom, right-aligned.
///
/// `footer_block` is the area reserved for all footer rows; it is split
/// vertically into one row per line.
fn render_footers(frame: &mut Frame<'_>, footer_block: Rect, footers: &[Line<'_>], color: Color) {
    use ratatui::style::Style;
    use ratatui::widgets::Paragraph;

    if footers.is_empty() {
        return;
    }

    let rows = Layout::vertical(vec![Constraint::Length(1); footers.len()]).split(footer_block);
    for (line, area) in footers.iter().zip(rows.iter()) {
        let paragraph = Paragraph::new(line.clone())
            .style(Style::default().fg(color))
            .right_aligned();
        frame.render_widget(paragraph, *area);
    }
}
