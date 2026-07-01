use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand::RngCore;
#[cfg(not(target_os = "macos"))]
use std::fs;
use std::path::Path;
#[cfg(not(target_os = "macos"))]
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;

const KEY_BYTES: usize = 32;
#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "Puffer Code Secrets";
#[cfg(target_os = "macos")]
const KEYCHAIN_ACCOUNT: &str = "puffer-code";
const OVERRIDE_KEY_ENV: &str = "PUFFER_SECRET_STORE_KEY";

/// Loads or creates the vault encryption key from the platform key source.
pub(crate) fn load_or_create_key(store_path: &Path) -> Result<[u8; KEY_BYTES]> {
    if let Ok(encoded) = std::env::var(OVERRIDE_KEY_ENV) {
        return decode_key(encoded.trim());
    }
    #[cfg(target_os = "macos")]
    {
        let _ = store_path;
        return load_or_create_macos_key();
    }
    #[cfg(not(target_os = "macos"))]
    {
        load_or_create_file_key(store_path)
    }
}

#[cfg(target_os = "macos")]
fn load_or_create_macos_key() -> Result<[u8; KEY_BYTES]> {
    if let Some(encoded) = read_keychain_password()? {
        return decode_key(&encoded);
    }
    let key = random_key();
    if let Err(error) = write_keychain_password(&BASE64.encode(key)) {
        if let Some(encoded) = read_keychain_password()? {
            return decode_key(&encoded);
        }
        return Err(error);
    }
    Ok(key)
}

/// `security` exit code for `errSecItemNotFound` — the item simply does not
/// exist yet. Locale-independent, unlike the tool's stderr text.
#[cfg(target_os = "macos")]
const KEYCHAIN_ITEM_NOT_FOUND: i32 = 44;

/// Wraps a failed `security` Keychain command in an actionable error,
/// preserving the tool's own stderr (macOS localizes it, e.g. Chinese) so the
/// user sees a clear next step instead of a bare system message.
#[cfg(target_os = "macos")]
fn keychain_error(action: &str, stderr: &[u8]) -> anyhow::Error {
    let detail = String::from_utf8_lossy(stderr);
    let detail = detail.trim();
    let suffix = if detail.is_empty() {
        String::new()
    } else {
        format!(" ({detail})")
    };
    anyhow::anyhow!(
        "could not {action} in the macOS login Keychain{suffix}. \
         The login Keychain is likely locked or unavailable — unlock the \
         \"login\" keychain in Keychain Access, or set PUFFER_SECRET_STORE_KEY \
         to a base64-encoded 32-byte key to bypass the Keychain entirely."
    )
}

#[cfg(target_os = "macos")]
fn read_keychain_password() -> Result<Option<String>> {
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-w",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            KEYCHAIN_ACCOUNT,
        ])
        .output()
        .context("run `security find-generic-password`")?;
    if output.status.success() {
        return Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ));
    }
    // A missing item (exit 44) is the normal "create it now" path. Any other
    // failure — most commonly a locked/unavailable login keychain — must
    // surface clearly rather than be mistaken for "no key": treating it as
    // absent would overwrite the existing key and leave already-stored secrets
    // undecryptable.
    if output.status.code() == Some(KEYCHAIN_ITEM_NOT_FOUND) {
        return Ok(None);
    }
    Err(keychain_error("read the Puffer secret key", &output.stderr))
}

#[cfg(target_os = "macos")]
fn write_keychain_password(encoded: &str) -> Result<()> {
    // `.output()` (not `.status()`) captures the tool's stderr so the raw
    // macOS error is folded into our actionable message instead of leaking to
    // the console.
    let output = Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            KEYCHAIN_ACCOUNT,
            "-w",
            encoded,
        ])
        .output()
        .context("run `security add-generic-password`")?;
    if !output.status.success() {
        return Err(keychain_error(
            "store the Puffer secret key",
            &output.stderr,
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn load_or_create_file_key(store_path: &Path) -> Result<[u8; KEY_BYTES]> {
    let key_path = fallback_key_path(store_path);
    if key_path.exists() {
        let encoded = fs::read_to_string(&key_path)
            .with_context(|| format!("read fallback secret key {}", key_path.display()))?;
        return decode_key(encoded.trim());
    }
    if let Some(parent) = key_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create fallback key dir {}", parent.display()))?;
    }
    let key = random_key();
    // Reparse-safe create (Windows): the Chrome v20 import may create this key file
    // as SYSTEM inside the user-writable vault dir, so it must not follow a reparse
    // point a same-user process planted at the path. No-op difference elsewhere.
    crate::write_file_no_follow(&key_path, BASE64.encode(key).as_bytes())
        .with_context(|| format!("write fallback secret key {}", key_path.display()))?;
    set_private_permissions(&key_path);
    Ok(key)
}

#[cfg(not(target_os = "macos"))]
fn fallback_key_path(store_path: &Path) -> PathBuf {
    store_path.with_extension("key")
}

fn random_key() -> [u8; KEY_BYTES] {
    let mut key = [0u8; KEY_BYTES];
    rand::thread_rng().fill_bytes(&mut key);
    key
}

fn decode_key(encoded: &str) -> Result<[u8; KEY_BYTES]> {
    let bytes = BASE64.decode(encoded).context("decode Puffer secret key")?;
    if bytes.len() != KEY_BYTES {
        anyhow::bail!("Puffer secret key has invalid length");
    }
    let mut key = [0u8; KEY_BYTES];
    key.copy_from_slice(&bytes);
    Ok(key)
}

#[cfg(not(target_os = "macos"))]
fn set_private_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn keychain_error_includes_stderr_and_actionable_guidance() {
        let err = keychain_error(
            "store the Puffer secret key",
            "找不到用于存储“puffer-code”的钥匙串".as_bytes(),
        );
        let message = format!("{err}");
        // The action and the tool's own (localized) stderr are preserved.
        assert!(message.contains("store the Puffer secret key"));
        assert!(message.contains("找不到用于存储"));
        // The user gets both remediation paths.
        assert!(message.contains("Keychain Access"));
        assert!(message.contains("PUFFER_SECRET_STORE_KEY"));
    }

    #[test]
    fn keychain_error_omits_empty_stderr_suffix() {
        let err = keychain_error("read the Puffer secret key", b"   ");
        let message = format!("{err}");
        assert!(!message.contains("()"));
        assert!(message.contains("PUFFER_SECRET_STORE_KEY"));
    }
}
