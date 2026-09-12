//! Service wrapper for the provider registry.
//!
//! Wraps [`ProviderRegistry`] in a shared, cheap-to-clone container.
//! All clones of [`ProviderRegistryService`] share the same underlying
//! registry via `Arc<RwLock<...>>`. Callers that need multiple operations
//! should use [`read`](Self::read) to acquire a guard and work with
//! `&ProviderRegistry` directly.

use std::sync::Arc;

use error_stack::Report;
use parking_lot::RwLock;
use parking_lot::RwLockReadGuard;

use jinn_provider::ReasoningEffort;

use super::api_keys::ApiKeys;
use super::config::{AliasEntry, ProvidersConfig};
use super::provider_id::ProviderId;
use super::registry::ProviderRegistry;
use super::resolved_provider::ResolvedProvider;
use super::service::{LlmServiceError, LlmServiceFactory};

/// Shared service wrapper for the provider registry.
///
/// Wraps `ProviderRegistry` in an `Arc<RwLock<...>>` so that all clones
/// share the same data. Cloning is cheap - only an Arc refcount bump.
///
/// Follows the project's service wrapper pattern.
#[derive(Debug, Clone)]
pub struct ProviderRegistryService {
    /// The wrapped registry, protected by an [`RwLock`] for shared access.
    inner: Arc<RwLock<ProviderRegistry>>,
}

impl ProviderRegistryService {
    /// Creates a new service wrapper around the given registry.
    #[must_use]
    pub fn new(registry: ProviderRegistry) -> Self {
        Self {
            inner: Arc::new(RwLock::new(registry)),
        }
    }

    /// Returns a read guard to the underlying registry.
    ///
    /// Use this when you need to make multiple calls without
    /// repeated locking (e.g., `filtered_entries`).
    pub fn read(&self) -> RwLockReadGuard<'_, ProviderRegistry> {
        self.inner.read()
    }

    /// Returns all expanded (per-model) providers.
    ///
    /// Acquires a read guard and clones the resolved provider list.
    /// Acceptable for small datasets (typically 2–10 entries).
    #[must_use]
    pub fn providers(&self) -> Vec<ResolvedProvider> {
        self.read().providers().to_vec()
    }

    /// Returns all configured aliases.
    ///
    /// Acquires a read guard and clones the alias list.
    #[must_use]
    pub fn aliases(&self) -> Vec<AliasEntry> {
        self.read().aliases().to_vec()
    }

    /// Looks up a resolved provider by ID.
    ///
    /// Acquires a read guard and clones the entry if found.
    #[must_use]
    pub fn get(&self, id: &ProviderId) -> Option<ResolvedProvider> {
        self.read().get(id).cloned()
    }

    /// Checks whether a provider is available given the resolved API keys.
    #[must_use]
    pub fn is_available(&self, id: &ProviderId, api_keys: &ApiKeys) -> bool {
        self.read().is_available(id, api_keys)
    }

    /// Resolves an alias name to its target resolved provider.
    ///
    /// Acquires a read guard and clones the target entry if found.
    #[must_use]
    pub fn resolve_alias<S>(&self, alias_name: S) -> Option<ResolvedProvider>
    where
        S: AsRef<str>,
    {
        self.read().resolve_alias(alias_name.as_ref()).cloned()
    }

    /// Returns the configured default provider ID, if set and valid.
    #[must_use]
    pub fn default_provider_id(&self) -> Option<ProviderId> {
        self.read().default_provider_id()
    }

    /// Creates an `LlmServiceFactory` for the given provider.
    ///
    /// Acquires a read guard and delegates to the underlying registry.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider is not found or the factory
    /// cannot be built.
    pub fn create_factory(
        &self,
        id: &ProviderId,
        api_keys: &ApiKeys,
        reasoning: Option<ReasoningEffort>,
        endpoint_tag: Option<&str>,
    ) -> Result<Box<dyn LlmServiceFactory>, Report<LlmServiceError>> {
        self.read()
            .create_factory(id, api_keys, reasoning, endpoint_tag)
    }

    /// Merges runtime-discovered models from the model cache into the registry.
    ///
    /// Acquires a write guard. For each cached model not already present,
    /// creates a `ResolvedProvider` with `is_remote: true`.
    pub fn merge_cache(&self, cache: &super::ModelCache) {
        self.inner.write().merge_cache(cache);
    }

    /// Replaces the entire registry contents.
    ///
    /// Used during startup when the init actor builds the registry
    /// from `providers.toml` and needs to swap it into the service.
    pub fn replace(&self, registry: ProviderRegistry) {
        let mut guard = self.inner.write();
        // Carry the test-injected factory override across the swap so it
        // survives the init actor's rebuild-from-config at startup.
        let preserved = guard.factory_override();
        // Subscription wiring is set once at startup; the rebuild would
        // otherwise drop it and take every subscription model offline.
        let auth = guard.subscription_auth();
        *guard = registry;
        if let Some(auth) = auth {
            guard.set_subscription_auth(auth);
        }
        guard.set_factory_override(preserved);
    }

    /// Attaches subscription authentication, registering the built-in
    /// subscription models and making their availability follow stored
    /// credentials.
    pub fn set_subscription_auth(&self, auth: jinn_auth::AuthService) {
        self.inner.write().set_subscription_auth(auth);
    }

    /// Updates the default provider in the config.
    pub fn set_default_provider(&self, name: Option<String>) {
        self.inner.write().set_default_provider(name);
    }

    /// Returns a snapshot of the current config for persistence.
    ///
    /// Acquires a read guard and clones the config. The caller
    /// passes this to [`ConfigStorageService::save`](crate::ConfigStorageService::save).
    #[must_use]
    pub fn config_snapshot(&self) -> ProvidersConfig {
        self.inner.read().config().clone()
    }
}
