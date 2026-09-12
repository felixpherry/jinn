//! OpenAI Codex — subscription authentication against a ChatGPT account.
//!
//! This is the first (and currently only) subscription provider adapter. It
//! supplies the provider-specific pieces — endpoints, authorization
//! parameters, device-code semantics, refresh — while the shared
//! infrastructure owns storage, the picker flow, and cancellation.
//!
//! Protocol reference verified against `earendil-works/pi-mono` HEAD
//! `71dca871bc80b6bc97be37f0ca3189399d651fff` during implementation:
//! `packages/ai/src/auth/oauth/openai-codex.ts`, `auth/oauth/device-code.ts`,
//! `auth/resolve.ts`, and `packages/coding-agent/src/core/auth-storage.ts`.
//! Browser parameters, device endpoints (including pending HTTP 403/404),
//! token/account extraction, five-minute refresh window and fifteen-second
//! refresh timeout follow that revision. Storage holds a cross-process lock
//! through refresh and rotation, rechecking the credential after acquisition.
//! Unlike upstream's verbose response diagnostics, jinn never includes token
//! response bodies or deserialization values in errors.

pub mod account;
pub mod authorization_input;
pub mod endpoints;
pub mod oauth;

pub use endpoints::CodexAuthEndpoints;
pub use oauth::OpenAiCodexAuth;
