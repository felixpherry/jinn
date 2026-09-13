//! Clipboard flush - extracts selected text from the buffer and copies to system clipboard.

use ratatui::buffer::Buffer;

use crate::TuiApp;
use crate::selection::SelectionState;

/// If a clipboard copy is pending, extracts the selected text from the buffer
/// and copies it to the system clipboard. Clears the pending flag regardless
/// of success or failure.
///
/// The shared clipboard service performs the platform write off-thread and
/// keeps the native clipboard handle alive long enough for X11 clipboard
/// managers to take ownership.
pub(super) fn flush_pending_clipboard(app: &mut TuiApp, buf: &Buffer) {
    if !app.pending_clipboard {
        return;
    }
    app.pending_clipboard = false;

    let text = match app.selection.extract_text(buf) {
        Some(text) if !text.is_empty() => text,
        _ => {
            // Empty selection - clear highlight silently.
            app.selection = SelectionState::Idle;
            return;
        }
    };

    // Clear selection highlight immediately.
    app.selection = SelectionState::Idle;

    let sender = app.events.sender();
    app.clipboard.copy(text, move |completion| {
        sender.send(crate::msg::Msg::Clipboard(completion));
    });
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::indexing_slicing,
        reason = "test code, panics are acceptable"
    )]
    use std::sync::Arc;
    use std::time::Duration;

    use error_stack::Report;

    use super::*;
    use crate::clipboard::{ClipboardBackend, ClipboardError, ClipboardService};
    use crate::selection::SelectionState;
    use ratatui::layout::Rect;

    /// Creates a minimal `TuiApp` for render testing.
    async fn render_test_app() -> crate::TuiApp {
        crate::TuiApp::test_builder().build().await
    }

    #[derive(Debug)]
    struct ChannelClipboard {
        copied: std::sync::mpsc::Sender<String>,
    }

    impl ClipboardBackend for ChannelClipboard {
        fn name(&self) -> &'static str {
            "channel"
        }

        fn set_text(&self, text: &str) -> Result<(), Report<ClipboardError>> {
            self.copied
                .send(text.to_owned())
                .map_err(|_send_error| Report::new(ClipboardError))
        }
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn clipboard_copy_clears_pending_flag_on_idle_selection() {
        // Given an app with pending_clipboard set but Idle selection.
        let mut app = render_test_app().await;
        app.selection = SelectionState::Idle;
        app.pending_clipboard = true;

        let area = Rect::new(0, 0, 20, 5);
        let buf = ratatui::buffer::Buffer::empty(area);

        // When flushing the pending clipboard.
        flush_pending_clipboard(&mut app, &buf);

        // Then the pending flag is cleared (even though there was nothing to copy).
        assert!(!app.pending_clipboard);
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn clipboard_copy_skips_empty_selection() {
        // Given an app with pending_clipboard and an Active selection over empty cells.
        let area = Rect::new(0, 0, 20, 5);
        let buf = ratatui::buffer::Buffer::empty(area);

        let mut app = render_test_app().await;
        app.selection = SelectionState::Active {
            anchor: (0, 0),
            focus: (3, 0),
            bounds: area,
        };
        app.pending_clipboard = true;

        // When flushing the pending clipboard.
        flush_pending_clipboard(&mut app, &buf);

        // Then the pending flag is cleared.
        assert!(!app.pending_clipboard);
        // And the selection is cleared to Idle (no highlight persists).
        assert_eq!(app.selection, SelectionState::Idle);
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn clipboard_clears_pending_flag_immediately() {
        // Given a buffer with known text and an active selection.
        let area = Rect::new(0, 0, 20, 5);
        let mut buf = ratatui::buffer::Buffer::empty(area);
        // Write "Hello" on row 2.
        for (i, ch) in "Hello".chars().enumerate() {
            buf.cell_mut((2 + i as u16, 2))
                .unwrap()
                .set_symbol(&ch.to_string());
        }

        let mut app = render_test_app().await;
        app.selection = SelectionState::Active {
            anchor: (2, 2),
            focus: (6, 2),
            bounds: area,
        };
        app.pending_clipboard = true;

        // When flushing the pending clipboard.
        flush_pending_clipboard(&mut app, &buf);

        // Then the pending flag is cleared immediately.
        assert!(!app.pending_clipboard);
        // And the selection is cleared to Idle.
        assert_eq!(app.selection, SelectionState::Idle);
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn clipboard_copy_uses_shared_backend_for_selected_text() {
        // Given an app with selected buffer text and a recording clipboard backend.
        let area = Rect::new(0, 0, 20, 5);
        let mut buf = ratatui::buffer::Buffer::empty(area);
        for (i, ch) in "Hello".chars().enumerate() {
            buf.cell_mut((2 + i as u16, 2))
                .unwrap()
                .set_symbol(&ch.to_string());
        }
        let (copied, receiver) = std::sync::mpsc::channel();
        let clipboard = ClipboardService::new(Arc::new(ChannelClipboard { copied }));
        let mut app = crate::TuiApp::test_builder()
            .clipboard(clipboard)
            .build()
            .await;
        app.selection = SelectionState::Active {
            anchor: (2, 2),
            focus: (6, 2),
            bounds: area,
        };
        app.pending_clipboard = true;

        // When flushing the pending clipboard.
        flush_pending_clipboard(&mut app, &buf);

        // Then the shared backend receives the selected text.
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("clipboard write"),
            "Hello"
        );
    }

    #[rstest::rstest]
    #[ignore = "requires clipboard access (run with --ignored)"]
    #[tokio::test]
    async fn clipboard_contains_selected_text() {
        // Given a buffer with known text and an active selection.
        let area = Rect::new(0, 0, 20, 5);
        let mut buf = ratatui::buffer::Buffer::empty(area);
        // Write "Hello" on row 2.
        for (i, ch) in "Hello".chars().enumerate() {
            buf.cell_mut((2 + i as u16, 2))
                .unwrap()
                .set_symbol(&ch.to_string());
        }

        let mut app = render_test_app().await;
        app.selection = SelectionState::Active {
            anchor: (2, 2),
            focus: (6, 2),
            bounds: area,
        };
        app.pending_clipboard = true;

        // When flushing the pending clipboard.
        flush_pending_clipboard(&mut app, &buf);

        // Then after the clipboard thread completes, the clipboard contains
        // the selected text.
        std::thread::sleep(std::time::Duration::from_millis(500));
        let mut clipboard = arboard::Clipboard::new().expect("clipboard access");
        let content = clipboard.get_text().expect("read clipboard");
        assert_eq!(content, "Hello");
    }
}
