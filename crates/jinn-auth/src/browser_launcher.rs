//! Opening the authorization page in the user's browser.
//!
//! Launching is a convenience, never a requirement: the authorization URL is
//! always shown in the modal so a failed or unavailable launch still leaves a
//! usable login path.

use std::sync::Arc;

use error_stack::{Report, ResultExt as _};
use parking_lot::Mutex;
use wherror::Error;

/// Raised when the browser could not be launched.
#[derive(Debug, Error)]
#[error("could not open a browser")]
pub struct BrowserLaunchError;

/// Opens URLs in the user's default browser.
pub trait BrowserLauncher: Send + Sync + std::fmt::Debug {
    /// Human-readable name for diagnostics.
    fn name(&self) -> &'static str;

    /// Opens `url`.
    ///
    /// # Errors
    ///
    /// Returns an error if no browser could be launched.
    fn open(&self, url: &str) -> Result<(), Report<BrowserLaunchError>>;
}

/// Shared handle to a [`BrowserLauncher`], following the service-wrapper pattern.
#[derive(Clone, derive_more::Debug)]
pub struct BrowserLauncherService {
    #[debug("BrowserLauncher<{}>", self.launcher.name())]
    launcher: Arc<dyn BrowserLauncher>,
}

impl BrowserLauncherService {
    /// Wraps a launcher for shared ownership.
    #[must_use]
    pub fn new(launcher: Arc<dyn BrowserLauncher>) -> Self {
        Self { launcher }
    }

    /// Opens `url`.
    ///
    /// # Errors
    ///
    /// Returns an error if no browser could be launched.
    pub fn open(&self, url: &str) -> Result<(), Report<BrowserLaunchError>> {
        self.launcher.open(url)
    }
}

/// Launches the platform's default URL handler.
#[derive(Debug, Default)]
pub struct SystemBrowserLauncher;

impl BrowserLauncher for SystemBrowserLauncher {
    fn name(&self) -> &'static str {
        "system"
    }

    fn open(&self, url: &str) -> Result<(), Report<BrowserLaunchError>> {
        let (program, leading_args) = default_opener();
        std::process::Command::new(program)
            .args(leading_args)
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .change_context(BrowserLaunchError)
            .attach(format!("failed to launch {program}"))?;
        Ok(())
    }
}

/// The platform command that opens a URL in the default browser.
const fn default_opener() -> (&'static str, &'static [&'static str]) {
    #[cfg(target_os = "macos")]
    {
        ("open", &[])
    }
    #[cfg(target_os = "windows")]
    {
        ("cmd", &["/C", "start", ""])
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        ("xdg-open", &[])
    }
}

/// A launcher that records requested URLs instead of opening them.
///
/// Used by tests, and by environments where launching a browser is undesirable.
#[derive(Debug, Default)]
pub struct RecordingBrowserLauncher {
    opened: Mutex<Vec<String>>,
    fails: bool,
}

impl RecordingBrowserLauncher {
    /// Creates a launcher that records successes.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a launcher that always fails, as a machine with no browser would.
    #[must_use]
    pub fn failing() -> Self {
        Self {
            opened: Mutex::new(Vec::new()),
            fails: true,
        }
    }

    /// URLs this launcher was asked to open, in order.
    #[must_use]
    pub fn opened(&self) -> Vec<String> {
        self.opened.lock().clone()
    }
}

impl BrowserLauncher for RecordingBrowserLauncher {
    fn name(&self) -> &'static str {
        "recording"
    }

    fn open(&self, url: &str) -> Result<(), Report<BrowserLaunchError>> {
        self.opened.lock().push(url.to_owned());
        if self.fails {
            return Err(Report::new(BrowserLaunchError).attach("no browser available"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;

    #[rstest::rstest]
    fn recording_launcher_remembers_the_requested_url() {
        // Given a recording launcher.
        let launcher = RecordingBrowserLauncher::new();

        // When asked to open a URL.
        launcher
            .open("https://example.test/authorize")
            .expect("open");

        // Then the URL is recorded.
        assert_eq!(launcher.opened(), vec!["https://example.test/authorize"]);
    }

    #[rstest::rstest]
    fn a_failing_launcher_reports_the_failure() {
        // Given a launcher on a machine with no browser.
        let launcher = RecordingBrowserLauncher::failing();

        // When asked to open a URL.
        let result = launcher.open("https://example.test/authorize");

        // Then the failure is reported to the caller.
        assert!(result.is_err(), "launch failure must be visible");
    }
}
