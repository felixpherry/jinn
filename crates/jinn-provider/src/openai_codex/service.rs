//! Streaming chat against the OpenAI Codex backend.
//!
//! Each request resolves a subscription access token, sends a Responses
//! protocol body to the Codex endpoint, and normalizes the event stream into
//! jinn's provider-neutral events. Access tokens are resolved immediately
//! before the request so a refresh performed by another in-flight request is
//! always picked up.

use error_stack::{Report, ResultExt as _};
use futures::StreamExt as _;
use jinn_auth::{AccessToken, AccessTokenError, AccessTokenProvider};
use reqwest::Client;
use std::sync::Arc;

use crate::llm_message::LlmMessage;
use crate::openai_compat::sse::{SseEvent, SseParser};
use crate::openai_responses::request::{build_input_items, build_tools};
use crate::openai_responses::stream::ResponsesStreamParser;
use crate::reasoning::ReasoningEffort;
use crate::service::{ChatStream, LlmService, LlmServiceError, ToolStream};
use crate::stream_event::StreamEvent;
use crate::tool_types::ToolDefinition;

/// Default Codex backend host.
pub const DEFAULT_CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api";

/// Client identifier the Codex backend expects on every request.
const ORIGINATOR: &str = "jinn";

/// Fallback instructions when a request carries no system prompt.
const DEFAULT_INSTRUCTIONS: &str = "You are a helpful assistant.";

/// Name used in diagnostics and error messages.
const PROVIDER_NAME: &str = "openai-codex";

/// A streaming chat session against the Codex backend.
#[derive(derive_more::Debug)]
pub struct OpenAiCodexService {
    #[debug(skip)]
    client: Client,
    model: String,
    base_url: String,
    #[debug("AccessTokenProvider<{}>", self.tokens.name())]
    tokens: Arc<dyn AccessTokenProvider>,
    reasoning: Option<ReasoningEffort>,
}

impl OpenAiCodexService {
    /// Creates a service for `model`, authenticating through `tokens`.
    #[must_use]
    pub fn new(
        model: String,
        base_url: Option<String>,
        tokens: Arc<dyn AccessTokenProvider>,
        reasoning: Option<ReasoningEffort>,
    ) -> Self {
        Self::with_client(Client::new(), model, base_url, tokens, reasoning)
    }

    /// Creates a service using a caller-supplied HTTP client.
    #[must_use]
    pub fn with_client(
        client: Client,
        model: String,
        base_url: Option<String>,
        tokens: Arc<dyn AccessTokenProvider>,
        reasoning: Option<ReasoningEffort>,
    ) -> Self {
        Self {
            client,
            model,
            base_url: base_url.unwrap_or_else(|| DEFAULT_CODEX_BASE_URL.to_owned()),
            tokens,
            reasoning,
        }
    }

    /// The Codex responses endpoint for this service's backend host.
    fn endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        match base.ends_with("/codex/responses") {
            true => base.to_owned(),
            false if base.ends_with("/codex") => format!("{base}/responses"),
            false => format!("{base}/codex/responses"),
        }
    }

    /// Builds the Responses protocol request body.
    fn request_body(
        &self,
        system_prompt: Option<&str>,
        messages: &[LlmMessage],
        tools: &[ToolDefinition],
    ) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": self.model,
            "store": false,
            "stream": true,
            "instructions": system_prompt.unwrap_or(DEFAULT_INSTRUCTIONS),
            "input": build_input_items(messages),
            "tool_choice": "auto",
            "parallel_tool_calls": true,
            "text": { "verbosity": "low" },
            "include": ["reasoning.encrypted_content"],
        });

        let converted_tools = build_tools(tools);
        if !converted_tools.is_empty() {
            body["tools"] = serde_json::Value::Array(converted_tools);
        }
        if let Some(effort) = self.reasoning {
            body["reasoning"] = serde_json::json!({
                "effort": effort.as_str(),
                "summary": "auto",
            });
        }
        body
    }

    /// Sends the request and returns the streaming response.
    async fn send_streaming_request(
        &self,
        system_prompt: Option<&str>,
        messages: &[LlmMessage],
        tools: &[ToolDefinition],
    ) -> Result<reqwest::Response, Report<LlmServiceError>> {
        let AccessToken { token, account_id } =
            self.tokens.access_token().await.map_err(map_token_error)?;

        let body = self.request_body(system_prompt, messages, tools);
        let request_id = uuid::Uuid::now_v7().to_string();

        tracing::debug!(
            provider = PROVIDER_NAME,
            model = %self.model,
            endpoint = %self.endpoint(),
            "sending Codex responses request"
        );

        let response = self
            .client
            .post(self.endpoint())
            .bearer_auth(token)
            .header("chatgpt-account-id", account_id)
            .header("originator", ORIGINATOR)
            .header("OpenAI-Beta", "responses=experimental")
            .header("session-id", &request_id)
            .header("x-client-request-id", &request_id)
            .header("accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .change_context(LlmServiceError::Provider)
            .attach("Codex streaming request failed")?;

        let status = response.status();
        if !status.is_success() {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(crate::service::parse_retry_after_header);
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "(no body)".to_owned());
            return Err(crate::service::classify_http_error(
                status,
                &error_body,
                PROVIDER_NAME,
                retry_after,
            ));
        }

        Ok(response)
    }
}

/// Maps a token-resolution failure onto the service's error vocabulary.
///
/// Credentials that cannot be refreshed surface as an authentication problem
/// so the request fails and the session is preserved: re-logging in stays an
/// explicit user action.
fn map_token_error(failure: Report<AccessTokenError>) -> Report<LlmServiceError> {
    let target = match failure.downcast_ref::<AccessTokenError>() {
        Some(AccessTokenError::NotAuthenticated | AccessTokenError::Expired) => {
            LlmServiceError::ApiKey
        }
        _ => LlmServiceError::Provider,
    };
    failure.change_context(target)
}

/// Turns the HTTP response body into a stream of jinn events.
fn create_tool_stream(response: reqwest::Response) -> ToolStream {
    let initial = (
        ResponsesStreamParser::new(),
        SseParser::new(),
        response.bytes_stream().boxed(),
    );

    let stream = futures::stream::unfold(initial, |(mut parser, mut sse, mut bytes)| async move {
        if parser.is_finished() {
            return None;
        }
        let results: Vec<Result<StreamEvent, Report<LlmServiceError>>> = match bytes.next().await {
            None => parser
                .finish_without_terminal_event()
                .into_iter()
                .map(Ok)
                .collect(),
            Some(Ok(bytes)) => sse
                .feed(&bytes)
                .into_iter()
                .flat_map(|event| match event {
                    SseEvent::Data(payload) => parser.parse_event(&payload),
                    SseEvent::Done => parser.finish_without_terminal_event(),
                })
                .map(Ok)
                .collect(),
            Some(Err(err)) => {
                parser.finish_without_terminal_event();
                vec![Err(Report::new(LlmServiceError::Provider)
                    .attach("Codex stream error")
                    .attach(err.to_string()))]
            }
        };
        Some((results, (parser, sse, bytes)))
    })
    .flat_map(futures::stream::iter);

    Box::pin(stream)
}

#[async_trait::async_trait]
impl LlmService for OpenAiCodexService {
    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    async fn chat_stream(
        &self,
        system_prompt: Option<&str>,
        messages: Vec<LlmMessage>,
    ) -> Result<ChatStream, Report<LlmServiceError>> {
        let events = self
            .chat_stream_with_tools(system_prompt, messages, Vec::new())
            .await?;
        let text = events.filter_map(|event| async move {
            match event {
                Ok(StreamEvent::Text(text)) => Some(Ok(text)),
                Ok(StreamEvent::Error {
                    error_type,
                    message,
                }) => Some(Err(Report::new(LlmServiceError::Provider)
                    .attach(error_type)
                    .attach(message))),
                Ok(_) => None,
                Err(err) => Some(Err(err)),
            }
        });
        Ok(Box::pin(text))
    }

    async fn chat_stream_with_tools(
        &self,
        system_prompt: Option<&str>,
        messages: Vec<LlmMessage>,
        tools: Vec<ToolDefinition>,
    ) -> Result<ToolStream, Report<LlmServiceError>> {
        let response = self
            .send_streaming_request(system_prompt, &messages, &tools)
            .await?;
        Ok(create_tool_stream(response))
    }
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

    #[derive(Debug)]
    struct StubTokens;

    #[async_trait::async_trait]
    impl AccessTokenProvider for StubTokens {
        fn name(&self) -> &'static str {
            "stub"
        }

        async fn access_token(&self) -> Result<AccessToken, Report<AccessTokenError>> {
            Ok(AccessToken {
                token: "bearer-secret".to_owned(),
                account_id: "acct-1".to_owned(),
            })
        }
    }

    fn service(base_url: Option<String>) -> OpenAiCodexService {
        OpenAiCodexService::new("gpt-5.5".to_owned(), base_url, Arc::new(StubTokens), None)
    }

    #[rstest::rstest]
    fn the_endpoint_is_the_codex_responses_path() {
        // Given a service pointed at a backend host.
        let service = service(Some("https://example.test/backend-api".to_owned()));

        // When resolving the request endpoint.
        // Then it targets the Codex responses path.
        assert_eq!(
            service.endpoint(),
            "https://example.test/backend-api/codex/responses"
        );
    }

    #[rstest::rstest]
    fn a_base_url_already_naming_the_path_is_not_doubled() {
        // Given a base URL that already names the responses path.
        let service = service(Some("https://example.test/codex/responses".to_owned()));

        // When resolving the request endpoint.
        // Then the path is used as-is.
        assert_eq!(service.endpoint(), "https://example.test/codex/responses");
    }

    #[rstest::rstest]
    fn the_request_body_disables_server_side_storage() {
        // Given a Codex service.
        let service = service(None);

        // When building a request body.
        let body = service.request_body(None, &[], &[]);

        // Then storage is disabled, as the Codex backend requires.
        assert_eq!(body["store"], serde_json::json!(false));
    }

    #[rstest::rstest]
    fn the_system_prompt_travels_as_instructions() {
        // Given a Codex service.
        let service = service(None);

        // When building a request body with a system prompt.
        let body = service.request_body(Some("be terse"), &[], &[]);

        // Then the prompt travels as request data, not as a message.
        assert_eq!(body["instructions"], serde_json::json!("be terse"));
    }

    #[rstest::rstest]
    fn a_request_without_tools_declares_none() {
        // Given a Codex service.
        let service = service(None);

        // When building a request body with no tools.
        let body = service.request_body(None, &[], &[]);

        // Then no tools field is sent at all.
        assert!(
            body.get("tools").is_none(),
            "an empty tool list must be omitted"
        );
    }

    #[rstest::rstest]
    fn reasoning_effort_is_forwarded_when_configured() {
        // Given a service configured for high reasoning effort.
        let service = OpenAiCodexService::new(
            "gpt-5.5".to_owned(),
            None,
            Arc::new(StubTokens),
            Some(ReasoningEffort::High),
        );

        // When building a request body.
        let body = service.request_body(None, &[], &[]);

        // Then the effort travels with the request.
        assert_eq!(body["reasoning"]["effort"], serde_json::json!("high"));
    }

    #[rstest::rstest]
    fn debug_output_does_not_expose_credentials() {
        // Given a Codex service holding a token provider.
        let service = service(None);

        // When formatting it for diagnostics.
        let rendered = format!("{service:?}");

        // Then no bearer token appears in the output.
        assert!(
            !rendered.contains("bearer-secret"),
            "provider diagnostics must not carry credentials: {rendered}"
        );
    }
}
