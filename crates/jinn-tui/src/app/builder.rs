//! Builder for constructing a [`TuiApp`] with sensible defaults for tests.

use jinn_domain::AppCore;

use super::TuiApp;
use crate::clipboard::ClipboardService;

/// Builder for constructing a [`TuiApp`] with sensible defaults for tests.
///
/// All fields default to fake/noop implementations. Override only what the test needs.
///
/// See the tests in this crate for usage patterns.
#[derive(Default)]
pub struct TuiAppBuilder {
    /// Optional services override (defaults to fake services).
    services: Option<jinn_domain::Services>,
    /// Optional app state override (defaults to default state).
    state: Option<jinn_domain::AppState>,
    /// Optional clipboard service override (defaults to the native backend).
    clipboard: Option<ClipboardService>,
}

impl TuiAppBuilder {
    /// Override the default services.
    #[must_use]
    pub fn services(mut self, services: jinn_domain::Services) -> Self {
        self.services = Some(services);
        self
    }

    /// Override the default app state.
    #[must_use]
    pub fn state(mut self, state: jinn_domain::AppState) -> Self {
        self.state = Some(state);
        self
    }

    /// Override the clipboard service.
    #[must_use]
    pub fn clipboard(mut self, clipboard: ClipboardService) -> Self {
        self.clipboard = Some(clipboard);
        self
    }

    /// Build the `TuiApp` with the configured overrides.
    ///
    /// Delegates to [`crate::launch::launch_for_test`] so that the test path and
    /// the real launch path ([`crate::launch::launch`]) share a single keymap
    /// bootstrap site. This is what prevents test/prod divergence in keymap binding.
    pub async fn build(self) -> TuiApp {
        let services = match self.services {
            Some(s) => s,
            None => jinn_domain::Services::new_fake().await,
        };
        let state = self.state.unwrap_or_default();

        let core = AppCore {
            state: jinn_domain::State::new(state),
            bridge: services.bridge.clone(),
        };

        crate::launch::launch_for_test(core, services, self.clipboard.unwrap_or_default())
    }
}
