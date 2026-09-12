//! Registry behaviour for subscription-backed providers.
//!
//! Subscription models ship with jinn rather than coming from
//! `providers.toml`, and their availability follows stored credentials instead
//! of an API key. These tests pin both, and the guarantee that attaching
//! subscription support changes nothing for API-key providers.

#![allow(clippy::expect_used, clippy::panic, reason = "test code")]

use std::collections::BTreeMap;

use jinn_auth::{AuthProviderId, fake_auth_service, fake_auth_service_logged_in};
use jinn_provider::CODEX_PROVIDER_NAME;

use crate::api_keys::ApiKeys;
use crate::config::{ProviderEntry, ProvidersConfig};
use crate::provider_id::ProviderId;
use crate::registry::ProviderRegistry;

fn empty_config() -> ProvidersConfig {
    ProvidersConfig {
        providers: BTreeMap::new(),
        aliases: vec![],
        default_provider: None,
    }
}

fn config_with_openai_api_key() -> ProvidersConfig {
    let mut providers = BTreeMap::new();
    providers.insert(
        "openai".to_owned(),
        ProviderEntry {
            backend: "openai".to_owned(),
            models: vec!["gpt-4o".to_owned()],
            base_url: None,
            api_key_env: Some("OPENAI_API_KEY".to_owned()),
            requires_key: true,
            extra_body: None,
            context_length: None,
            model_info: Vec::new(),
        },
    );
    ProvidersConfig {
        providers,
        aliases: vec![],
        default_provider: None,
    }
}

fn first_codex_id(registry: &ProviderRegistry) -> ProviderId {
    registry
        .providers()
        .iter()
        .find(|provider| provider.name == CODEX_PROVIDER_NAME)
        .map(|provider| provider.id.clone())
        .expect("a subscription model is registered")
}

#[rstest::rstest]
#[tokio::test]
async fn subscription_models_are_registered_when_auth_is_attached() {
    // Given a registry built from a config that mentions no subscription.
    let (auth, _provider) = fake_auth_service();

    // When subscription authentication is attached.
    let registry = ProviderRegistry::from_config(empty_config())
        .expect("registry")
        .with_subscription_auth(auth);

    // Then subscription models are present without any user configuration.
    assert!(
        registry
            .providers()
            .iter()
            .any(|provider| provider.name == CODEX_PROVIDER_NAME),
        "subscription models must ship with jinn"
    );
}

#[rstest::rstest]
#[tokio::test]
async fn subscription_models_are_unavailable_without_a_stored_login() {
    // Given a registry with subscription support but no stored credential.
    let (auth, _provider) = fake_auth_service();
    let registry = ProviderRegistry::from_config(empty_config())
        .expect("registry")
        .with_subscription_auth(auth);

    // When checking a subscription model's availability.
    let id = first_codex_id(&registry);

    // Then it is not available for selection.
    assert!(!registry.is_available(&id, &ApiKeys::new()));
}

#[rstest::rstest]
#[tokio::test]
async fn subscription_models_become_available_once_credentials_are_stored() {
    // Given a registry whose subscription provider is logged in.
    let (auth, _provider) = fake_auth_service_logged_in(AuthProviderId::OpenAiCodex).await;
    let registry = ProviderRegistry::from_config(empty_config())
        .expect("registry")
        .with_subscription_auth(auth);

    // When checking a subscription model's availability.
    let id = first_codex_id(&registry);

    // Then it is available.
    assert!(registry.is_available(&id, &ApiKeys::new()));
}

#[rstest::rstest]
#[tokio::test]
async fn logging_out_takes_subscription_models_offline_without_a_rebuild() {
    // Given a registry whose subscription provider is logged in.
    let (auth, _provider) = fake_auth_service_logged_in(AuthProviderId::OpenAiCodex).await;
    let registry = ProviderRegistry::from_config(empty_config())
        .expect("registry")
        .with_subscription_auth(auth.clone());
    let id = first_codex_id(&registry);

    // When the user logs out.
    auth.logout(AuthProviderId::OpenAiCodex)
        .await
        .expect("logout succeeds");

    // Then the same registry instance already reports the model unavailable.
    assert!(!registry.is_available(&id, &ApiKeys::new()));
}

#[rstest::rstest]
#[tokio::test]
async fn a_subscription_request_without_a_login_cannot_build_a_factory() {
    // Given a registry with subscription support but no stored credential.
    let (auth, _provider) = fake_auth_service();
    let registry = ProviderRegistry::from_config(empty_config())
        .expect("registry")
        .with_subscription_auth(auth);
    let id = first_codex_id(&registry);

    // When a request tries to build a factory for a subscription model.
    let result = registry.create_factory(&id, &ApiKeys::new(), None, None);

    // Then it fails rather than falling back to a separately billed route.
    assert!(
        result.is_err(),
        "a subscription request must not fall back to API-key access"
    );
}

#[rstest::rstest]
#[tokio::test]
async fn a_logged_in_subscription_model_builds_its_own_factory() {
    // Given a registry whose subscription provider is logged in.
    let (auth, _provider) = fake_auth_service_logged_in(AuthProviderId::OpenAiCodex).await;
    let registry = ProviderRegistry::from_config(empty_config())
        .expect("registry")
        .with_subscription_auth(auth);
    let id = first_codex_id(&registry);

    // When a request builds a factory for it.
    let factory = registry
        .create_factory(&id, &ApiKeys::new(), None, None)
        .expect("factory");

    // Then the factory belongs to the subscription provider.
    assert_eq!(factory.name(), CODEX_PROVIDER_NAME);
}

#[rstest::rstest]
#[tokio::test]
async fn an_api_key_provider_stays_unavailable_without_its_key() {
    // Given a registry with both an API-key provider and subscription support.
    let (auth, _provider) = fake_auth_service_logged_in(AuthProviderId::OpenAiCodex).await;
    let registry = ProviderRegistry::from_config(config_with_openai_api_key())
        .expect("registry")
        .with_subscription_auth(auth);

    // When checking the API-key provider with no key resolved.
    let id = ProviderId::new("openai/gpt-4o".to_owned());

    // Then a subscription login does not make it available.
    assert!(
        !registry.is_available(&id, &ApiKeys::new()),
        "a subscription login must not stand in for an API key"
    );
}

#[rstest::rstest]
#[tokio::test]
async fn an_api_key_provider_is_available_with_its_key() {
    // Given a registry with both an API-key provider and subscription support.
    let (auth, _provider) = fake_auth_service();
    let registry = ProviderRegistry::from_config(config_with_openai_api_key())
        .expect("registry")
        .with_subscription_auth(auth);
    let mut api_keys = ApiKeys::new();
    api_keys.insert("OPENAI_API_KEY".to_owned(), "sk-test".to_owned());

    // When checking the API-key provider with its key resolved.
    let id = ProviderId::new("openai/gpt-4o".to_owned());

    // Then its existing behaviour is unchanged.
    assert!(registry.is_available(&id, &api_keys));
}

#[rstest::rstest]
#[tokio::test]
async fn rebuilding_configuration_retains_subscription_availability() {
    // Given a shared registry wired to a stored subscription account.
    let (auth, _provider) = fake_auth_service_logged_in(AuthProviderId::OpenAiCodex).await;
    let registry = crate::ProviderRegistryService::new(
        ProviderRegistry::from_config(empty_config())
            .expect("registry")
            .with_subscription_auth(auth),
    );
    let id = first_codex_id(&registry.read());

    // When startup or configuration reload replaces its configuration.
    registry.replace(ProviderRegistry::from_config(empty_config()).expect("replacement"));

    // Then the subscription model remains available without rewiring authentication.
    assert!(registry.is_available(&id, &ApiKeys::new()));
}

#[rstest::rstest]
#[tokio::test]
async fn a_user_configured_block_keeps_its_own_definition() {
    // Given a user block that already claims a subscription model id.
    let mut providers = BTreeMap::new();
    providers.insert(
        CODEX_PROVIDER_NAME.to_owned(),
        ProviderEntry {
            backend: "openai".to_owned(),
            models: vec!["gpt-5.5".to_owned()],
            base_url: None,
            api_key_env: Some("OPENAI_API_KEY".to_owned()),
            requires_key: true,
            extra_body: None,
            context_length: None,
            model_info: Vec::new(),
        },
    );
    let config = ProvidersConfig {
        providers,
        aliases: vec![],
        default_provider: None,
    };
    let (auth, _provider) = fake_auth_service();

    // When subscription support is attached.
    let registry = ProviderRegistry::from_config(config)
        .expect("registry")
        .with_subscription_auth(auth);

    // Then the user's definition of that id survives.
    let entry = registry
        .get(&ProviderId::new(format!("{CODEX_PROVIDER_NAME}/gpt-5.5")))
        .expect("entry");
    assert_eq!(entry.backend, "openai");
}
