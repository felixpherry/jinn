//! Contract tests for the OpenAI Codex transport against a local mock server.
//!
//! These cover the request the Codex backend requires, the Responses event
//! stream it returns, and how failures surface — all without a live
//! subscription.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use error_stack::Report;
use futures::StreamExt as _;
use jinn_auth::{AccessToken, AccessTokenError, AccessTokenProvider};
use jinn_provider::{
    LlmMessage, LlmService, LlmServiceError, LlmServiceFactory, OpenAiCodexFactory, StopReason,
    StreamEvent, ToolDefinition,
};

/// Installs the process-wide rustls crypto provider for this test binary.
#[ctor::ctor]
fn install_rustls_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// A token provider whose outcome is scripted by the test.
#[derive(Debug)]
struct ScriptedTokens {
    outcome: Result<(&'static str, &'static str), AccessTokenError>,
    calls: AtomicUsize,
}

impl ScriptedTokens {
    fn granting() -> Arc<Self> {
        Arc::new(Self {
            outcome: Ok(("bearer-secret", "acct-1")),
            calls: AtomicUsize::new(0),
        })
    }

    fn failing(error: AccessTokenError) -> Arc<Self> {
        Arc::new(Self {
            outcome: Err(error),
            calls: AtomicUsize::new(0),
        })
    }
}

#[async_trait::async_trait]
impl AccessTokenProvider for ScriptedTokens {
    fn name(&self) -> &'static str {
        "scripted"
    }

    async fn access_token(&self) -> Result<AccessToken, Report<AccessTokenError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match &self.outcome {
            Ok((token, account_id)) => Ok(AccessToken {
                token: (*token).to_owned(),
                account_id: (*account_id).to_owned(),
            }),
            Err(AccessTokenError::NotAuthenticated) => {
                Err(Report::new(AccessTokenError::NotAuthenticated))
            }
            Err(AccessTokenError::Expired) => Err(Report::new(AccessTokenError::Expired)),
            Err(AccessTokenError::Storage) => Err(Report::new(AccessTokenError::Storage)),
            Err(AccessTokenError::Provider) => Err(Report::new(AccessTokenError::Provider)),
        }
    }
}

fn factory(
    server: &mockito::ServerGuard,
    tokens: Arc<dyn AccessTokenProvider>,
) -> OpenAiCodexFactory {
    OpenAiCodexFactory::new(
        "openai-codex".to_owned(),
        "gpt-5.5".to_owned(),
        Some(server.url()),
        tokens,
        None,
    )
}

fn user(text: &str) -> LlmMessage {
    LlmMessage::User {
        content: text.to_owned(),
        attachments: Vec::new(),
    }
}

/// Renders Responses events as an SSE body.
fn sse(events: &[serde_json::Value]) -> String {
    use std::fmt::Write as _;
    events.iter().fold(String::new(), |mut body, event| {
        let _ = write!(body, "data: {event}\n\n");
        body
    })
}

async fn collect(
    service: Box<dyn LlmService>,
    messages: Vec<LlmMessage>,
    tools: Vec<ToolDefinition>,
) -> Vec<StreamEvent> {
    let stream = service
        .chat_stream_with_tools(Some("be helpful"), messages, tools)
        .await
        .expect("stream starts");
    stream
        .map(|event| event.expect("response event"))
        .collect()
        .await
}

fn text_stream_body() -> String {
    sse(&[
        serde_json::json!({ "type": "response.created", "response": { "id": "resp_1" } }),
        serde_json::json!({ "type": "response.output_text.delta", "delta": "Hello" }),
        serde_json::json!({ "type": "response.output_text.delta", "delta": " world" }),
        serde_json::json!({
            "type": "response.completed",
            "response": { "usage": { "input_tokens": 10, "output_tokens": 3 } },
        }),
    ])
}

#[rstest::rstest]
#[case("response.done")]
#[case("response.completed")]
#[tokio::test]
async fn terminal_aliases_finish_once_and_ignore_late_content(#[case] terminal: &str) {
    // Given a backend sending a terminal event followed by stale content.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/codex/responses")
        .with_status(200)
        .with_body(sse(&[
            serde_json::json!({"type": terminal, "response": {"status": "completed"}}),
            serde_json::json!({"type": "response.output_text.delta", "delta": "late"}),
        ]))
        .create_async()
        .await;
    let service = factory(&server, ScriptedTokens::granting())
        .create()
        .expect("service");

    // When consuming the response.
    let events = collect(service, vec![user("hi")], vec![]).await;

    // Then only one terminal result is delivered.
    assert_eq!(
        events,
        vec![StreamEvent::Done {
            stop_reason: StopReason::EndTurn,
            usage: None
        }]
    );
}

#[rstest::rstest]
#[tokio::test]
async fn an_http_eof_without_a_terminal_event_reports_truncation() {
    // Given a backend closing after a partial response, without a DONE marker.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/codex/responses")
        .with_status(200)
        .with_body(sse(&[
            serde_json::json!({"type": "response.output_text.delta", "delta": "partial"}),
        ]))
        .create_async()
        .await;
    let service = factory(&server, ScriptedTokens::granting())
        .create()
        .expect("service");

    // When consuming the response.
    let events = collect(service, vec![user("hi")], vec![]).await;

    // Then the incomplete answer is not mistaken for success.
    assert!(
        matches!(events.last(), Some(StreamEvent::Error { error_type, .. }) if error_type == "incomplete_stream")
    );
}

#[rstest::rstest]
#[case("content_filter")]
#[case("unknown")]
#[tokio::test]
async fn incomplete_responses_surface_failure(#[case] reason: &str) {
    // Given an incomplete response that did not reach the output-token limit.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/codex/responses")
        .with_status(200)
        .with_body(sse(&[
            serde_json::json!({"type": "response.incomplete", "response": {
                "status": "incomplete", "incomplete_details": {"reason": reason}
            }}),
        ]))
        .create_async()
        .await;
    let service = factory(&server, ScriptedTokens::granting())
        .create()
        .expect("service");

    // When consuming the response.
    let events = collect(service, vec![user("hi")], vec![]).await;

    // Then it fails instead of reporting a normal end of turn.
    assert!(matches!(events.as_slice(), [StreamEvent::Error { message, .. }] if message == reason));
}

#[rstest::rstest]
#[tokio::test]
async fn text_only_streams_do_not_hide_response_failures() {
    // Given a backend reporting a streaming quota error.
    let mut server = mockito::Server::new_async().await;
    server.mock("POST", "/codex/responses").with_status(200).with_body(sse(&[
        serde_json::json!({"type": "error", "code": "usage_limit_reached", "message": "quota exhausted"}),
    ])).create_async().await;
    let service = factory(&server, ScriptedTokens::granting())
        .create()
        .expect("service");

    // When using the public text-only interface.
    let events: Vec<_> = service
        .chat_stream(None, vec![user("hi")])
        .await
        .expect("stream")
        .collect()
        .await;

    // Then the caller receives an error, not an empty successful answer.
    assert!(matches!(events.as_slice(), [Err(_)]));
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

#[rstest::rstest]
#[tokio::test]
async fn the_request_carries_the_subscription_bearer_token() {
    // Given a Codex backend that requires the subscription token.
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/codex/responses")
        .match_header("authorization", "Bearer bearer-secret")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(text_stream_body())
        .create_async()
        .await;
    let service = factory(&server, ScriptedTokens::granting())
        .create()
        .expect("service");

    // When a request runs.
    collect(service, vec![user("hi")], vec![]).await;

    // Then the token was sent as the bearer credential.
    mock.assert_async().await;
}

#[rstest::rstest]
#[tokio::test]
async fn the_request_names_the_chatgpt_account() {
    // Given a Codex backend that requires the account header.
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/codex/responses")
        .match_header("chatgpt-account-id", "acct-1")
        .with_status(200)
        .with_body(text_stream_body())
        .create_async()
        .await;
    let service = factory(&server, ScriptedTokens::granting())
        .create()
        .expect("service");

    // When a request runs.
    collect(service, vec![user("hi")], vec![]).await;

    // Then the subscription's account id travels with it.
    mock.assert_async().await;
}

#[rstest::rstest]
#[tokio::test]
async fn the_request_identifies_jinn_as_the_originator() {
    // Given a Codex backend that requires the originator header.
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/codex/responses")
        .match_header("originator", "jinn")
        .with_status(200)
        .with_body(text_stream_body())
        .create_async()
        .await;
    let service = factory(&server, ScriptedTokens::granting())
        .create()
        .expect("service");

    // When a request runs.
    collect(service, vec![user("hi")], vec![]).await;

    // Then the request identifies the client.
    mock.assert_async().await;
}

#[rstest::rstest]
#[tokio::test]
async fn the_request_uses_the_responses_protocol_body() {
    // Given a Codex backend that inspects the request body.
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/codex/responses")
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({
            "model": "gpt-5.5",
            "stream": true,
            "store": false,
            "instructions": "be helpful",
        })))
        .with_status(200)
        .with_body(text_stream_body())
        .create_async()
        .await;
    let service = factory(&server, ScriptedTokens::granting())
        .create()
        .expect("service");

    // When a request runs.
    collect(service, vec![user("hi")], vec![]).await;

    // Then the body is a Responses request, not a chat-completions one.
    mock.assert_async().await;
}

// ---------------------------------------------------------------------------
// Streaming
// ---------------------------------------------------------------------------

#[rstest::rstest]
#[tokio::test]
async fn text_deltas_stream_as_text_events() {
    // Given a Codex backend streaming two text deltas.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/codex/responses")
        .with_status(200)
        .with_body(text_stream_body())
        .create_async()
        .await;
    let service = factory(&server, ScriptedTokens::granting())
        .create()
        .expect("service");

    // When the response streams.
    let events = collect(service, vec![user("hi")], vec![]).await;

    // Then both text tokens reach the caller in order.
    let text: Vec<String> = events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::Text(text) => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(text, vec!["Hello".to_owned(), " world".to_owned()]);
}

#[rstest::rstest]
#[tokio::test]
async fn reasoning_summaries_stream_as_reasoning_events() {
    // Given a Codex backend streaming a reasoning summary.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/codex/responses")
        .with_status(200)
        .with_body(sse(&[
            serde_json::json!({
                "type": "response.reasoning_summary_text.delta",
                "delta": "Considering options",
            }),
            serde_json::json!({ "type": "response.completed", "response": {} }),
        ]))
        .create_async()
        .await;
    let service = factory(&server, ScriptedTokens::granting())
        .create()
        .expect("service");

    // When the response streams.
    let events = collect(service, vec![user("hi")], vec![]).await;

    // Then the summary surfaces as reasoning.
    assert!(
        events.contains(&StreamEvent::Reasoning("Considering options".to_owned())),
        "reasoning must reach the caller: {events:?}"
    );
}

#[rstest::rstest]
#[tokio::test]
async fn a_completed_response_reports_usage() {
    // Given a Codex backend reporting token usage.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/codex/responses")
        .with_status(200)
        .with_body(text_stream_body())
        .create_async()
        .await;
    let service = factory(&server, ScriptedTokens::granting())
        .create()
        .expect("service");

    // When the response streams.
    let events = collect(service, vec![user("hi")], vec![]).await;

    // Then the terminal event carries the provider's token counts.
    let usage = events.iter().find_map(|event| match event {
        StreamEvent::Done { usage, .. } => usage.clone(),
        _ => None,
    });
    assert_eq!(
        usage.and_then(|usage| usage.prompt_tokens),
        Some(10),
        "usage must travel with the terminal event"
    );
}

fn tool_stream_body() -> String {
    sse(&[
        serde_json::json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "item": { "type": "function_call", "call_id": "call_1", "name": "file_read" },
        }),
        serde_json::json!({
            "type": "response.function_call_arguments.delta",
            "output_index": 0,
            "delta": "{\"path\":",
        }),
        serde_json::json!({
            "type": "response.function_call_arguments.delta",
            "output_index": 0,
            "delta": "\"a.txt\"}",
        }),
        serde_json::json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "item": {
                "type": "function_call",
                "call_id": "call_1",
                "name": "file_read",
                "arguments": "{\"path\":\"a.txt\"}",
            },
        }),
        serde_json::json!({ "type": "response.completed", "response": {} }),
    ])
}

fn tools() -> Vec<ToolDefinition> {
    vec![ToolDefinition {
        name: "file_read".to_owned(),
        description: "Read a file".to_owned(),
        parameters: serde_json::json!({ "type": "object", "properties": {} }),
        prompt_snippet: None,
        prompt_guidelines: Vec::new(),
        server_tool_type: None,
    }]
}

#[rstest::rstest]
#[tokio::test]
async fn tool_call_arguments_stream_as_they_arrive() {
    // Given a Codex backend streaming a tool call in chunks.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/codex/responses")
        .with_status(200)
        .with_body(tool_stream_body())
        .create_async()
        .await;
    let service = factory(&server, ScriptedTokens::granting())
        .create()
        .expect("service");

    // When the response streams.
    let events = collect(service, vec![user("read a.txt")], tools()).await;

    // Then each argument chunk is forwarded as it arrives.
    let deltas: Vec<String> = events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::ToolUseInputDelta { partial_json, .. } => Some(partial_json.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        deltas,
        vec!["{\"path\":".to_owned(), "\"a.txt\"}".to_owned()]
    );
}

#[rstest::rstest]
#[tokio::test]
async fn a_finished_tool_call_is_delivered_complete() {
    // Given a Codex backend streaming a tool call to completion.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/codex/responses")
        .with_status(200)
        .with_body(tool_stream_body())
        .create_async()
        .await;
    let service = factory(&server, ScriptedTokens::granting())
        .create()
        .expect("service");

    // When the response streams.
    let events = collect(service, vec![user("read a.txt")], tools()).await;

    // Then the assembled call reaches the caller.
    let call = events.iter().find_map(|event| match event {
        StreamEvent::ToolUseComplete { tool_call, .. } => Some(tool_call.clone()),
        _ => None,
    });
    let call = call.expect("a completed tool call");
    assert_eq!(call.arguments, "{\"path\":\"a.txt\"}");
}

#[rstest::rstest]
#[tokio::test]
async fn a_response_that_called_a_tool_stops_for_tool_use() {
    // Given a Codex backend that finished with a tool call.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/codex/responses")
        .with_status(200)
        .with_body(tool_stream_body())
        .create_async()
        .await;
    let service = factory(&server, ScriptedTokens::granting())
        .create()
        .expect("service");

    // When the response streams.
    let events = collect(service, vec![user("read a.txt")], tools()).await;

    // Then the turn ends asking for tool execution.
    assert!(
        events.iter().any(|event| matches!(
            event,
            StreamEvent::Done {
                stop_reason: StopReason::ToolUse,
                ..
            }
        )),
        "the turn must stop for tool use: {events:?}"
    );
}

#[rstest::rstest]
#[tokio::test]
async fn a_tool_result_continues_the_conversation() {
    // Given a Codex backend inspecting the follow-up request.
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/codex/responses")
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({
            "input": [
                { "type": "message", "role": "user" },
                { "type": "function_call", "call_id": "call_1" },
                { "type": "function_call_output", "call_id": "call_1", "output": "file body" },
            ]
        })))
        .with_status(200)
        .with_body(text_stream_body())
        .create_async()
        .await;
    let service = factory(&server, ScriptedTokens::granting())
        .create()
        .expect("service");

    // When the tool result is sent back for continuation.
    let messages = vec![
        user("read a.txt"),
        LlmMessage::Assistant {
            content: String::new(),
            tool_calls: Some(vec![jinn_provider::ToolCall {
                id: "call_1".to_owned(),
                name: "file_read".to_owned(),
                arguments: "{\"path\":\"a.txt\"}".to_owned(),
            }]),
        },
        LlmMessage::Tool {
            tool_call_id: "call_1".to_owned(),
            name: "file_read".to_owned(),
            content: "file body".to_owned(),
        },
    ];
    collect(service, messages, tools()).await;

    // Then the follow-up carries the call and its output as protocol items.
    mock.assert_async().await;
}

#[rstest::rstest]
#[tokio::test]
async fn dropping_the_stream_closes_the_in_flight_http_response() {
    use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _};

    // Given a backend holding a response open after its first event.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("connection");
        let mut socket = tokio::io::BufReader::new(socket);
        let mut length = 0;
        loop {
            let mut line = String::new();
            socket.read_line(&mut line).await.expect("header");
            if line == "\r\n" {
                break;
            }
            if let Some((key, value)) = line.split_once(':')
                && key.eq_ignore_ascii_case("content-length")
            {
                length = value.trim().parse::<usize>().expect("length");
            }
        }
        socket
            .read_exact(&mut vec![0; length])
            .await
            .expect("request body");
        socket.get_mut().write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n").await.expect("response");
        socket.read(&mut [0; 1]).await
    });
    let service = jinn_provider::OpenAiCodexService::new(
        "gpt-5.5".to_owned(),
        Some(format!("http://{address}")),
        ScriptedTokens::granting(),
        None,
    );
    let mut stream = service
        .chat_stream_with_tools(None, vec![user("hi")], vec![])
        .await
        .expect("stream");
    stream.next().await.expect("first event").expect("text");

    // When the caller cancels the in-flight response.
    drop(stream);

    // Then the backend observes the client disconnect rather than background consumption.
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), server)
        .await
        .expect("client disconnects promptly")
        .expect("server task");
    assert!(matches!(result, Ok(0) | Err(_)));
}

// ---------------------------------------------------------------------------
// Failures
// ---------------------------------------------------------------------------

#[rstest::rstest]
#[tokio::test]
async fn a_quota_failure_surfaces_as_a_rate_limit() {
    // Given a subscription that has hit its usage limit.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/codex/responses")
        .with_status(429)
        .with_body(serde_json::json!({ "error": { "code": "usage_limit_reached" } }).to_string())
        .create_async()
        .await;
    let service = factory(&server, ScriptedTokens::granting())
        .create()
        .expect("service");

    // When a request runs.
    let Err(error) = service
        .chat_stream_with_tools(None, vec![user("hi")], vec![])
        .await
    else {
        panic!("the request must fail when the quota is exhausted");
    };

    // Then it surfaces through normal rate-limit handling.
    assert!(matches!(
        error.downcast_ref::<LlmServiceError>(),
        Some(LlmServiceError::RateLimited { .. })
    ));
}

#[rstest::rstest]
#[tokio::test]
async fn an_unauthorized_response_fails_the_request() {
    // Given a backend that rejects the subscription token.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/codex/responses")
        .with_status(401)
        .with_body(serde_json::json!({ "error": "invalid_token" }).to_string())
        .create_async()
        .await;
    let service = factory(&server, ScriptedTokens::granting())
        .create()
        .expect("service");

    // When a request runs.
    let result = service
        .chat_stream_with_tools(None, vec![user("hi")], vec![])
        .await;

    // Then the request fails rather than falling back to another route.
    assert!(result.is_err(), "an unauthorized response must fail");
}

#[rstest::rstest]
#[tokio::test]
async fn unusable_credentials_fail_without_contacting_the_backend() {
    // Given credentials that can no longer be refreshed.
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/codex/responses")
        .expect(0)
        .with_status(200)
        .create_async()
        .await;
    let service = factory(&server, ScriptedTokens::failing(AccessTokenError::Expired))
        .create()
        .expect("service");

    // When a request runs.
    let Err(error) = service
        .chat_stream_with_tools(None, vec![user("hi")], vec![])
        .await
    else {
        panic!("the request must fail when credentials are unusable");
    };

    // Then no request is attempted and the failure names authentication.
    mock.assert_async().await;
    assert!(matches!(
        error.downcast_ref::<LlmServiceError>(),
        Some(LlmServiceError::ApiKey)
    ));
}

#[rstest::rstest]
#[tokio::test]
async fn a_failed_response_event_surfaces_the_provider_error() {
    // Given a backend that fails the response mid-stream.
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/codex/responses")
        .with_status(200)
        .with_body(sse(&[serde_json::json!({
            "type": "response.failed",
            "response": {
                "error": { "code": "usage_not_included", "message": "not on your plan" }
            },
        })]))
        .create_async()
        .await;
    let service = factory(&server, ScriptedTokens::granting())
        .create()
        .expect("service");

    // When the response streams.
    let events = collect(service, vec![user("hi")], vec![]).await;

    // Then the provider's error reaches the caller.
    assert!(
        events.iter().any(|event| matches!(
            event,
            StreamEvent::Error { error_type, .. } if error_type == "usage_not_included"
        )),
        "the provider error must reach the caller: {events:?}"
    );
}
