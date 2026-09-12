//! Persistence for subscription credentials.
//!
//! Credentials live in their own user-level file, separate from the
//! user-editable provider configuration, so editing or sharing
//! `providers.toml` can never expose a token. The file holds at most one
//! credential per provider and is shared by every jinn project on the machine.
//!
//! The filesystem implementation writes owner-only (`0600`) files into an
//! owner-only (`0700`) directory, and replaces them atomically so a crash
//! mid-write cannot leave a half-written credential behind.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use error_stack::{Report, ResultExt as _};
use wherror::Error;

use crate::credential::StoredCredential;
use crate::provider_id::AuthProviderId;

/// Raised when credential storage cannot be read or written.
#[derive(Debug, Error)]
#[error("credential storage failure")]
pub struct CredentialStoreError;

/// App-owned credential storage, keyed by [`AuthProviderId`].
///
/// One credential per provider: logging in again replaces the stored record
/// rather than adding a second account.
#[async_trait::async_trait]
pub trait CredentialStore: Send + Sync + std::fmt::Debug {
    /// Human-readable name for diagnostics.
    fn name(&self) -> &'static str;

    /// Holds the storage mutation boundary across a refresh and its persistence.
    ///
    /// # Errors
    /// Returns an error if the exclusive storage lock cannot be acquired.
    async fn transaction(
        &self,
    ) -> Result<Box<dyn CredentialTransaction>, Report<CredentialStoreError>>;

    /// Reads the stored credential, which may be expired.
    ///
    /// # Errors
    ///
    /// Returns an error if the backing storage cannot be read.
    async fn read(
        &self,
        provider: AuthProviderId,
    ) -> Result<Option<StoredCredential>, Report<CredentialStoreError>>;

    /// Lists the providers that currently have a stored credential.
    ///
    /// # Errors
    ///
    /// Returns an error if the backing storage cannot be read.
    async fn list(&self) -> Result<Vec<AuthProviderId>, Report<CredentialStoreError>>;

    /// Writes (or replaces) the credential for one provider.
    ///
    /// # Errors
    ///
    /// Returns an error if the backing storage cannot be written.
    async fn write(
        &self,
        provider: AuthProviderId,
        credential: StoredCredential,
    ) -> Result<(), Report<CredentialStoreError>>;

    /// Removes the credential for one provider. Removing an absent credential
    /// succeeds.
    ///
    /// # Errors
    ///
    /// Returns an error if the backing storage cannot be written.
    async fn delete(&self, provider: AuthProviderId) -> Result<(), Report<CredentialStoreError>>;
}

/// An exclusive credential operation. Dropping it releases the lock, including
/// when an asynchronous refresh is cancelled.
pub trait CredentialTransaction: Send {
    /// Reads the latest credential under the lock.
    ///
    /// # Errors
    /// Returns an error if storage cannot be read.
    fn read(
        &self,
        provider: AuthProviderId,
    ) -> Result<Option<StoredCredential>, Report<CredentialStoreError>>;

    /// Atomically replaces a credential under the lock.
    ///
    /// # Errors
    /// Returns an error if persistence fails.
    fn write(
        &mut self,
        provider: AuthProviderId,
        credential: StoredCredential,
    ) -> Result<(), Report<CredentialStoreError>>;
}

/// Shared handle to a [`CredentialStore`], following the service-wrapper pattern.
#[derive(Clone, derive_more::Debug)]
pub struct CredentialStoreService {
    #[debug("CredentialStore<{}>", self.store.name())]
    store: Arc<dyn CredentialStore>,
}

impl CredentialStoreService {
    /// Wraps a store for shared ownership.
    #[must_use]
    pub fn new(store: Arc<dyn CredentialStore>) -> Self {
        Self { store }
    }

    /// Borrows the underlying store.
    #[must_use]
    pub fn inner(&self) -> &Arc<dyn CredentialStore> {
        &self.store
    }
}

impl std::ops::Deref for CredentialStoreService {
    type Target = dyn CredentialStore;

    fn deref(&self) -> &Self::Target {
        self.store.as_ref()
    }
}

/// The on-disk document: a provider-id keyed map of credentials.
type CredentialDocument = BTreeMap<String, StoredCredential>;

/// Credential storage backed by a single JSON file.
///
/// The file is created owner-only and rewritten atomically. Writes are
/// serialized in-process so a login racing a token refresh cannot interleave
/// read-modify-write cycles.
#[derive(Debug)]
pub struct FilesystemCredentialStore {
    path: PathBuf,
}

impl FilesystemCredentialStore {
    /// Creates a store backed by `path`. The file and its parent directory are
    /// created on first write.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn load(&self) -> Result<CredentialDocument, Report<CredentialStoreError>> {
        load_document(&self.path)
    }

    fn store(&self, document: &CredentialDocument) -> Result<(), Report<CredentialStoreError>> {
        write_document(&self.path, document)
    }
}

/// Reads the credential document, treating a missing file as empty.
fn load_document(path: &Path) -> Result<CredentialDocument, Report<CredentialStoreError>> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(err) => {
            return Err(Report::new(err)
                .change_context(CredentialStoreError)
                .attach("failed to read credential storage"));
        }
    };
    if contents.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    serde_json::from_str(&contents)
        .change_context(CredentialStoreError)
        .attach("credential storage is not valid JSON")
}

/// Writes the credential document atomically with owner-only permissions.
fn write_document(
    path: &Path,
    document: &CredentialDocument,
) -> Result<(), Report<CredentialStoreError>> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .change_context(CredentialStoreError)
        .attach("failed to create credential storage directory")?;
    restrict_dir_permissions(parent)?;

    let serialized = serde_json::to_string_pretty(document)
        .change_context(CredentialStoreError)
        .attach("failed to serialize credentials")?;

    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).change_context(CredentialStoreError)?;
    {
        use std::io::Write as _;
        temporary
            .write_all(serialized.as_bytes())
            .change_context(CredentialStoreError)?;
        temporary
            .as_file()
            .sync_all()
            .change_context(CredentialStoreError)?;
    }
    temporary
        .persist(path)
        .change_context(CredentialStoreError)
        .attach("failed to replace credential storage")?;
    Ok(())
}

/// Tightens the credential directory before any secrets are written.
#[cfg(unix)]
fn restrict_dir_permissions(dir: &Path) -> Result<(), Report<CredentialStoreError>> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .change_context(CredentialStoreError)
}

#[cfg(not(unix))]
fn restrict_dir_permissions(_dir: &Path) -> Result<(), Report<CredentialStoreError>> {
    Ok(())
}

#[async_trait::async_trait]
impl CredentialStore for FilesystemCredentialStore {
    async fn transaction(
        &self,
    ) -> Result<Box<dyn CredentialTransaction>, Report<CredentialStoreError>> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).change_context(CredentialStoreError)?;
        restrict_dir_permissions(parent)?;
        let lock_path = self.path.with_extension("lock");
        let file = {
            let mut options = std::fs::OpenOptions::new();
            options.read(true).write(true).create(true).truncate(false);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt as _;
                options.mode(0o600);
            }
            options
                .open(lock_path)
                .change_context(CredentialStoreError)?
        };
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            match file.try_lock() {
                Ok(()) => {
                    return Ok(Box::new(FilesystemTransaction {
                        path: self.path.clone(),
                        _lock: file,
                    }));
                }
                Err(std::fs::TryLockError::WouldBlock)
                    if tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                Err(_) => {
                    return Err(Report::new(CredentialStoreError)
                        .attach("could not lock credential storage"));
                }
            }
        }
    }

    fn name(&self) -> &'static str {
        "filesystem"
    }

    async fn read(
        &self,
        provider: AuthProviderId,
    ) -> Result<Option<StoredCredential>, Report<CredentialStoreError>> {
        Ok(self.load()?.remove(provider.as_str()))
    }

    async fn list(&self) -> Result<Vec<AuthProviderId>, Report<CredentialStoreError>> {
        Ok(self
            .load()?
            .keys()
            .filter_map(|key| key.parse::<AuthProviderId>().ok())
            .collect())
    }

    async fn write(
        &self,
        provider: AuthProviderId,
        credential: StoredCredential,
    ) -> Result<(), Report<CredentialStoreError>> {
        self.transaction().await?.write(provider, credential)
    }

    async fn delete(&self, provider: AuthProviderId) -> Result<(), Report<CredentialStoreError>> {
        let _guard = self.transaction().await?;
        let mut document = self.load()?;
        if document.remove(provider.as_str()).is_none() {
            return Ok(());
        }
        self.store(&document)
    }
}

/// In-memory credential storage for tests.
#[derive(Debug, Default)]
pub struct InMemoryCredentialStore {
    entries: Arc<tokio::sync::Mutex<BTreeMap<AuthProviderId, StoredCredential>>>,
}

impl InMemoryCredentialStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl CredentialStore for InMemoryCredentialStore {
    async fn transaction(
        &self,
    ) -> Result<Box<dyn CredentialTransaction>, Report<CredentialStoreError>> {
        Ok(Box::new(MemoryTransaction {
            entries: self.entries.clone().lock_owned().await,
        }))
    }

    fn name(&self) -> &'static str {
        "in-memory"
    }

    async fn read(
        &self,
        provider: AuthProviderId,
    ) -> Result<Option<StoredCredential>, Report<CredentialStoreError>> {
        Ok(self.entries.lock().await.get(&provider).cloned())
    }

    async fn list(&self) -> Result<Vec<AuthProviderId>, Report<CredentialStoreError>> {
        Ok(self.entries.lock().await.keys().copied().collect())
    }

    async fn write(
        &self,
        provider: AuthProviderId,
        credential: StoredCredential,
    ) -> Result<(), Report<CredentialStoreError>> {
        self.entries.lock().await.insert(provider, credential);
        Ok(())
    }

    async fn delete(&self, provider: AuthProviderId) -> Result<(), Report<CredentialStoreError>> {
        self.entries.lock().await.remove(&provider);
        Ok(())
    }
}

struct FilesystemTransaction {
    path: PathBuf,
    _lock: std::fs::File,
}

impl CredentialTransaction for FilesystemTransaction {
    fn read(
        &self,
        provider: AuthProviderId,
    ) -> Result<Option<StoredCredential>, Report<CredentialStoreError>> {
        Ok(load_document(&self.path)?.remove(provider.as_str()))
    }

    fn write(
        &mut self,
        provider: AuthProviderId,
        credential: StoredCredential,
    ) -> Result<(), Report<CredentialStoreError>> {
        let mut document = load_document(&self.path)?;
        document.insert(provider.as_str().to_owned(), credential);
        write_document(&self.path, &document)
    }
}

struct MemoryTransaction {
    entries: tokio::sync::OwnedMutexGuard<BTreeMap<AuthProviderId, StoredCredential>>,
}

impl CredentialTransaction for MemoryTransaction {
    fn read(
        &self,
        provider: AuthProviderId,
    ) -> Result<Option<StoredCredential>, Report<CredentialStoreError>> {
        Ok(self.entries.get(&provider).cloned())
    }

    fn write(
        &mut self,
        provider: AuthProviderId,
        credential: StoredCredential,
    ) -> Result<(), Report<CredentialStoreError>> {
        self.entries.insert(provider, credential);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;
    use crate::credential::OAuthCredential;

    fn credential(access: &str) -> StoredCredential {
        StoredCredential::Oauth(OAuthCredential {
            access_token: access.to_owned(),
            refresh_token: "refresh".to_owned(),
            expires_at_ms: 10_000,
            account_id: "acct-1".to_owned(),
        })
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn reading_an_absent_provider_yields_nothing() {
        // Given an empty store.
        let dir = tempfile::tempdir().expect("temp dir");
        let store = FilesystemCredentialStore::new(dir.path().join("auth.json"));

        // When reading a provider that was never written.
        let found = store
            .read(AuthProviderId::OpenAiCodex)
            .await
            .expect("read succeeds");

        // Then nothing comes back.
        assert!(found.is_none());
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn a_written_credential_survives_a_new_store_instance() {
        // Given a credential written through one store instance.
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("auth.json");
        FilesystemCredentialStore::new(path.clone())
            .write(AuthProviderId::OpenAiCodex, credential("first"))
            .await
            .expect("write succeeds");

        // When reading through a fresh instance, as a restarted app would.
        let found = FilesystemCredentialStore::new(path)
            .read(AuthProviderId::OpenAiCodex)
            .await
            .expect("read succeeds");

        // Then the stored credential is returned.
        assert_eq!(found, Some(credential("first")));
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn writing_again_replaces_the_previous_credential() {
        // Given a store holding one credential.
        let dir = tempfile::tempdir().expect("temp dir");
        let store = FilesystemCredentialStore::new(dir.path().join("auth.json"));
        store
            .write(AuthProviderId::OpenAiCodex, credential("first"))
            .await
            .expect("first write");

        // When writing a second credential for the same provider.
        store
            .write(AuthProviderId::OpenAiCodex, credential("second"))
            .await
            .expect("second write");

        // Then only the newer credential remains.
        let found = store
            .read(AuthProviderId::OpenAiCodex)
            .await
            .expect("read succeeds");
        assert_eq!(found, Some(credential("second")));
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn delete_removes_the_stored_credential() {
        // Given a store holding one credential.
        let dir = tempfile::tempdir().expect("temp dir");
        let store = FilesystemCredentialStore::new(dir.path().join("auth.json"));
        store
            .write(AuthProviderId::OpenAiCodex, credential("first"))
            .await
            .expect("write succeeds");

        // When deleting it.
        store
            .delete(AuthProviderId::OpenAiCodex)
            .await
            .expect("delete succeeds");

        // Then the provider no longer has a credential.
        let found = store
            .read(AuthProviderId::OpenAiCodex)
            .await
            .expect("read succeeds");
        assert!(found.is_none());
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn deleting_an_absent_credential_succeeds() {
        // Given an empty store.
        let dir = tempfile::tempdir().expect("temp dir");
        let store = FilesystemCredentialStore::new(dir.path().join("auth.json"));

        // When deleting a provider that has no credential.
        let result = store.delete(AuthProviderId::OpenAiCodex).await;

        // Then the operation succeeds.
        assert!(result.is_ok(), "deleting an absent credential is a no-op");
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn list_reports_providers_that_have_credentials() {
        // Given a store holding one credential.
        let dir = tempfile::tempdir().expect("temp dir");
        let store = FilesystemCredentialStore::new(dir.path().join("auth.json"));
        store
            .write(AuthProviderId::OpenAiCodex, credential("first"))
            .await
            .expect("write succeeds");

        // When listing stored providers.
        let stored = store.list().await.expect("list succeeds");

        // Then the written provider is listed.
        assert_eq!(stored, vec![AuthProviderId::OpenAiCodex]);
    }

    #[cfg(unix)]
    #[rstest::rstest]
    #[tokio::test]
    async fn the_credential_file_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt as _;

        // Given a credential written to disk.
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested").join("auth.json");
        FilesystemCredentialStore::new(path.clone())
            .write(AuthProviderId::OpenAiCodex, credential("first"))
            .await
            .expect("write succeeds");

        // When inspecting the file mode.
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;

        // Then no group or world bits are set.
        assert_eq!(mode, 0o600, "credential file must be owner-only");
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn unreadable_storage_surfaces_an_error() {
        // Given a credential file containing invalid JSON.
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("auth.json");
        std::fs::write(&path, "not json at all").expect("seed file");

        // When reading a credential.
        let result = FilesystemCredentialStore::new(path)
            .read(AuthProviderId::OpenAiCodex)
            .await;

        // Then the failure is reported rather than silently ignored.
        assert!(result.is_err(), "corrupt storage must surface an error");
    }
}
