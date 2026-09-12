//! Normalizing a Responses protocol event stream into jinn stream events.
//!
//! The protocol reports progress as semantic events keyed by the index of the
//! output item they belong to. The parser tracks the in-flight tool calls so a
//! finished call can be emitted complete, and remembers whether the turn ended
//! in tool use so the terminal event reports the right stop reason.

use std::collections::BTreeMap;

use crate::stream_event::{StopReason, StreamEvent, StreamUsage};
use crate::tool_types::ToolCall;

/// Parses Responses protocol events into [`StreamEvent`]s.
#[derive(Debug, Default)]
pub struct ResponsesStreamParser {
    /// Tool calls being streamed, keyed by their output index.
    pending_calls: BTreeMap<usize, PendingToolCall>,
    /// Whether any tool call completed during this response.
    saw_tool_call: bool,
    /// Whether a terminal response event has been seen.
    finished: bool,
}

/// A tool call whose arguments are still streaming.
#[derive(Debug, Clone)]
struct PendingToolCall {
    call_id: String,
    name: String,
    arguments: String,
}

impl ResponsesStreamParser {
    /// Creates a parser for one response stream.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a terminal response event has been observed.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Translates one Responses event payload into zero or more stream events.
    pub fn parse_event(&mut self, payload: &str) -> Vec<StreamEvent> {
        if self.finished {
            return Vec::new();
        }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(payload) else {
            return Vec::new();
        };
        let Some(kind) = event.get("type").and_then(serde_json::Value::as_str) else {
            return Vec::new();
        };

        match kind {
            "response.output_text.delta" | "response.refusal.delta" => delta_of(&event)
                .map(StreamEvent::Text)
                .into_iter()
                .collect(),
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                delta_of(&event)
                    .map(StreamEvent::Reasoning)
                    .into_iter()
                    .collect()
            }
            "response.output_item.added" => self.start_item(&event),
            "response.function_call_arguments.delta" => self.append_arguments(&event),
            "response.output_item.done" => self.finish_item(&event),
            "response.completed" | "response.done" | "response.incomplete" => {
                self.finish_response(&event)
            }
            "response.failed" | "error" => self.fail(&event),
            _ => Vec::new(),
        }
    }

    /// Begins tracking a tool call announced by the provider.
    fn start_item(&mut self, event: &serde_json::Value) -> Vec<StreamEvent> {
        let Some(item) = event.get("item") else {
            return Vec::new();
        };
        if item.get("type").and_then(serde_json::Value::as_str) != Some("function_call") {
            return Vec::new();
        }
        let index = output_index(event);
        let call_id = string_field(item, "call_id").unwrap_or_default();
        let name = string_field(item, "name").unwrap_or_default();

        self.pending_calls.insert(
            index,
            PendingToolCall {
                call_id: call_id.clone(),
                name: name.clone(),
                arguments: string_field(item, "arguments").unwrap_or_default(),
            },
        );

        vec![StreamEvent::ToolUseStart {
            index,
            id: call_id,
            name,
        }]
    }

    /// Accumulates a chunk of a tool call's streaming arguments.
    fn append_arguments(&mut self, event: &serde_json::Value) -> Vec<StreamEvent> {
        let index = output_index(event);
        let Some(delta) = delta_of(event) else {
            return Vec::new();
        };
        let Some(pending) = self.pending_calls.get_mut(&index) else {
            return Vec::new();
        };
        pending.arguments.push_str(&delta);

        vec![StreamEvent::ToolUseInputDelta {
            index,
            partial_json: delta,
        }]
    }

    /// Emits a completed tool call once the provider closes its output item.
    fn finish_item(&mut self, event: &serde_json::Value) -> Vec<StreamEvent> {
        let index = output_index(event);
        let Some(mut pending) = self.pending_calls.remove(&index) else {
            return Vec::new();
        };
        if let Some(arguments) = event
            .get("item")
            .and_then(|item| string_field(item, "arguments"))
        {
            pending.arguments = arguments;
        }
        self.saw_tool_call = true;

        vec![StreamEvent::ToolUseComplete {
            index,
            tool_call: ToolCall {
                id: pending.call_id,
                name: pending.name,
                arguments: if pending.arguments.is_empty() {
                    "{}".to_owned()
                } else {
                    pending.arguments
                },
            },
        }]
    }

    /// Emits the terminal event for a response that completed.
    fn finish_response(&mut self, event: &serde_json::Value) -> Vec<StreamEvent> {
        let response = event.get("response");
        let status = response
            .and_then(|value| value.get("status"))
            .and_then(serde_json::Value::as_str);
        let reason = response
            .and_then(|value| value.get("incomplete_details"))
            .and_then(|value| value.get("reason"))
            .and_then(serde_json::Value::as_str);
        let incomplete = status == Some("incomplete")
            || event.get("type").and_then(serde_json::Value::as_str) == Some("response.incomplete");
        if matches!(status, Some("failed" | "cancelled"))
            || (incomplete && reason != Some("max_output_tokens"))
        {
            return self.fail(event);
        }
        self.finished = true;
        let stop_reason = match (incomplete, self.saw_tool_call) {
            (true, _) => StopReason::Other("max_tokens".to_owned()),
            (false, true) => StopReason::ToolUse,
            (false, false) => StopReason::EndTurn,
        };
        vec![StreamEvent::Done {
            stop_reason,
            usage: event.get("response").and_then(usage_of),
        }]
    }

    /// Emits an error event for a response the provider could not complete.
    fn fail(&mut self, event: &serde_json::Value) -> Vec<StreamEvent> {
        self.finished = true;
        let error = event
            .get("response")
            .and_then(|response| response.get("error"))
            .or_else(|| event.get("error"))
            .cloned()
            .unwrap_or_else(|| event.clone());

        let error_type = string_field(&error, "code")
            .or_else(|| string_field(&error, "type"))
            .unwrap_or_else(|| "response_failed".to_owned());
        let message = string_field(&error, "message").unwrap_or_else(|| {
            event
                .get("response")
                .and_then(|response| response.get("incomplete_details"))
                .and_then(|details| string_field(details, "reason"))
                .unwrap_or_else(|| "the provider did not complete the response".to_owned())
        });

        vec![StreamEvent::Error {
            error_type,
            message,
        }]
    }

    /// Produces the terminal event for a stream that ended without one.
    ///
    /// A dropped connection mid-response must still close the stream, or the
    /// caller waits forever for a completion that is never coming.
    pub fn finish_without_terminal_event(&mut self) -> Vec<StreamEvent> {
        if self.finished {
            return Vec::new();
        }
        self.finished = true;
        vec![StreamEvent::Error {
            error_type: "incomplete_stream".to_owned(),
            message: "the response stream ended before the model finished".to_owned(),
        }]
    }
}

/// Reads the `delta` string from an event.
fn delta_of(event: &serde_json::Value) -> Option<String> {
    string_field(event, "delta")
}

/// Reads a string field from a JSON object.
fn string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

/// Reads the output index an event belongs to, defaulting to the first slot.
fn output_index(event: &serde_json::Value) -> usize {
    event
        .get("output_index")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize
}

/// Reads token usage from a finished response.
fn usage_of(response: &serde_json::Value) -> Option<StreamUsage> {
    let usage = response.get("usage")?;
    let number = |key: &str| usage.get(key).and_then(serde_json::Value::as_u64);
    Some(StreamUsage {
        prompt_tokens: number("input_tokens"),
        completion_tokens: number("output_tokens"),
        cost: None,
        cached_tokens: usage
            .get("input_tokens_details")
            .and_then(|details| details.get("cached_tokens"))
            .and_then(serde_json::Value::as_u64),
    })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        reason = "test code"
    )]
    use super::*;

    fn parse(parser: &mut ResponsesStreamParser, event: &serde_json::Value) -> Vec<StreamEvent> {
        parser.parse_event(&event.to_string())
    }

    #[rstest::rstest]
    fn an_output_text_delta_becomes_a_text_event() {
        // Given a fresh parser.
        let mut parser = ResponsesStreamParser::new();

        // When a text delta arrives.
        let events = parse(
            &mut parser,
            &serde_json::json!({ "type": "response.output_text.delta", "delta": "Hello" }),
        );

        // Then it surfaces as a text token.
        assert_eq!(events, vec![StreamEvent::Text("Hello".to_owned())]);
    }

    #[rstest::rstest]
    fn a_reasoning_summary_delta_becomes_a_reasoning_event() {
        // Given a fresh parser.
        let mut parser = ResponsesStreamParser::new();

        // When a reasoning summary delta arrives.
        let events = parse(
            &mut parser,
            &serde_json::json!({
                "type": "response.reasoning_summary_text.delta",
                "delta": "Thinking…",
            }),
        );

        // Then it surfaces as reasoning rather than as assistant text.
        assert_eq!(events, vec![StreamEvent::Reasoning("Thinking…".to_owned())]);
    }

    #[rstest::rstest]
    fn an_added_function_call_starts_a_tool_use() {
        // Given a fresh parser.
        let mut parser = ResponsesStreamParser::new();

        // When the provider announces a function call.
        let events = parse(
            &mut parser,
            &serde_json::json!({
                "type": "response.output_item.added",
                "output_index": 1,
                "item": { "type": "function_call", "call_id": "call_1", "name": "file_read" },
            }),
        );

        // Then the tool call is announced with its id and name.
        assert_eq!(
            events,
            vec![StreamEvent::ToolUseStart {
                index: 1,
                id: "call_1".to_owned(),
                name: "file_read".to_owned(),
            }]
        );
    }

    #[rstest::rstest]
    fn streamed_arguments_surface_as_input_deltas() {
        // Given an announced function call.
        let mut parser = ResponsesStreamParser::new();
        parse(
            &mut parser,
            &serde_json::json!({
                "type": "response.output_item.added",
                "output_index": 0,
                "item": { "type": "function_call", "call_id": "call_1", "name": "file_read" },
            }),
        );

        // When part of its arguments arrives.
        let events = parse(
            &mut parser,
            &serde_json::json!({
                "type": "response.function_call_arguments.delta",
                "output_index": 0,
                "delta": "{\"path\":",
            }),
        );

        // Then the partial JSON is forwarded as it streams.
        assert_eq!(
            events,
            vec![StreamEvent::ToolUseInputDelta {
                index: 0,
                partial_json: "{\"path\":".to_owned(),
            }]
        );
    }

    #[rstest::rstest]
    fn a_finished_function_call_is_emitted_complete() {
        // Given a function call whose arguments streamed in two chunks.
        let mut parser = ResponsesStreamParser::new();
        parse(
            &mut parser,
            &serde_json::json!({
                "type": "response.output_item.added",
                "output_index": 0,
                "item": { "type": "function_call", "call_id": "call_1", "name": "file_read" },
            }),
        );
        parse(
            &mut parser,
            &serde_json::json!({
                "type": "response.function_call_arguments.delta",
                "output_index": 0,
                "delta": "{\"path\":\"a\"}",
            }),
        );

        // When the provider closes the item.
        let events = parse(
            &mut parser,
            &serde_json::json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": { "type": "function_call", "call_id": "call_1", "name": "file_read" },
            }),
        );

        // Then the assembled call is emitted.
        assert_eq!(
            events,
            vec![StreamEvent::ToolUseComplete {
                index: 0,
                tool_call: ToolCall {
                    id: "call_1".to_owned(),
                    name: "file_read".to_owned(),
                    arguments: "{\"path\":\"a\"}".to_owned(),
                },
            }]
        );
    }

    #[rstest::rstest]
    fn a_completed_response_ends_the_turn() {
        // Given a response with no tool calls.
        let mut parser = ResponsesStreamParser::new();

        // When it completes.
        let events = parse(
            &mut parser,
            &serde_json::json!({ "type": "response.completed", "response": {} }),
        );

        // Then the stream ends with an end-of-turn stop reason.
        assert_eq!(
            events,
            vec![StreamEvent::Done {
                stop_reason: StopReason::EndTurn,
                usage: None,
            }]
        );
    }

    #[rstest::rstest]
    fn a_response_that_called_a_tool_stops_for_tool_use() {
        // Given a response that completed a tool call.
        let mut parser = ResponsesStreamParser::new();
        parse(
            &mut parser,
            &serde_json::json!({
                "type": "response.output_item.added",
                "output_index": 0,
                "item": { "type": "function_call", "call_id": "call_1", "name": "file_read" },
            }),
        );
        parse(
            &mut parser,
            &serde_json::json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": { "type": "function_call", "call_id": "call_1", "name": "file_read" },
            }),
        );

        // When the response completes.
        let events = parse(
            &mut parser,
            &serde_json::json!({ "type": "response.completed", "response": {} }),
        );

        // Then the stop reason tells the caller to run the tool.
        assert_eq!(
            events,
            vec![StreamEvent::Done {
                stop_reason: StopReason::ToolUse,
                usage: None,
            }]
        );
    }

    #[rstest::rstest]
    fn a_completed_response_reports_token_usage() {
        // Given a completed response carrying usage.
        let mut parser = ResponsesStreamParser::new();

        // When it completes.
        let events = parse(
            &mut parser,
            &serde_json::json!({
                "type": "response.completed",
                "response": {
                    "usage": {
                        "input_tokens": 120,
                        "output_tokens": 45,
                        "input_tokens_details": { "cached_tokens": 100 },
                    }
                },
            }),
        );

        // Then the reported counts travel with the terminal event.
        assert_eq!(
            events,
            vec![StreamEvent::Done {
                stop_reason: StopReason::EndTurn,
                usage: Some(StreamUsage {
                    prompt_tokens: Some(120),
                    completion_tokens: Some(45),
                    cost: None,
                    cached_tokens: Some(100),
                }),
            }]
        );
    }

    #[rstest::rstest]
    fn a_failed_response_surfaces_the_provider_error() {
        // Given a parser mid-stream.
        let mut parser = ResponsesStreamParser::new();

        // When the provider reports the response failed.
        let events = parse(
            &mut parser,
            &serde_json::json!({
                "type": "response.failed",
                "response": {
                    "error": { "code": "usage_limit_reached", "message": "quota exhausted" }
                },
            }),
        );

        // Then the error reaches the caller with the provider's wording.
        assert_eq!(
            events,
            vec![StreamEvent::Error {
                error_type: "usage_limit_reached".to_owned(),
                message: "quota exhausted".to_owned(),
            }]
        );
    }

    #[rstest::rstest]
    fn a_stream_that_stops_early_still_terminates() {
        // Given a stream that ended without a terminal event.
        let mut parser = ResponsesStreamParser::new();
        parse(
            &mut parser,
            &serde_json::json!({ "type": "response.output_text.delta", "delta": "partial" }),
        );

        // When the transport reports the stream is over.
        let events = parser.finish_without_terminal_event();

        // Then an error closes the stream rather than leaving it open.
        assert_eq!(
            events,
            vec![StreamEvent::Error {
                error_type: "incomplete_stream".to_owned(),
                message: "the response stream ended before the model finished".to_owned(),
            }]
        );
    }

    #[rstest::rstest]
    fn a_completed_stream_needs_no_synthetic_terminator() {
        // Given a stream that already completed.
        let mut parser = ResponsesStreamParser::new();
        parse(
            &mut parser,
            &serde_json::json!({ "type": "response.completed", "response": {} }),
        );

        // When the transport reports the stream is over.
        let events = parser.finish_without_terminal_event();

        // Then nothing further is emitted.
        assert!(events.is_empty(), "a completed stream must not error");
    }

    #[rstest::rstest]
    fn an_unrecognized_event_is_ignored() {
        // Given a fresh parser.
        let mut parser = ResponsesStreamParser::new();

        // When an event jinn does not model arrives.
        let events = parse(
            &mut parser,
            &serde_json::json!({ "type": "response.audio.delta", "delta": "..." }),
        );

        // Then it is ignored rather than treated as content.
        assert!(events.is_empty(), "unknown events must not produce output");
    }
}
