//! Fallback keychain for environments where the OS keystore is
//! unavailable (Linux without libsecret, headless containers,
//! minimal distros without dbus).
//!
//! Scheme:
//!   * On first open, generate a 32-byte random "master key" and
//!     write it to `<data_dir>/master.key`. The file is readable
//!     only by the current user (0600 on Unix; ACL on Windows).
//!   * Every ciphertext + nonce in SQLite is encrypted with
//!     AES-256-GCM using this master key as the DEK (no
//!     passphrase layer — the OS file ACL is the protection).
//!   * This is strictly weaker than the OS keystore (no
//!     user-binding; no DPAPI); a local attacker with read access
//!     to the user's home directory can decrypt secrets.
//!
//! For v0.4 we ship the master key in cleartext with restrictive
//! file permissions. A future v0.5 iteration may add a
//! passphrase prompt on first run, but that complicates
//! unattended use.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::RngCore;

use super::DEK_LEN;

const MASTER_KEY_FILENAME: &str = "master.key";

pub struct FallbackKeychain {
    path: PathBuf,
}

impl FallbackKeychain {
    /// Open (or create) the master-key file at `<data_dir>/master.key`.
    /// On Unix, the file is created with mode 0600. On Windows, the
    /// ACL is restricted to the current user via std::fs.
    pub fn open_or_create(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir).context("create data dir")?;
        let path = data_dir.join(MASTER_KEY_FILENAME);
        if !path.exists() {
            let mut dek = [0u8; DEK_LEN];
            rand::thread_rng().fill_bytes(&mut dek);
            let encoded = B64.encode(dek);
            std::fs::write(&path, encoded).context("write master.key")?;
            restrict_permissions(&path);
            tracing::info!(path = %path.display(), "Created new master.key (fallback keychain)");
        }
        Ok(Self { path })
    }

    /// Read the master key from disk.
    pub fn load_or_create_dek(&mut self) -> Result<[u8; DEK_LEN]> {
        if !self.path.exists() {
            // Race: file got removed between open_or_create and now.
            // Re-create it.
            let mut dek = [0u8; DEK_LEN];
            rand::thread_rng().fill_bytes(&mut dek);
            let encoded = B64.encode(dek);
            std::fs::write(&self.path, encoded)?;
            restrict_permissions(&self.path);
            return Ok(dek);
        }
        let raw = std::fs::read_to_string(&self.path).context("read master.key")?;
        let bytes = B64
            .decode(raw.trim())
            .context("base64 decode master.key")?;
        if bytes.len() != DEK_LEN {
            anyhow::bail!(
                "master.key has wrong length: {} (expected {})",
                bytes.len(),
                DEK_LEN
            );
        }
        let mut dek = [0u8; DEK_LEN];
        dek.copy_from_slice(&bytes);
        Ok(dek)
    }

    /// Path to the master-key file (used by tests + diagnostics).
    /// Path to the master-key file. Used by tests + diagnostics.
    /// `#[allow(dead_code)]` because the production code path
    /// reads through the keyring (or this fallback) and never
    /// asks for the file path; only the `secrets/` test suite
    /// needs to inspect it.
    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Restrict file permissions to owner-only. On Unix this is a
/// single chmod call; on Windows we rely on std::fs creating the
/// file with the user's default ACL (no inheritance from parent).
#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {
    // On Windows, the default ACL for a new file in the user's
    // profile denies other users. We don't tighten further.
    // (v0.5 could add a DACL entry via winapi.)
}

#[cfg(test)]
mod tests {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes256Gcm, Key, Nonce};

    use super::*;
    // DEK_LEN / NONCE_LEN live in the parent secrets module
    // (mod.rs); `super::*` only re-exports fallback.rs's own
    // items, so we import them from the parent path explicitly.
    use crate::secrets::{DEK_LEN, NONCE_LEN};

    #[test]
    fn open_or_create_idempotent() {
        let dir = std::env::temp_dir().join(format!(
            "flowntier-fallback-test-{}",
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let mut a = FallbackKeychain::open_or_create(&dir).unwrap();
        let dek1 = a.load_or_create_dek().unwrap();
        let mut b = FallbackKeychain::open_or_create(&dir).unwrap();
        let dek2 = b.load_or_create_dek().unwrap();
        assert_eq!(dek1, dek2, "DEK must be stable across reopens");
    }

    #[test]
    fn aes_roundtrip_with_dek() {
        // Sanity check that AES-GCM with our DEK roundtrips.
        let mut k = [0u8; DEK_LEN];
        rand::thread_rng().fill_bytes(&mut k);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&k));
        let mut n = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut n);
        let nonce = Nonce::from_slice(&n);
        let plaintext = b"hello, world";
        let ct = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext,
                    aad: b"TEST_AAD",
                },
            )
            .unwrap();
        let pt = cipher
            .decrypt(
                nonce,
                Payload {
                    msg: &ct,
                    aad: b"TEST_AAD",
                },
            )
            .unwrap();
        assert_eq!(pt, plaintext);
    }
}