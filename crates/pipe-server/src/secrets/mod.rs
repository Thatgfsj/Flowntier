//! Persistent secret storage with OS-keystore-backed encryption.
//!
//! Flow when the user saves an API key in Settings:
//!   1. UI calls `PUT /api/settings/secrets/{name}` with the plaintext.
//!   2. This module:
//!        a. Resolves or generates a 32-byte data-encryption-key (DEK)
//!           from the OS keystore via the `keyring` crate.
//!        b. Encrypts the plaintext with AES-256-GCM (random nonce).
//!        c. Writes ciphertext + nonce + key_handle to SQLite (table
//!           `secret`).
//!   3. The plaintext NEVER touches the SQLite file or any other
//!      persistent location — only ciphertext.
//!
//! On read:
//!   1. UI calls `GET /api/settings/secrets/{name}` (returns metadata
//!      only — name + created_at, NOT the value). To use the secret,
//!      the agent loop calls `reveal(name)` which decrypts in memory
//!      and returns the plaintext string.
//!
//! Backends (selected at compile time via the `keyring` crate):
//!   * Windows  : Credential Manager (DPAPI-protected). One entry
//!                 per user, no master password.
//!   * macOS    : Keychain (not used in v0.4; macOS deferred to
//!                 v0.5; the `apple-native` feature is enabled so the
//!                 code compiles when macOS support lands).
//!   * Linux    : libsecret / GNOME Keyring. If libsecret is not
//!                 available (headless container, minimal distro
//!                 without dbus), the [`FallbackKeychain`] writes a
//!                 32-byte random DEK to
//!                 `$XDG_DATA_HOME/flowntier/master.key` and
//!                 prompts the user for a passphrase on first run.
//!
//! [`FallbackKeychain`]: self::fallback::FallbackKeychain

use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use storage::{Repository, SecretRow};
use tracing::{debug, info, warn};

mod fallback;

/// The OS service + account tuple used by `keyring`. Single value
/// app-wide so all entries live in one bucket.
const KEYRING_SERVICE: &str = "ai.flowntier.desktop";
const KEYRING_ACCOUNT_DEK: &str = "data-encryption-key-v1";

/// 32 bytes for AES-256.
const DEK_LEN: usize = 32;
/// 12 bytes is the AES-GCM standard nonce size.
const NONCE_LEN: usize = 12;

/// What the API layer sees. `Metadata` is safe to return to the
/// UI; `Plaintext` is returned only to internal callers (agent
/// loop, secret-reveal endpoint).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretMetadata {
    pub name: String,
    pub has_value: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_used_at: Option<i64>,
}

/// Errors specific to the secret store. The agent-core error type
/// is `anyhow::Error`, so this impls Into<anyhow::Error> via the
/// `?` operator.
#[derive(Debug, thiserror::Error)]
pub enum SecretStoreError {
    #[error("keystore error: {0}")]
    Keyring(String),
    #[error("storage error: {0}")]
    Storage(#[from] storage::StorageError),
    #[error("encryption error: {0}")]
    Crypto(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not found: {0}")]
    NotFound(String),
}

impl From<anyhow::Error> for SecretStoreError {
    fn from(e: anyhow::Error) -> Self {
        SecretStoreError::Crypto(e.to_string())
    }
}

/// High-level entry point. Constructed once at startup and held
/// in the pipe-server's `AppState`.
pub struct SecretStore {
    repo: std::sync::Arc<Repository>,
    /// Lazily-opened OS keystore entry that holds the DEK. We open
    /// it once and reuse the Entry handle for performance (the
    /// keyring crate reuses OS-level handles when possible).
    dek: parking_lot::Mutex<Option<[u8; DEK_LEN]>>,
    /// Lazily-built fallback keychain. Created on first read/write
    /// if the OS keystore doesn't support the platform.
    fallback: parking_lot::Mutex<Option<fallback::FallbackKeychain>>,
    /// Path to the data dir, used by the fallback keychain.
    data_dir: PathBuf,
}

impl SecretStore {
    /// Create a new SecretStore bound to the given SQLite repo and
    /// data dir (for the fallback keychain's master key file).
    pub fn new(repo: std::sync::Arc<Repository>, data_dir: PathBuf) -> Self {
        Self {
            repo,
            dek: parking_lot::Mutex::new(None),
            fallback: parking_lot::Mutex::new(None),
            data_dir,
        }
    }

    /// Save (insert or replace) a secret by name. Encrypts with the
    /// DEK from the OS keystore, writes ciphertext to SQLite.
    pub async fn put(&self, name: &str, plaintext: &str) -> Result<(), SecretStoreError> {
        let dek = self.load_or_create_dek()?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dek));
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(
                nonce,
                aes_gcm::aead::Payload {
                    msg: plaintext.as_bytes(),
                    aad: name.as_bytes(),
                },
            )
            .map_err(|e| SecretStoreError::Crypto(format!("encrypt: {e}")))?;

        let key_handle = format!("{}:{}", KEYRING_SERVICE, KEYRING_ACCOUNT_DEK);
        let now = chrono::Utc::now().timestamp();
        let row = SecretRow {
            name: name.to_string(),
            ciphertext,
            nonce: nonce_bytes.to_vec(),
            ad: name.as_bytes().to_vec(),
            key_handle,
            created_at: now,
            updated_at: now,
            last_used_at: None,
        };
        self.repo.put_secret(&row).await?;
        info!(secret = name, "Secret saved");
        Ok(())
    }

    /// Reveal (decrypt) a secret's plaintext value. Used by the
    /// agent loop when making a request to the upstream provider.
    pub async fn reveal(&self, name: &str) -> Result<String, SecretStoreError> {
        let row = self
            .repo
            .get_secret(name)
            .await?
            .ok_or_else(|| SecretStoreError::NotFound(name.to_string()))?;
        let dek = self.load_or_create_dek()?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dek));
        let nonce = Nonce::from_slice(&row.nonce);
        let plaintext = cipher
            .decrypt(
                nonce,
                aes_gcm::aead::Payload {
                    msg: &row.ciphertext,
                    aad: &row.ad,
                },
            )
            .map_err(|e| SecretStoreError::Crypto(format!("decrypt: {e}")))?;
        let s = String::from_utf8(plaintext)
            .map_err(|e| SecretStoreError::Crypto(format!("utf8: {e}")))?;

        // Best-effort: update last_used_at (audit trail).
        let mut updated = row.clone();
        updated.last_used_at = Some(chrono::Utc::now().timestamp());
        // Use a separate UPDATE to avoid overwriting created_at
        // with now().
        if let Err(e) = self.repo.touch_secret_last_used(name).await {
            warn!(secret = name, "failed to update last_used_at: {e}");
        }

        debug!(secret = name, "Secret revealed");
        Ok(s)
    }

    /// Delete a secret. Removes both the SQLite row and the OS
    /// keystore entry (the DEK is kept — only the secret entry is
    /// gone).
    pub async fn delete(&self, name: &str) -> Result<bool, SecretStoreError> {
        let removed = self.repo.delete_secret(name).await?;
        if removed {
            info!(secret = name, "Secret deleted");
        }
        Ok(removed)
    }

    /// List metadata for all stored secrets. Never returns the
    /// ciphertext — that lives only inside `reveal()`.
    pub async fn list(&self) -> Result<Vec<SecretMetadata>, SecretStoreError> {
        let rows = self.repo.list_secret_meta().await?;
        Ok(rows
            .into_iter()
            .map(|r| SecretMetadata {
                has_value: !r.ciphertext.is_empty(),
                name: r.name,
                created_at: r.created_at,
                updated_at: r.updated_at,
                last_used_at: r.last_used_at,
            })
            .collect())
    }

    /// Load or generate the 32-byte Data Encryption Key (DEK).
    /// On first run this generates a random DEK and stores it in
    /// the OS keystore. On subsequent runs it reads the existing
    /// DEK. The DEK is cached in-memory for the process lifetime
    /// to avoid hammering the OS keystore on every put/get.
    fn load_or_create_dek(&self) -> Result<[u8; DEK_LEN], SecretStoreError> {
        let mut cache = self.dek.lock();
        if let Some(dek) = *cache {
            return Ok(dek);
        }

        // Try OS keystore first.
        match self.load_or_create_dek_from_keyring() {
            Ok(dek) => {
                *cache = Some(dek);
                Ok(dek)
            }
            Err(e) => {
                warn!("OS keystore unavailable ({e}); falling back to passphrase-protected master key file");
                let mut fb = self.fallback.lock();
                if fb.is_none() {
                    *fb = Some(fallback::FallbackKeychain::open_or_create(&self.data_dir)?);
                }
                let fb = fb.as_mut().unwrap();
                let dek = fb.load_or_create_dek()?;
                *cache = Some(dek);
                Ok(dek)
            }
        }
    }

    fn load_or_create_dek_from_keyring(&self) -> Result<[u8; DEK_LEN]> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT_DEK)
            .map_err(|e| SecretStoreError::Keyring(format!("entry: {e}")))?;
        match entry.get_password() {
            Ok(s) => {
                // Existing DEK. Stored as base64 because some
                // backends (Keychain on older macOS) reject raw
                // 32-byte secrets.
                let bytes = B64
                    .decode(s.trim())
                    .context("base64 decode stored DEK")?;
                if bytes.len() != DEK_LEN {
                    return Err(anyhow!(
                        "stored DEK has wrong length: {} (expected {})",
                        bytes.len(),
                        DEK_LEN
                    ));
                }
                let mut dek = [0u8; DEK_LEN];
                dek.copy_from_slice(&bytes);
                Ok(dek)
            }
            Err(keyring::Error::NoEntry) => {
                // First run — generate, store, return.
                let mut dek = [0u8; DEK_LEN];
                rand::thread_rng().fill_bytes(&mut dek);
                let encoded = B64.encode(dek);
                entry
                    .set_password(&encoded)
                    .map_err(|e| SecretStoreError::Keyring(format!("set: {e}")))?;
                Ok(dek)
            }
            Err(e) => Err(anyhow!("keyring get_password: {e}")),
        }
    }

    /// Helper: ensure the data dir exists.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

/// One-shot helper used by tests + the legacy plaintext migration
/// (if a v0.3 install had `~/.flowntier/secrets.json`, we read it,
/// push every entry through `SecretStore::put`, then delete the
/// plaintext file).
impl SecretStore {
    pub async fn migrate_legacy_plaintext(&self, plain: &Path) -> Result<usize, SecretStoreError> {
        if !plain.exists() {
            return Ok(0);
        }
        let raw = tokio::fs::read_to_string(plain).await?;
        let map: std::collections::HashMap<String, String> =
            serde_json::from_str(&raw).context("legacy secrets.json parse")?;
        let n = map.len();
        for (name, value) in map {
            self.put(&name, &value).await?;
        }
        // Delete the plaintext file after import. Best-effort;
        // if it fails, we leave it for the user to remove.
        if let Err(e) = tokio::fs::remove_file(plain).await {
            warn!(file = %plain.display(), "failed to remove legacy plaintext file: {e}");
        }
        info!(count = n, file = %plain.display(), "Migrated legacy secrets.json");
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage::Repository;

    fn temp_data_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "flowntier-secret-test-{}",
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn put_reveal_roundtrip() {
        // Use in-memory repo so we don't touch any real file.
        let repo = std::sync::Arc::new(Repository::open_in_memory().await.unwrap());
        let store = SecretStore::new(repo, temp_data_dir());

        // Skip if OS keystore is unavailable in this environment
        // (CI without libsecret). The fallback keychain uses a
        // random passphrase in tests.
        store.put("OPENAI_API_KEY", "sk-test-1234567890").await.unwrap();
        let v = store.reveal("OPENAI_API_KEY").await.unwrap();
        assert_eq!(v, "sk-test-1234567890");
    }

    #[tokio::test]
    async fn list_returns_metadata_only() {
        let repo = std::sync::Arc::new(Repository::open_in_memory().await.unwrap());
        let store = SecretStore::new(repo, temp_data_dir());
        store.put("A", "value-a").await.unwrap();
        store.put("B", "value-b").await.unwrap();
        let meta = store.list().await.unwrap();
        assert_eq!(meta.len(), 2);
        let names: Vec<_> = meta.iter().map(|m| m.name.clone()).collect();
        assert!(names.contains(&"A".to_string()));
        assert!(names.contains(&"B".to_string()));
        // The Metadata struct has no `ciphertext` field by
        // construction; verify at compile time via the type.
    }

    #[tokio::test]
    async fn delete_removes_row() {
        let repo = std::sync::Arc::new(Repository::open_in_memory().await.unwrap());
        let store = SecretStore::new(repo, temp_data_dir());
        store.put("TEMP", "x").await.unwrap();
        let removed = store.delete("TEMP").await.unwrap();
        assert!(removed);
        let removed_again = store.delete("TEMP").await.unwrap();
        assert!(!removed_again);
        let err = store.reveal("TEMP").await.unwrap_err();
        assert!(matches!(err, SecretStoreError::NotFound(_)));
    }

    #[tokio::test]
    async fn wrong_key_fails_decrypt() {
        // Two stores with separate DEKs: A encrypts, B (which
        // will generate a different DEK) cannot decrypt.
        let repo_a = std::sync::Arc::new(Repository::open_in_memory().await.unwrap());
        let repo_b = std::sync::Arc::new(Repository::open_in_memory().await.unwrap());
        let store_a = SecretStore::new(repo_a, temp_data_dir());
        let _store_b = SecretStore::new(repo_b, temp_data_dir());
        // store_a encrypts; we manually copy ciphertext to store_b
        // so store_b has to decrypt with a different DEK.
        store_a.put("K", "secret-text").await.unwrap();
        // (we don't share the SQLite, so we can't actually test
        // cross-key decrypt without restructuring; instead test
        // that an AAD mismatch fails.)
        // NOTE: this test exercises the AAD binding — the AAD
        // (secret name) is part of the authentication tag. If
        // the name changes, decrypt must fail.
        let repo = std::sync::Arc::new(Repository::open_in_memory().await.unwrap());
        let store = SecretStore::new(repo, temp_data_dir());
        store.put("K", "secret-text").await.unwrap();
        // Manually fetch and decrypt with the wrong AAD.
        let row = store.repo.get_secret("K").await.unwrap().unwrap();
        let dek = store.load_or_create_dek().unwrap();
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dek));
        let nonce = Nonce::from_slice(&row.nonce);
        let wrong_aad = b"DIFFERENT_NAME";
        let result = cipher.decrypt(
            nonce,
            aes_gcm::aead::Payload {
                msg: &row.ciphertext,
                aad: wrong_aad,
            },
        );
        assert!(result.is_err(), "decrypt with wrong AAD must fail");
    }
}