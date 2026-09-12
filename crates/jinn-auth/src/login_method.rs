//! The ways a user can complete a subscription login.
//!
//! Providers advertise the subset they support, so the shared login UI never
//! assumes a provider implements every method.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A way of completing a login.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginMethod {
    /// Authorize in a web browser on this machine; jinn receives the result on
    /// a local callback, or accepts a pasted authorization code.
    Browser,
    /// Authorize from any browser using a short code; jinn polls for
    /// completion. Suited to headless and remote sessions.
    DeviceCode,
}

impl LoginMethod {
    /// Stable wire id.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::DeviceCode => "device_code",
        }
    }

    /// Row label shown in the login-method picker.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Browser => "Browser login",
            Self::DeviceCode => "Device code login",
        }
    }

    /// One-line explanation of when to pick this method.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Browser => "opens the authorization page on this machine",
            Self::DeviceCode => "enter a code from another browser or device",
        }
    }
}

impl fmt::Display for LoginMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;

    #[rstest::rstest]
    #[case(LoginMethod::Browser, "browser")]
    #[case(LoginMethod::DeviceCode, "device_code")]
    fn wire_id_is_stable(#[case] method: LoginMethod, #[case] expected: &str) {
        assert_eq!(method.as_str(), expected);
    }
}
