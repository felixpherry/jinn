//! Authentication events.
//!
//! Events describe what happened to an attempt. They never carry
//! authorization codes or tokens: an authorization URL and a device code are
//! meant to be shown to the user, everything else is not.

use jinn_auth::{AuthProviderId, LoginMethod};
use serde::{Deserialize, Serialize};

use crate::common::bus::BusMessage;
use crate::feat::auth::state::AuthPhase;

/// The active attempt reported progress.
///
/// `attempt` identifies which attempt reported it, so a report from a
/// cancelled or superseded attempt can be discarded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthProgressReported {
    /// Which attempt this report belongs to.
    pub attempt: u64,
    /// What the modal should now show.
    pub phase: AuthPhase,
}
impl BusMessage for AuthProgressReported {}

/// An attempt finished, successfully or not.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginAttemptFinished {
    /// Which attempt finished.
    pub attempt: u64,
    /// The provider that was being authenticated.
    pub provider: AuthProviderId,
    /// The method that was used.
    pub method: LoginMethod,
    /// `None` on success; the reason to show the user on failure.
    pub failure: Option<String>,
}
impl BusMessage for LoginAttemptFinished {}

/// Stored credentials changed: a provider was logged in or out.
///
/// Subscribers refresh whatever depends on which accounts are configured.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionCredentialsChanged {
    /// The provider whose credentials changed.
    pub provider: AuthProviderId,
    /// Whether credentials are now stored for it.
    pub is_configured: bool,
}
impl BusMessage for SubscriptionCredentialsChanged {}
