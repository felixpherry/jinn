//! OpenAI Responses protocol — message conversion and stream normalization.
//!
//! The Responses protocol models a turn as a list of typed input items rather
//! than a flat chat transcript, and streams its output as a sequence of
//! semantic events. This module translates between that shape and jinn's
//! provider-neutral [`LlmMessage`](crate::LlmMessage) and
//! [`StreamEvent`](crate::StreamEvent) types.
//!
//! It is deliberately free of any authentication or transport concern, so any
//! backend speaking the Responses protocol — a subscription-backed one today,
//! an API-key one later — can share it.

pub mod request;
pub mod stream;

pub use request::{build_input_items, build_tools};
pub use stream::ResponsesStreamParser;
