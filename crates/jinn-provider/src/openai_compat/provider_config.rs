//! Per-backend configuration for OpenAI-compatible providers.
//!
//! Each [`Backend`] variant maps to a [`ProviderConfig`] with the correct
//! default base URL, endpoint paths, and custom headers. Anthropic and Google
//! are intentionally unsupported here - they have their own implementations.

use crate::Backend;

/// Static configuration for an OpenAI-compatible backend.
///
/// Constructed via [`ProviderConfig::from`] or the per-backend constructors.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    /// Human-readable provider name (for error messages and logging).
    pub name: &'static str,
    /// Default base URL for API requests (trailing slash).
    pub default_base_url: &'static str,
    /// Chat completions endpoint path relative to base URL.
    pub chat_endpoint: &'static str,
    /// Models endpoint path relative to base URL.
    pub models_endpoint: &'static str,
    /// Custom HTTP headers to send with every request.
    pub custom_headers: Vec<(String, String)>,
}

impl From<&Backend> for ProviderConfig {
    fn from(backend: &Backend) -> Self {
        match backend {
            // `OpenAiCodex` is not an OpenAI-compatible chat-completions
            // backend; it speaks the Responses protocol and is routed to its
            // own transport before reaching here. The arm exists so the match
            // stays exhaustive, and yields the closest OpenAI shape.
            Backend::OpenAI
            | Backend::OpenAiCodex
            | Backend::Anthropic
            | Backend::Google
            | Backend::AzureOpenAI => Self::openai(),
            Backend::OpenRouter => Self::openrouter(),
            Backend::ZAI => Self::zai(),
            Backend::DeepSeek => Self::deepseek(),
            Backend::Groq => Self::groq(),
            Backend::XAI => Self::xai(),
            Backend::Mistral => Self::mistral(),
            Backend::Cohere => Self::cohere(),
            Backend::HuggingFace => Self::huggingface(),
            Backend::LmStudio => Self::lmstudio(),
            Backend::Phind => Self::phind(),
            Backend::Ollama => Self::ollama(),
            Backend::ElevenLabs => Self::elevenlabs(),
        }
    }
}

impl ProviderConfig {
    #[must_use]
    pub fn openai() -> Self {
        Self {
            name: "OpenAI",
            default_base_url: "https://api.openai.com/v1/",
            chat_endpoint: "chat/completions",
            models_endpoint: "models",
            custom_headers: vec![],
        }
    }

    /// Static configuration for the OpenRouter backend, including app
    /// attribution headers that identify jinn on openrouter.ai.
    #[must_use]
    pub fn openrouter() -> Self {
        Self {
            name: "OpenRouter",
            default_base_url: "https://openrouter.ai/api/v1/",
            chat_endpoint: "chat/completions",
            models_endpoint: "models",
            custom_headers: vec![
                (
                    "X-OpenRouter-Experimental-Metadata".to_owned(),
                    "enabled".to_owned(),
                ),
                (
                    "HTTP-Referer".to_owned(),
                    "https://jaysonlennon.dev".to_owned(),
                ),
                ("X-OpenRouter-Title".to_owned(), "jinn".to_owned()),
                ("X-OpenRouter-Categories".to_owned(), "cli-agent".to_owned()),
            ],
        }
    }

    #[must_use]
    pub fn zai() -> Self {
        Self {
            name: "ZAI",
            default_base_url: "https://api.z.ai/api/coding/paas/v4/",
            chat_endpoint: "chat/completions",
            models_endpoint: "models",
            custom_headers: vec![],
        }
    }

    #[must_use]
    pub fn deepseek() -> Self {
        Self {
            name: "DeepSeek",
            default_base_url: "https://api.deepseek.com/",
            chat_endpoint: "chat/completions",
            models_endpoint: "models",
            custom_headers: vec![],
        }
    }

    #[must_use]
    pub fn groq() -> Self {
        Self {
            name: "Groq",
            default_base_url: "https://api.groq.com/openai/v1/",
            chat_endpoint: "chat/completions",
            models_endpoint: "models",
            custom_headers: vec![],
        }
    }

    #[must_use]
    pub fn xai() -> Self {
        Self {
            name: "XAI",
            default_base_url: "https://api.x.ai/v1/",
            chat_endpoint: "chat/completions",
            models_endpoint: "models",
            custom_headers: vec![],
        }
    }

    #[must_use]
    pub fn mistral() -> Self {
        Self {
            name: "Mistral",
            default_base_url: "https://api.mistral.ai/v1/",
            chat_endpoint: "chat/completions",
            models_endpoint: "models",
            custom_headers: vec![],
        }
    }

    #[must_use]
    pub fn cohere() -> Self {
        Self {
            name: "Cohere",
            default_base_url: "https://api.cohere.ai/v2/",
            chat_endpoint: "chat/completions",
            models_endpoint: "models",
            custom_headers: vec![],
        }
    }

    #[must_use]
    pub fn huggingface() -> Self {
        Self {
            name: "HuggingFace",
            default_base_url: "https://api-inference.huggingface.co/v1/",
            chat_endpoint: "chat/completions",
            models_endpoint: "models",
            custom_headers: vec![],
        }
    }

    #[must_use]
    pub fn lmstudio() -> Self {
        Self {
            name: "LMStudio",
            default_base_url: "http://localhost:1234/v1/",
            chat_endpoint: "chat/completions",
            models_endpoint: "models",
            custom_headers: vec![],
        }
    }

    #[must_use]
    pub fn phind() -> Self {
        Self {
            name: "Phind",
            default_base_url: "https://https.api.phind.com/v1/",
            chat_endpoint: "chat/completions",
            models_endpoint: "models",
            custom_headers: vec![],
        }
    }

    #[must_use]
    pub fn ollama() -> Self {
        Self {
            name: "Ollama",
            default_base_url: "http://localhost:11434/v1/",
            chat_endpoint: "chat/completions",
            models_endpoint: "models",
            custom_headers: vec![],
        }
    }

    #[must_use]
    pub fn elevenlabs() -> Self {
        Self {
            name: "ElevenLabs",
            default_base_url: "https://api.elevenlabs.io/v1/",
            chat_endpoint: "chat/completions",
            models_endpoint: "models",
            custom_headers: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing, reason = "test code")]
    use super::*;

    #[rstest::rstest]
    fn openrouter_config_includes_experimental_metadata_header() {
        let config = ProviderConfig::openrouter();
        assert!(
            config
                .custom_headers
                .iter()
                .any(|(k, v)| k == "X-OpenRouter-Experimental-Metadata" && v == "enabled"),
            "OpenRouter config should include experimental metadata header"
        );
    }

    #[rstest::rstest]
    #[case("HTTP-Referer", "https://jaysonlennon.dev")]
    #[case("X-OpenRouter-Title", "jinn")]
    #[case("X-OpenRouter-Categories", "cli-agent")]
    fn openrouter_config_includes_attribution_header(#[case] header: &str, #[case] value: &str) {
        // Given the OpenRouter provider config.
        let config = ProviderConfig::openrouter();

        // When looking up the attribution header.
        let found = config
            .custom_headers
            .iter()
            .find(|(k, _)| k == header)
            .map(|(_, v)| v.as_str());

        // Then it is present with the exact attributed value.
        assert_eq!(found, Some(value), "missing or wrong value for {header}");
    }

    #[rstest::rstest]
    #[case(&Backend::OpenAI)]
    #[case(&Backend::ZAI)]
    #[case(&Backend::DeepSeek)]
    #[case(&Backend::Groq)]
    #[case(&Backend::XAI)]
    #[case(&Backend::Mistral)]
    #[case(&Backend::Cohere)]
    #[case(&Backend::HuggingFace)]
    #[case(&Backend::LmStudio)]
    #[case(&Backend::Phind)]
    #[case(&Backend::Ollama)]
    #[case(&Backend::ElevenLabs)]
    fn non_openrouter_backends_carry_no_attribution_headers(#[case] backend: &Backend) {
        // Given a non-OpenRouter backend's provider config.
        let config = ProviderConfig::from(backend);

        // When scanning its custom headers for attribution headers.
        let has_attribution = config.custom_headers.iter().any(|(k, _)| {
            k == "HTTP-Referer" || k == "X-OpenRouter-Title" || k == "X-OpenRouter-Categories"
        });

        // Then none are present.
        assert!(
            !has_attribution,
            "{} config should not carry OpenRouter attribution headers",
            config.name
        );
    }

    #[rstest::rstest]
    #[case(&Backend::OpenAI, "OpenAI", "https://api.openai.com/v1/")]
    #[case(&Backend::OpenRouter, "OpenRouter", "https://openrouter.ai/api/v1/")]
    #[case(&Backend::ZAI, "ZAI", "https://api.z.ai/api/coding/paas/v4/")]
    #[case(&Backend::DeepSeek, "DeepSeek", "https://api.deepseek.com/")]
    #[case(&Backend::Groq, "Groq", "https://api.groq.com/openai/v1/")]
    #[case(&Backend::XAI, "XAI", "https://api.x.ai/v1/")]
    #[case(&Backend::Mistral, "Mistral", "https://api.mistral.ai/v1/")]
    #[case(&Backend::Cohere, "Cohere", "https://api.cohere.ai/v2/")]
    #[case(&Backend::HuggingFace, "HuggingFace", "https://api-inference.huggingface.co/v1/")]
    #[case(&Backend::LmStudio, "LMStudio", "http://localhost:1234/v1/")]
    #[case(&Backend::Phind, "Phind", "https://https.api.phind.com/v1/")]
    #[case(&Backend::Ollama, "Ollama", "http://localhost:11434/v1/")]
    #[case(&Backend::ElevenLabs, "ElevenLabs", "https://api.elevenlabs.io/v1/")]
    #[case(&Backend::Anthropic, "OpenAI", "https://api.openai.com/v1/")]
    #[case(&Backend::Google, "OpenAI", "https://api.openai.com/v1/")]
    #[case(&Backend::AzureOpenAI, "OpenAI", "https://api.openai.com/v1/")]
    fn backend_maps_to_correct_config(
        #[case] backend: &Backend,
        #[case] expected_name: &str,
        #[case] expected_url: &str,
    ) {
        let config = ProviderConfig::from(backend);
        assert_eq!(config.name, expected_name);
        assert_eq!(config.default_base_url, expected_url);
    }
}
