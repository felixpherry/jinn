//! Platform clipboard access for the TUI.
//!
//! All clipboard writes pass through [`ClipboardService`]. The service keeps
//! platform access off the TUI thread and reports completion through an owned
//! value that callers can forward to the TUI message queue.

use std::sync::Arc;
use std::time::Duration;

use derive_more::Debug;
use error_stack::{Report, ResultExt as _};
use wherror::Error;

/// Error returned when text cannot be written to the platform clipboard.
#[derive(Debug, Error)]
#[error(debug)]
pub struct ClipboardError;

/// Platform boundary used to write text to a system clipboard.
pub trait ClipboardBackend: Send + Sync {
    /// Stable backend name used in diagnostics.
    fn name(&self) -> &'static str;

    /// Writes `text` to the platform clipboard.
    ///
    /// # Errors
    ///
    /// Returns an error when clipboard initialization or the write fails.
    fn set_text(&self, text: &str) -> Result<(), Report<ClipboardError>>;
}

/// Native clipboard backend implemented by `arboard`.
#[derive(Debug, Default)]
pub struct ArboardClipboard;

impl ClipboardBackend for ArboardClipboard {
    fn name(&self) -> &'static str {
        "arboard"
    }

    fn set_text(&self, text: &str) -> Result<(), Report<ClipboardError>> {
        let mut clipboard = arboard::Clipboard::new()
            .change_context(ClipboardError)
            .attach("failed to initialize the platform clipboard")?;
        clipboard
            .set_text(text)
            .change_context(ClipboardError)
            .attach("failed to write text to the platform clipboard")?;

        // X11 owns clipboard data through the open connection. Keep the handle
        // alive briefly so a clipboard manager has time to take ownership.
        std::thread::sleep(Duration::from_secs(2));
        Ok(())
    }
}

/// Owned outcome of one asynchronous clipboard write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardCompletion {
    /// The text was written successfully.
    Success {
        /// Backend that performed the write.
        backend: &'static str,
        /// UTF-8 byte length of the copied text.
        text_len: usize,
    },
    /// The backend could not write the text.
    Failed {
        /// Backend that attempted the write.
        backend: &'static str,
        /// Diagnostic safe to write to the application log.
        error: String,
    },
}

/// Cloneable asynchronous access to a platform clipboard backend.
#[derive(Debug, Clone)]
pub struct ClipboardService {
    #[debug("ClipboardBackend<{}>", backend.name())]
    backend: Arc<dyn ClipboardBackend>,
}

impl ClipboardService {
    /// Creates a service backed by `backend`.
    pub fn new(backend: Arc<dyn ClipboardBackend>) -> Self {
        Self { backend }
    }

    /// Creates the production native clipboard service.
    pub fn native() -> Self {
        Self::new(Arc::new(ArboardClipboard))
    }

    /// Writes `text` on a worker thread and invokes `on_complete` once done.
    ///
    /// The copied text is never included in completion diagnostics.
    pub fn copy<F>(&self, text: String, on_complete: F)
    where
        F: FnOnce(ClipboardCompletion) + Send + 'static,
    {
        let backend = self.backend.clone();
        let backend_name = backend.name();
        let text_len = text.len();
        let worker = std::thread::Builder::new()
            .name("clipboard-copy".to_owned())
            .spawn(move || {
                let completion = match backend.set_text(&text) {
                    Ok(()) => ClipboardCompletion::Success {
                        backend: backend_name,
                        text_len,
                    },
                    Err(error) => ClipboardCompletion::Failed {
                        backend: backend_name,
                        error: format!("{error:?}"),
                    },
                };
                on_complete(completion);
            });

        if let Err(error) = worker {
            // The completion callback moved into the failed spawn request and
            // cannot be recovered. Log this exceptional local failure here;
            // backend failures still return through the normal completion path.
            tracing::warn!(
                backend = backend_name,
                err = %error,
                "failed to start clipboard worker"
            );
        }
    }
}

impl Default for ClipboardService {
    fn default() -> Self {
        Self::native()
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::unwrap_in_result,
        reason = "test code, panics are acceptable"
    )]

    use std::sync::Mutex;

    use super::*;

    #[derive(std::fmt::Debug, Default)]
    struct RecordingClipboard {
        copied: Mutex<Vec<String>>,
    }

    impl ClipboardBackend for RecordingClipboard {
        fn name(&self) -> &'static str {
            "recording"
        }

        fn set_text(&self, text: &str) -> Result<(), Report<ClipboardError>> {
            self.copied
                .lock()
                .expect("recording clipboard lock")
                .push(text.to_owned());
            Ok(())
        }
    }

    #[derive(std::fmt::Debug)]
    struct FailingClipboard;

    impl ClipboardBackend for FailingClipboard {
        fn name(&self) -> &'static str {
            "failing"
        }

        fn set_text(&self, _text: &str) -> Result<(), Report<ClipboardError>> {
            Err(Report::new(ClipboardError).attach("clipboard unavailable"))
        }
    }

    #[rstest::rstest]
    fn copy_delivers_exact_text_to_backend() {
        // Given a clipboard service with a recording backend.
        let backend = Arc::new(RecordingClipboard::default());
        let service = ClipboardService::new(backend.clone());
        let (tx, rx) = std::sync::mpsc::channel();

        // When copying text with whitespace and Unicode intact.
        service.copy(" hello\n世界 ".to_owned(), move |completion| {
            tx.send(completion).expect("completion receiver");
        });
        rx.recv_timeout(Duration::from_secs(1))
            .expect("clipboard completion");

        // Then the backend receives the exact text once.
        assert_eq!(
            *backend.copied.lock().expect("recording clipboard lock"),
            [" hello\n世界 "]
        );
    }

    #[rstest::rstest]
    fn copy_reports_successful_completion() {
        // Given a clipboard service with a successful backend.
        let service = ClipboardService::new(Arc::new(RecordingClipboard::default()));
        let (tx, rx) = std::sync::mpsc::channel();

        // When copying known text.
        service.copy("hello".to_owned(), move |completion| {
            tx.send(completion).expect("completion receiver");
        });
        let completion = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("clipboard completion");

        // Then completion identifies the backend and copied byte length.
        assert_eq!(
            completion,
            ClipboardCompletion::Success {
                backend: "recording",
                text_len: 5,
            }
        );
    }

    #[rstest::rstest]
    fn copy_reports_backend_failure_without_copied_text() {
        // Given a clipboard service whose backend fails.
        let service = ClipboardService::new(Arc::new(FailingClipboard));
        let (tx, rx) = std::sync::mpsc::channel();

        // When copying sensitive text.
        service.copy("secret-marker".to_owned(), move |completion| {
            tx.send(completion).expect("completion receiver");
        });
        let completion = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("clipboard completion");

        // Then completion contains the diagnostic but not copied content.
        let ClipboardCompletion::Failed { backend, error } = completion else {
            panic!("expected failed clipboard completion");
        };
        assert_eq!(backend, "failing");
        assert!(error.contains("clipboard unavailable"));
        assert!(!error.contains("secret-marker"));
    }
}
