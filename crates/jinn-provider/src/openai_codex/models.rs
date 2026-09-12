//! The model catalog available through an OpenAI Codex subscription.
//!
//! Codex does not publish a listing endpoint, so the catalog is carried here
//! and surfaced through the ordinary provider/model mechanisms. Entries appear
//! under their own provider name, keeping subscription-backed models visibly
//! distinct from OpenAI API-key models in the picker.

/// Provider block name that subscription-backed OpenAI models appear under.
pub const CODEX_PROVIDER_NAME: &str = "openai-codex";

/// One model served through a Codex subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodexModel {
    /// Model id sent to the backend and shown in the picker.
    pub id: &'static str,
    /// Maximum context window in tokens.
    pub context_length: u32,
    /// Whether the model accepts image input.
    pub supports_images: bool,
}

/// Context window shared by the general-purpose Codex models.
const STANDARD_CONTEXT: u32 = 272_000;

/// Context window of the smaller, text-only Spark model.
const SPARK_CONTEXT: u32 = 128_000;

/// Every model a Codex subscription can serve, in picker order.
const CODEX_MODELS: &[CodexModel] = &[
    CodexModel {
        id: "gpt-5.6-terra",
        context_length: STANDARD_CONTEXT,
        supports_images: true,
    },
    CodexModel {
        id: "gpt-5.6-luna",
        context_length: STANDARD_CONTEXT,
        supports_images: true,
    },
    CodexModel {
        id: "gpt-5.6-sol",
        context_length: STANDARD_CONTEXT,
        supports_images: true,
    },
    CodexModel {
        id: "gpt-6-astra",
        context_length: STANDARD_CONTEXT,
        supports_images: true,
    },
    CodexModel {
        id: "gpt-5.5",
        context_length: STANDARD_CONTEXT,
        supports_images: true,
    },
    CodexModel {
        id: "gpt-5.3-codex-spark",
        context_length: SPARK_CONTEXT,
        supports_images: false,
    },
];

/// The subscription model catalog.
#[must_use]
pub fn codex_models() -> &'static [CodexModel] {
    CODEX_MODELS
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;
    use std::collections::BTreeSet;

    #[rstest::rstest]
    fn the_catalog_is_not_empty() {
        // Given the built-in catalog.
        // When listing it.
        // Then subscription users have models to pick from.
        assert!(!codex_models().is_empty());
    }

    #[rstest::rstest]
    fn every_model_id_is_unique() {
        // Given the built-in catalog.
        let ids: BTreeSet<&str> = codex_models().iter().map(|model| model.id).collect();

        // When comparing unique ids against the catalog length.
        // Then no id is listed twice, so no picker row can collide.
        assert_eq!(ids.len(), codex_models().len());
    }
}
