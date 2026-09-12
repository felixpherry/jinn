//! Authentication — signing in with a subscription you already pay for.
//!
//! Some providers let an existing consumer subscription authorize model
//! requests. This slice is the user-facing half of that: the login and logout
//! pickers, the modal that carries an authorization through to completion, and
//! the actor that owns stored credentials.
//!
//! `<leader>l` opens the login picker, `<leader>L` the logout picker. Both
//! reuse the ordinary Telescope-style picker, so navigation, filtering, and
//! styling are the same as everywhere else in jinn.
//!
//! Logging in does not change the selected model or the conversation. It makes
//! subscription models available and says so in the chat area; choosing one
//! stays the user's decision.

pub mod auth_actor;
pub mod intent;
pub mod interaction;
pub mod picker_entry;
pub mod picker_render;
pub mod protocol;
pub mod state;
pub mod validator;

pub use picker_entry::{AuthMethodEntry, AuthProviderEntry};
pub use state::{AuthFlow, AuthPhase, AuthState};
