//! Authentication commands.
//!
//! These carry user decisions from the synchronous intent layer to the auth
//! actor, which owns every external and asynchronous part of authentication.

use jinn_auth::{AuthProviderId, LoginMethod};
use serde::{Deserialize, Serialize};

use crate::common::bus::BusMessage;

/// Load the providers the login picker should list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadLoginPickerEntries;
impl BusMessage for LoadLoginPickerEntries {}

/// Load the providers the logout picker should list.
///
/// Only providers with stored credentials appear.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadLogoutPickerEntries;
impl BusMessage for LoadLogoutPickerEntries {}

/// Begin authenticating `provider` using `method`.
///
/// Replaces any attempt already running; the previous one is cancelled and its
/// result ignored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartLogin {
    /// The provider to authenticate against.
    pub provider: AuthProviderId,
    /// The login method the user chose.
    pub method: LoginMethod,
}
impl BusMessage for StartLogin {}

/// Abandon the active attempt.
///
/// Stops its callback listener and its polling, and prevents a late result
/// from changing stored credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelLogin;
impl BusMessage for CancelLogin {}

/// Hand the active attempt an authorization code the user pasted.
///
/// Accepts a bare code, a `code#state` pair, a query fragment, or the whole
/// redirect URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitAuthorizationCode {
    /// What the user pasted.
    pub input: String,
}
impl BusMessage for SubmitAuthorizationCode {}

/// Remove `provider`'s stored credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Logout {
    /// The provider to log out of.
    pub provider: AuthProviderId,
}
impl BusMessage for Logout {}
