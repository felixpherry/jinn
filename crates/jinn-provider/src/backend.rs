//! Backend discriminator for LLM providers.
//!
//! Replaces `llm::builder::LLMBackend` with our own enum that maps to
//! provider configuration strings.

use wherror::Error;

/// Error type for invalid backend strings.
#[derive(Debug, Error)]
#[error("invalid backend")]
pub struct BackendError;

/// Supported LLM backend providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    /// OpenAI (also covers OpenAI-compatible providers).
    OpenAI,
    /// OpenAI Codex, reached through a ChatGPT subscription rather than an
    /// API key. Speaks the Responses protocol against the Codex backend.
    OpenAiCodex,
    /// Anthropic (Claude models).
    Anthropic,
    /// Ollama (local inference).
    Ollama,
    /// DeepSeek.
    DeepSeek,
    /// xAI (Grok models).
    XAI,
    /// Phind.
    Phind,
    /// Google (Gemini models).
    Google,
    /// Groq.
    Groq,
    /// Azure OpenAI.
    AzureOpenAI,
    /// ElevenLabs.
    ElevenLabs,
    /// Cohere.
    Cohere,
    /// Mistral.
    Mistral,
    /// OpenRouter.
    OpenRouter,
    /// HuggingFace.
    HuggingFace,
    /// LM Studio (local inference).
    LmStudio,
    /// ZAI.
    ZAI,
}

impl std::str::FromStr for Backend {
    type Err = BackendError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openai" => Ok(Self::OpenAI),
            "openai-codex" => Ok(Self::OpenAiCodex),
            "anthropic" => Ok(Self::Anthropic),
            "ollama" => Ok(Self::Ollama),
            "deepseek" => Ok(Self::DeepSeek),
            "xai" => Ok(Self::XAI),
            "phind" => Ok(Self::Phind),
            "google" => Ok(Self::Google),
            "groq" => Ok(Self::Groq),
            "azure-openai" => Ok(Self::AzureOpenAI),
            "elevenlabs" => Ok(Self::ElevenLabs),
            "cohere" => Ok(Self::Cohere),
            "mistral" => Ok(Self::Mistral),
            "openrouter" => Ok(Self::OpenRouter),
            "huggingface" => Ok(Self::HuggingFace),
            "lmstudio" => Ok(Self::LmStudio),
            "zai" => Ok(Self::ZAI),
            _ => Err(BackendError),
        }
    }
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenAI => write!(f, "openai"),
            Self::OpenAiCodex => write!(f, "openai-codex"),
            Self::Anthropic => write!(f, "anthropic"),
            Self::Ollama => write!(f, "ollama"),
            Self::DeepSeek => write!(f, "deepseek"),
            Self::XAI => write!(f, "xai"),
            Self::Phind => write!(f, "phind"),
            Self::Google => write!(f, "google"),
            Self::Groq => write!(f, "groq"),
            Self::AzureOpenAI => write!(f, "azure-openai"),
            Self::ElevenLabs => write!(f, "elevenlabs"),
            Self::Cohere => write!(f, "cohere"),
            Self::Mistral => write!(f, "mistral"),
            Self::OpenRouter => write!(f, "openrouter"),
            Self::HuggingFace => write!(f, "huggingface"),
            Self::LmStudio => write!(f, "lmstudio"),
            Self::ZAI => write!(f, "zai"),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing, reason = "test code")]
    use super::*;

    #[rstest::rstest]
    #[case("openai", Backend::OpenAI)]
    #[case("openai-codex", Backend::OpenAiCodex)]
    #[case("anthropic", Backend::Anthropic)]
    #[case("ollama", Backend::Ollama)]
    #[case("deepseek", Backend::DeepSeek)]
    #[case("xai", Backend::XAI)]
    #[case("phind", Backend::Phind)]
    #[case("google", Backend::Google)]
    #[case("groq", Backend::Groq)]
    #[case("azure-openai", Backend::AzureOpenAI)]
    #[case("elevenlabs", Backend::ElevenLabs)]
    #[case("cohere", Backend::Cohere)]
    #[case("mistral", Backend::Mistral)]
    #[case("openrouter", Backend::OpenRouter)]
    #[case("huggingface", Backend::HuggingFace)]
    #[case("lmstudio", Backend::LmStudio)]
    #[case("zai", Backend::ZAI)]
    #[case("OPENAI", Backend::OpenAI)]
    #[case("OpenRouter", Backend::OpenRouter)]
    fn from_str_parses_known_backends(#[case] input: &str, #[case] expected: Backend) {
        assert_eq!(input.parse::<Backend>().unwrap(), expected);
    }

    #[rstest::rstest]
    fn from_str_rejects_unknown() {
        assert!("not-a-backend".parse::<Backend>().is_err());
    }

    #[rstest::rstest]
    fn display_roundtrips() {
        let backend = Backend::OpenRouter;
        assert_eq!(backend.to_string().parse::<Backend>().unwrap(), backend);
    }
}
