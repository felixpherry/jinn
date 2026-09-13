//! Message channel for the TUI event loop.
//!
//! Provides a unified message type that merges crossterm terminal events,
//! periodic tick messages, and commands into a single stream.

pub mod handler;
pub mod sender;

pub use sender::MsgSender;

/// A unified message from any source.
///
/// Merges crossterm terminal events, periodic tick messages,
/// and commands (from key handling or actors) into a single stream
/// consumed by the main event loop.
pub enum Msg {
    /// Periodic tick for render refresh.
    Tick,
    /// A crossterm terminal event (key press, resize, etc.).
    Input(crossterm::event::Event),
    /// Completion of an asynchronous platform clipboard write.
    Clipboard(crate::clipboard::ClipboardCompletion),
}

impl std::fmt::Debug for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tick => f.write_str("Tick"),
            Self::Input(e) => f.debug_tuple("Input").field(e).finish(),
            Self::Clipboard(completion) => f.debug_tuple("Clipboard").field(completion).finish(),
        }
    }
}
