//! OpenAI Codex — the subscription-backed model transport.
//!
//! Codex serves ChatGPT subscribers through its own backend, speaking the
//! Responses protocol and authenticating with a subscription access token
//! rather than an API key. That combination is what this module adds; the
//! protocol conversion itself is shared with
//! [`openai_responses`](crate::openai_responses).
//!
//! Requests are billed to the subscription. There is deliberately no fallback
//! to API-key access: a quota or authorization failure fails the request so
//! the billing route can never change without the user asking for it.

//! Protocol reference: earendil-works/pi-mono revision
//! `71dca871bc80b6bc97be37f0ca3189399d651fff`, especially
//! `packages/ai/src/api/openai-codex-responses.ts`,
//! `packages/ai/src/api/openai-responses-shared.ts`, and the Codex catalog in
//! `packages/ai/scripts/generate-models.ts`. This adapter uses the SSE route;
//! dropping its returned stream cancels the HTTP body without background work.

mod factory;
mod models;
mod service;

pub use factory::OpenAiCodexFactory;
pub use models::{CODEX_PROVIDER_NAME, CodexModel, codex_models};
pub use service::{DEFAULT_CODEX_BASE_URL, OpenAiCodexService};
