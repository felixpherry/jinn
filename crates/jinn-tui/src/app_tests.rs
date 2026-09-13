#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::needless_lifetimes,
    reason = "test file, panics are acceptable"
)]
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use error_stack::Report;
use ratatui::layout::Rect;

use crate::TuiApp;
use crate::app::{WhichKeyInstance, scope_for_focus};
use crate::clipboard::{ClipboardBackend, ClipboardCompletion, ClipboardError, ClipboardService};
use crate::config::TuiConfig;
use crate::keymap;
use crate::msg::Msg;
use crate::scope::Scope;
use crate::selection::SelectionState;

/// Creates a minimal `TuiApp` for testing.
async fn test_app() -> TuiApp {
    TuiApp::test_builder().build().await
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

async fn app_recording_clipboard() -> (TuiApp, std::sync::mpsc::Receiver<String>) {
    let (copied, receiver) = std::sync::mpsc::channel();
    let clipboard = ClipboardService::new(Arc::new(ChannelClipboard { copied }));
    let app = TuiApp::test_builder().clipboard(clipboard).build().await;
    (app, receiver)
}

#[rstest::rstest]
#[tokio::test]
async fn clipboard_success_preserves_existing_status_hint() {
    // Given an app with a domain-specific clipboard success hint.
    let mut app = test_app().await;
    app.core.state.write_test_no_cap().frontend.status_hint =
        Some("yanked 3 terminal lines to the clipboard".to_owned());

    // When handling a successful clipboard completion.
    app.handle_msg(Msg::Clipboard(ClipboardCompletion::Success {
        backend: "test",
        text_len: 12,
    }));

    // Then the existing status hint remains unchanged.
    assert_eq!(
        app.core.state.read().frontend.status_hint.as_deref(),
        Some("yanked 3 terminal lines to the clipboard")
    );
}

#[rstest::rstest]
#[tokio::test]
async fn clipboard_failure_sets_visible_status_hint() {
    // Given an app without a status hint.
    let mut app = test_app().await;

    // When handling a failed clipboard completion.
    app.handle_msg(Msg::Clipboard(ClipboardCompletion::Failed {
        backend: "test",
        error: "clipboard unavailable".to_owned(),
    }));

    // Then the app exposes a concise failure hint.
    assert_eq!(
        app.core.state.read().frontend.status_hint.as_deref(),
        Some("failed to copy to clipboard; see log for details")
    );
}

#[rstest::rstest]
#[tokio::test]
async fn clipboard_failure_replaces_speculative_success_hint() {
    // Given an app with a success hint staged before the platform write.
    let mut app = test_app().await;
    app.core.state.write_test_no_cap().frontend.status_hint =
        Some("yanked 3 terminal lines to the clipboard".to_owned());

    // When the platform clipboard write fails.
    app.handle_msg(Msg::Clipboard(ClipboardCompletion::Failed {
        backend: "test",
        error: "clipboard unavailable".to_owned(),
    }));

    // Then failure feedback takes precedence over speculative success.
    assert_eq!(
        app.core.state.read().frontend.status_hint.as_deref(),
        Some("failed to copy to clipboard; see log for details")
    );
}

#[rstest::rstest]
#[tokio::test]
async fn clipboard_worker_completion_arrives_on_tui_message_queue() {
    // Given an app with a successful recording clipboard backend.
    let (app, _copied) = app_recording_clipboard().await;
    let sender = app.events.sender();

    // When dispatching a clipboard write through the shared service.
    app.clipboard
        .copy("message marker".to_owned(), move |completion| {
            sender.send(Msg::Clipboard(completion));
        });
    let message = app.events.recv().expect("clipboard completion message");

    // Then its result arrives as a clipboard message for the TUI thread.
    assert!(matches!(
        message,
        Msg::Clipboard(ClipboardCompletion::Success {
            backend: "channel",
            text_len: 14,
        })
    ));
}

fn install_terminal_screen(app: &TuiApp, screen: &str) {
    let mut state = app.core.state.write_test_no_cap();
    let session_id = state.session.active_session_id().clone();
    state
        .frontend
        .scope_stack
        .push(jinn_domain::FocusScope::TerminalView);
    state.frontend.terminal.apply_screen(
        &session_id,
        "term-test",
        screen.to_owned(),
        jinn_domain::feat::interactive_term::emulator::ScreenCells::default(),
        (0, 0),
        false,
    );
}

#[rstest::rstest]
#[tokio::test]
async fn chat_entry_yank_uses_shared_clipboard_backend() {
    // Given an app with a selected entry and recording clipboard backend.
    let (mut app, copied) = app_recording_clipboard().await;
    {
        let mut state = app.core.state.write_test_no_cap();
        state
            .active_session_mut()
            .push_entry(jinn_domain::ChatEntry::user("chat yank marker"));
        state.active_session_mut().select_next_entry();
    }

    // When yanking the selected entry.
    app.route_intent(jinn_domain::Intent::YankSelectedEntry);

    // Then the shared backend receives its exact yank text.
    assert_eq!(
        copied
            .recv_timeout(Duration::from_secs(1))
            .expect("clipboard write"),
        "chat yank marker"
    );
}

#[rstest::rstest]
#[tokio::test]
async fn terminal_yank_uses_shared_clipboard_backend() {
    // Given an app viewing a mirrored terminal screen.
    let (mut app, copied) = app_recording_clipboard().await;
    install_terminal_screen(&app, "terminal yank marker");

    // When yanking the terminal screen.
    app.route_intent(jinn_domain::Intent::TerminalYank);

    // Then the shared backend receives the exact screen text.
    assert_eq!(
        copied
            .recv_timeout(Duration::from_secs(1))
            .expect("clipboard write"),
        "terminal yank marker"
    );
}

#[rstest::rstest]
#[tokio::test]
async fn terminal_yank_and_push_uses_shared_clipboard_backend() {
    // Given an app viewing a mirrored terminal screen.
    let (mut app, copied) = app_recording_clipboard().await;
    install_terminal_screen(&app, "terminal push marker");

    // When yanking and pushing the screen.
    app.route_intent(jinn_domain::Intent::TerminalPushScreen);

    // Then the shared backend receives the exact screen text.
    assert_eq!(
        copied
            .recv_timeout(Duration::from_secs(1))
            .expect("clipboard write"),
        "terminal push marker"
    );
}

#[rstest::rstest]
#[case::normal_chat(jinn_domain::FocusScope::Normal, Scope::Normal)]
#[case::sidebar(jinn_domain::FocusScope::SidebarPersona, Scope::SidebarPersona)]
#[case::input(jinn_domain::FocusScope::Input, Scope::Input)]
#[case::picker_provider(jinn_domain::FocusScope::Picker { kind: jinn_domain::PickerKind::Provider }, Scope::PickerProvider)]
#[case::sidebar_resize(jinn_domain::FocusScope::SidebarResize, Scope::SidebarResize)]
#[case::picker_compaction_model(jinn_domain::FocusScope::Picker { kind: jinn_domain::PickerKind::CompactionModel }, Scope::PickerCompactionModel)]
#[case::picker_task_list(jinn_domain::FocusScope::Picker { kind: jinn_domain::PickerKind::TaskList }, Scope::PickerTaskList)]
fn scope_for_focus_maps_correctly(#[case] focus: jinn_domain::FocusScope, #[case] expected: Scope) {
    // Given a focus scope.
    // When mapping to a keymap scope.
    // Then the expected scope is returned.
    assert_eq!(scope_for_focus(&focus), expected);
}

#[rstest::rstest]
#[tokio::test]
async fn mouse_down_left_in_selectable_rect_starts_dragging() {
    // Given an app with a registered selectable rect.
    let mut app = test_app().await;
    let rect = Rect::new(5, 5, 20, 10);
    app.selectable_rects.rebuild(vec![rect]);

    // When sending a left-click inside the rect.
    let mouse = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: 8,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    app.handle_msg(Msg::Input(crossterm::event::Event::Mouse(mouse)));

    // Then the selection is Dragging with anchor at (10, 8).
    assert_eq!(
        app.selection,
        SelectionState::Dragging {
            anchor: (10, 8),
            focus: (10, 8),
            bounds: rect,
        }
    );
}

#[rstest::rstest]
#[tokio::test]
async fn mouse_down_left_outside_selectable_rect_does_not_start_dragging() {
    // Given an app with a registered selectable rect.
    let mut app = test_app().await;
    app.selectable_rects.rebuild(vec![Rect::new(5, 5, 10, 10)]);

    // When sending a left-click outside the rect.
    let mouse = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 30,
        row: 30,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    app.handle_msg(Msg::Input(crossterm::event::Event::Mouse(mouse)));

    // Then the selection remains Idle.
    assert_eq!(app.selection, SelectionState::Idle);
}

#[rstest::rstest]
#[tokio::test]
async fn mouse_drag_updates_focus_while_dragging() {
    // Given an app with an active drag.
    let mut app = test_app().await;
    let rect = Rect::new(0, 0, 40, 24);
    app.selectable_rects.rebuild(vec![rect]);
    app.selection = SelectionState::start_drag(5, 5, rect);

    // When sending a drag event.
    let mouse = MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 15,
        row: 10,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    app.handle_msg(Msg::Input(crossterm::event::Event::Mouse(mouse)));

    // Then the focus is updated to (15, 10).
    assert_eq!(
        app.selection,
        SelectionState::Dragging {
            anchor: (5, 5),
            focus: (15, 10),
            bounds: rect,
        }
    );
}

#[rstest::rstest]
#[tokio::test]
async fn mouse_up_left_finalizes_selection() {
    // Given an app with an active drag.
    let mut app = test_app().await;
    let rect = Rect::new(0, 0, 40, 24);
    app.selection = SelectionState::start_drag(2, 3, rect).update_focus(10, 12);

    // When sending a mouse-up event.
    let mouse = MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 10,
        row: 12,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    app.handle_msg(Msg::Input(crossterm::event::Event::Mouse(mouse)));

    // Then the selection is Active with the same anchor and focus.
    assert_eq!(
        app.selection,
        SelectionState::Active {
            anchor: (2, 3),
            focus: (10, 12),
            bounds: rect,
        }
    );
}

#[rstest::rstest]
#[tokio::test]
async fn mouse_down_right_cancels_selection() {
    // Given an app with an active selection.
    let mut app = test_app().await;
    let rect = Rect::new(0, 0, 40, 24);
    app.selection = SelectionState::start_drag(5, 5, rect);

    // When sending a right-click.
    let mouse = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column: 5,
        row: 5,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    app.handle_msg(Msg::Input(crossterm::event::Event::Mouse(mouse)));

    // Then the selection is cancelled to Idle.
    assert_eq!(app.selection, SelectionState::Idle);
}

#[rstest::rstest]
#[tokio::test]
async fn scroll_events_still_route_to_keymap() {
    // Given an app in Normal scope.
    let mut app = test_app().await;
    let initial_selection = app.selection.clone();

    // When sending a scroll-up mouse event.
    let mouse = MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 10,
        row: 10,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    app.handle_msg(Msg::Input(crossterm::event::Event::Mouse(mouse)));

    // Then the selection is unchanged (event fell through to keymap).
    assert_eq!(app.selection, initial_selection);
}

#[rstest::rstest]
#[tokio::test]
async fn mouse_events_not_handled_when_mouse_selection_disabled() {
    // Given an app with mouse selection disabled and a registered selectable rect.
    let mut app = test_app().await;
    app.config = TuiConfig::new(false);
    let rect = Rect::new(5, 5, 20, 10);
    app.selectable_rects.rebuild(vec![rect]);

    // When sending a left-click inside the rect.
    let mouse = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: 8,
        modifiers: crossterm::event::KeyModifiers::NONE,
    };
    app.handle_msg(Msg::Input(crossterm::event::Event::Mouse(mouse)));

    // Then the selection remains Idle (event was not handled).
    assert_eq!(app.selection, SelectionState::Idle);
}

// -----------------------------------------------------------------------------
// Keymap tests for the task-list zoom picker (sidebar `s` binding)
// -----------------------------------------------------------------------------
//
// These tests use a bare `WhichKeyInstance` rather than a full `TuiApp` because
// they only verify that the keymap resolves the right `Intent`. They don't need
// the actor host, sidebar, or selection state.

fn keymap_at(scope: Scope) -> WhichKeyInstance {
    WhichKeyInstance::new(keymap::init(), scope)
}

fn key<'a>(notation: &'a str) -> jinn_domain::KeyEvent {
    jinn_domain::KeyEvent::parse_notation(notation).expect("notation should parse")
}

#[rstest::rstest]
fn s_in_sidebar_task_list_opens_task_list_picker() {
    // Given the keymap rooted at SidebarTaskList.
    let mut wk = keymap_at(Scope::SidebarTaskList);

    // When pressing `s`.
    let intent = wk.handle_key(key("s"));

    // Then it resolves to OpenPicker { kind: TaskList } ("search task list").
    assert_eq!(
        intent.map(|i| i.to_string()).as_deref(),
        Some("search task list")
    );
}

#[rstest::rstest]
#[case::normal(Scope::Normal)]
#[case::input(Scope::Input)]
#[case::sidebar_sessions(Scope::SidebarSessions)]
#[case::picker_session(Scope::PickerSession)]
fn s_outside_sidebar_task_list_does_not_open_task_list_picker(#[case] scope: Scope) {
    // Given the keymap rooted at a non-SidebarTaskList scope.
    let mut wk = keymap_at(scope);

    // When pressing `s`.
    let intent = wk.handle_key(key("s"));

    // Then it does NOT resolve to the TaskList open intent. It may resolve to
    // some other intent (e.g. Input's catch-all `InsertChar('s')`) or None,
    // but never to "search task list".
    assert_ne!(
        intent.map(|i| i.to_string()).as_deref(),
        Some("search task list")
    );
}

#[rstest::rstest]
fn esc_in_picker_task_list_returns_to_normal_mode() {
    // Given the keymap rooted at PickerTaskList.
    let mut wk = keymap_at(Scope::PickerTaskList);

    // When pressing `<esc>`.
    let intent = wk.handle_key(key("escape"));

    // Then it resolves to EnterNormalMode (the existing handler closes the picker
    // and restores the prior SidebarTaskList scope).
    assert_eq!(
        intent.map(|i| i.to_string()).as_deref(),
        Some("enter normal mode")
    );
}

#[rstest::rstest]
#[test]
fn alt_q_in_input_scope_toggles_input_mode() {
    // Given the keymap rooted at Input scope.
    let mut wk = keymap_at(Scope::Input);

    // When pressing Alt+q (notation: `m-q`).
    let intent = wk.handle_key(key("m-q"));

    // Then it resolves to ToggleInputMode (Queue ↔ Steer).
    assert_eq!(
        intent.map(|i| i.to_string()).as_deref(),
        Some("toggle input mode")
    );
}

#[rstest::rstest]
#[test]
fn alt_s_in_input_scope_focuses_sidebar_sessions() {
    // Given the keymap rooted at Input scope.
    let mut wk = keymap_at(Scope::Input);

    // When pressing Alt+s (notation: `m-s`).
    let intent = wk.handle_key(key("m-s"));

    // Then it resolves to SidebarFocusSessions (now bound in Input scope too).
    assert_eq!(
        intent.map(|i| i.to_string()).as_deref(),
        Some("focus session list")
    );
}

#[rstest::rstest]
#[test]
fn sessions_i_resolves_to_sidebar_confirm_insert() {
    // Given the keymap rooted at the Sessions sidebar.
    let mut wk = keymap_at(Scope::SidebarSessions);

    // When pressing `i`.
    let intent = wk.handle_key(key("i"));

    // Then it resolves to SidebarConfirmInsert (activate + insert).
    assert_eq!(
        intent.map(|i| i.to_string()).as_deref(),
        Some("activate session -> insert mode")
    );
}

#[rstest::rstest]
#[test]
fn sessions_enter_still_resolves_to_sidebar_confirm() {
    // Given the keymap rooted at the Sessions sidebar.
    let mut wk = keymap_at(Scope::SidebarSessions);

    // When pressing `<enter>`.
    let intent = wk.handle_key(key("enter"));

    // Then it still resolves to SidebarConfirm (activate + normal).
    assert_eq!(
        intent.map(|i| i.to_string()).as_deref(),
        Some("activate session")
    );
}

#[rstest::rstest]
#[test]
fn pins_enter_resolves_to_sidebar_leave() {
    // Given the keymap rooted at the Pins sidebar.
    let mut wk = keymap_at(Scope::SidebarPins);

    // When pressing `<enter>`.
    let intent = wk.handle_key(key("enter"));

    // Then it resolves to SidebarLeave (leave to Normal at the pin's position).
    assert_eq!(
        intent.map(|i| i.to_string()).as_deref(),
        Some("return to normal mode")
    );
}

#[rstest::rstest]
#[test]
fn alt_s_in_normal_scope_focuses_sidebar_sessions() {
    // Given the keymap rooted at Normal scope.
    let mut wk = keymap_at(Scope::Normal);

    // When pressing Alt+s (notation: `m-s`).
    let intent = wk.handle_key(key("m-s"));

    // Then it resolves to SidebarFocusSessions (scope-aware binding).
    assert_eq!(
        intent.map(|i| i.to_string()).as_deref(),
        Some("focus session list")
    );
}

#[rstest::rstest]
#[test]
fn r_in_normal_scope_resets_entry_to_default_context() {
    // Given the keymap rooted at Normal scope.
    let mut wk = keymap_at(Scope::Normal);

    // When pressing `r`.
    let intent = wk.handle_key(key("r"));

    // Then it resolves to ChatEntryResetSelected.
    assert_eq!(
        intent.map(|i| i.to_string()).as_deref(),
        Some("reset entry to default context")
    );
}
