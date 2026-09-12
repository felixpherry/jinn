//! Factory for [`OpenAiCodexService`] instances.

use std::sync::Arc;

use error_stack::Report;
use jinn_auth::AccessTokenProvider;

use crate::openai_codex::service::OpenAiCodexService;
use crate::reasoning::ReasoningEffort;
use crate::service::{LlmService, LlmServiceError, LlmServiceFactory};

/// Creates Codex chat services for one model.
#[derive(derive_more::Debug)]
pub struct OpenAiCodexFactory {
    name: String,
    model: String,
    base_url: Option<String>,
    #[debug("AccessTokenProvider<{}>", self.tokens.name())]
    tokens: Arc<dyn AccessTokenProvider>,
    reasoning: Option<ReasoningEffort>,
}

impl OpenAiCodexFactory {
    /// Creates a factory for `model`, authenticating through `tokens`.
    #[must_use]
    pub fn new(
        name: String,
        model: String,
        base_url: Option<String>,
        tokens: Arc<dyn AccessTokenProvider>,
        reasoning: Option<ReasoningEffort>,
    ) -> Self {
        Self {
            name,
            model,
            base_url,
            tokens,
            reasoning,
        }
    }
}

impl LlmServiceFactory for OpenAiCodexFactory {
    fn create(&self) -> Result<Box<dyn LlmService>, Report<LlmServiceError>> {
        Ok(Box::new(OpenAiCodexService::new(
            self.model.clone(),
            self.base_url.clone(),
            self.tokens.clone(),
            self.reasoning,
        )))
    }

    fn name(&self) -> &str {
        &self.name
    }
}
