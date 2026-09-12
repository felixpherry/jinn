//! Converting conversation history into Responses protocol input items.
//!
//! The Responses protocol takes a flat list of typed items: user and assistant
//! messages, the function calls an assistant made, and the outputs those calls
//! produced. Each jinn [`LlmMessage`] expands into one or more of those items.

use base64::Engine as _;

use crate::Attachment;
use crate::LlmMessage;
use crate::tool_types::ToolDefinition;

/// Converts conversation history into Responses input items.
///
/// The system prompt is not part of this list: the Responses protocol carries
/// it as a top-level `instructions` field, so it travels as request data
/// rather than as a message.
#[must_use]
pub fn build_input_items(messages: &[LlmMessage]) -> Vec<serde_json::Value> {
    let mut items = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        match message {
            LlmMessage::User {
                content,
                attachments,
            } => items.push(user_item(content, attachments)),
            LlmMessage::Assistant {
                content,
                tool_calls,
            } => {
                if !content.is_empty() {
                    items.push(assistant_item(content, index));
                }
                for call in tool_calls.iter().flatten() {
                    items.push(serde_json::json!({
                        "type": "function_call",
                        "call_id": call.id,
                        "name": call.name,
                        "arguments": call.arguments,
                    }));
                }
            }
            LlmMessage::Tool {
                tool_call_id,
                content,
                ..
            } => items.push(serde_json::json!({
                "type": "function_call_output",
                "call_id": tool_call_id,
                "output": content,
            })),
        }
    }
    items
}

/// Builds a user input item, carrying any attachments alongside the text.
fn user_item(content: &str, attachments: &[Attachment]) -> serde_json::Value {
    let mut parts = vec![serde_json::json!({ "type": "input_text", "text": content })];
    for attachment in attachments {
        parts.push(serde_json::json!({
            "type": "input_image",
            "detail": "auto",
            "image_url": data_url(attachment),
        }));
    }
    serde_json::json!({
        "type": "message",
        "role": "user",
        "content": parts,
    })
}

/// Builds an assistant input item replaying previously generated text.
///
/// The protocol expects each replayed assistant message to carry an id. jinn
/// does not persist provider message ids, so a stable synthetic id derived
/// from the message's position is used instead.
fn assistant_item(content: &str, index: usize) -> serde_json::Value {
    serde_json::json!({
        "type": "message",
        "role": "assistant",
        "id": format!("msg_jinn_{index}"),
        "status": "completed",
        "content": [{ "type": "output_text", "text": content, "annotations": [] }],
    })
}

/// Renders an attachment as a base64 data URL.
fn data_url(attachment: &Attachment) -> String {
    format!(
        "data:{};base64,{}",
        attachment.media_type(),
        base64::engine::general_purpose::STANDARD.encode(attachment.data())
    )
}

/// Converts tool definitions into Responses protocol tool declarations.
///
/// Server-side tools have no Responses equivalent in this backend and are
/// dropped: sending them would make the provider reject the whole request.
#[must_use]
pub fn build_tools(tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .filter(|tool| tool.server_tool_type.is_none())
        .map(|tool| {
            serde_json::json!({
                "type": "function",
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.parameters,
                "strict": false,
            })
        })
        .collect()
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
    use crate::tool_types::ToolCall;

    fn user(content: &str) -> LlmMessage {
        LlmMessage::User {
            content: content.to_owned(),
            attachments: Vec::new(),
        }
    }

    #[rstest::rstest]
    fn a_user_message_becomes_an_input_text_item() {
        // Given a plain user message.
        let messages = vec![user("hello")];

        // When converting it to input items.
        let items = build_input_items(&messages);

        // Then it becomes a user message carrying input text.
        assert_eq!(
            items,
            vec![serde_json::json!({
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "hello" }],
            })]
        );
    }

    #[rstest::rstest]
    fn an_image_attachment_becomes_an_input_image_part() {
        // Given a user message with an image attached.
        let messages = vec![LlmMessage::User {
            content: "describe this".to_owned(),
            attachments: vec![Attachment::image("image/png", vec![1, 2, 3])],
        }];

        // When converting it to input items.
        let items = build_input_items(&messages);

        // Then the image rides alongside the text as a data URL.
        let parts = items[0]["content"].as_array().expect("content parts");
        assert_eq!(parts[1]["type"], "input_image");
        assert!(
            parts[1]["image_url"]
                .as_str()
                .expect("image url")
                .starts_with("data:image/png;base64,"),
            "the image must be embedded as a data URL"
        );
    }

    #[rstest::rstest]
    fn an_assistant_tool_call_becomes_a_function_call_item() {
        // Given an assistant turn that called a tool.
        let messages = vec![LlmMessage::Assistant {
            content: String::new(),
            tool_calls: Some(vec![ToolCall {
                id: "call_1".to_owned(),
                name: "file_read".to_owned(),
                arguments: "{\"path\":\"a\"}".to_owned(),
            }]),
        }];

        // When converting it to input items.
        let items = build_input_items(&messages);

        // Then it becomes a function call keyed by the provider's call id.
        assert_eq!(
            items,
            vec![serde_json::json!({
                "type": "function_call",
                "call_id": "call_1",
                "name": "file_read",
                "arguments": "{\"path\":\"a\"}",
            })]
        );
    }

    #[rstest::rstest]
    fn a_tool_result_becomes_a_function_call_output_item() {
        // Given the result of a tool call.
        let messages = vec![LlmMessage::Tool {
            tool_call_id: "call_1".to_owned(),
            name: "file_read".to_owned(),
            content: "file contents".to_owned(),
        }];

        // When converting it to input items.
        let items = build_input_items(&messages);

        // Then it becomes a function call output keyed by the same call id.
        assert_eq!(
            items,
            vec![serde_json::json!({
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "file contents",
            })]
        );
    }

    #[rstest::rstest]
    fn an_assistant_turn_with_text_and_a_call_produces_both_items() {
        // Given an assistant turn that spoke and then called a tool.
        let messages = vec![LlmMessage::Assistant {
            content: "Let me look.".to_owned(),
            tool_calls: Some(vec![ToolCall {
                id: "call_1".to_owned(),
                name: "file_read".to_owned(),
                arguments: "{}".to_owned(),
            }]),
        }];

        // When converting it to input items.
        let items = build_input_items(&messages);

        // Then the spoken text precedes the call, preserving turn order.
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["role"], "assistant");
        assert_eq!(items[1]["type"], "function_call");
    }

    #[rstest::rstest]
    fn an_empty_assistant_turn_contributes_no_message_item() {
        // Given an assistant turn that produced no text and no calls.
        let messages = vec![LlmMessage::Assistant {
            content: String::new(),
            tool_calls: None,
        }];

        // When converting it to input items.
        let items = build_input_items(&messages);

        // Then nothing is sent for it.
        assert!(items.is_empty(), "an empty turn must not become an item");
    }

    #[rstest::rstest]
    fn a_tool_definition_becomes_a_function_tool() {
        // Given a locally executed tool.
        let tools = vec![ToolDefinition {
            name: "file_read".to_owned(),
            description: "Read a file".to_owned(),
            parameters: serde_json::json!({ "type": "object", "properties": {} }),
            prompt_snippet: None,
            prompt_guidelines: Vec::new(),
            server_tool_type: None,
        }];

        // When converting it for the request.
        let converted = build_tools(&tools);

        // Then it is declared as a function tool with its schema.
        assert_eq!(
            converted,
            vec![serde_json::json!({
                "type": "function",
                "name": "file_read",
                "description": "Read a file",
                "parameters": { "type": "object", "properties": {} },
                "strict": false,
            })]
        );
    }

    #[rstest::rstest]
    fn a_server_side_tool_is_not_sent() {
        // Given a tool the provider would have to execute itself.
        let tools = vec![ToolDefinition {
            name: "web_search".to_owned(),
            description: "Search".to_owned(),
            parameters: serde_json::json!({}),
            prompt_snippet: None,
            prompt_guidelines: Vec::new(),
            server_tool_type: Some(crate::tool_types::ServerToolType::OpenrouterWebSearch),
        }];

        // When converting it for the request.
        let converted = build_tools(&tools);

        // Then it is dropped rather than sent to a backend that cannot run it.
        assert!(converted.is_empty(), "server tools must not be declared");
    }
}
