// API keys live in the Windows Credential Manager, never on disk and never in
// the front end — the island can only ask whether a key is present.
//
// OAuth bundles take a second path: a Credential Manager generic credential is
// capped at ~2560 bytes (the keyring backend writes UTF-16, so ~1280 chars),
// and a ChatGPT access_token JWT alone can exceed that. They are written as
// DPAPI blobs instead — CryptProtectData under the current user, the same
// per-user encryption CredMan itself uses — into
// %LOCALAPPDATA%\Coucou\secrets\<key>.bin. Keys ending in `-oauth` always take
// the blob path so the behavior is deterministic, not a silent fallback.

use std::path::PathBuf;

use keyring::Entry;
use windows::Win32::Foundation::HLOCAL;
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};

const SERVICE: &str = "fr.louisraille.coucou";

/// Every key Coucou may store. Anything outside this list is refused.
pub const KNOWN_KEYS: &[&str] = &[
    "anthropic-api-key",
    "n8n-url",
    "n8n-api-key",
    "vercel-token",
    "github-token",
    "stripe-api-key",
    "resend-api-key",
    "notion-api-key",
    "calcom-api-key",
];

/// Provider credentials are namespaced: `provider-<id>-key` for API keys and
/// `provider-<id>-oauth` for the token bundle. Only declared provider ids pass.
pub fn allowed(key: &str) -> bool {
    if KNOWN_KEYS.contains(&key) {
        return true;
    }
    let Some(rest) = key.strip_prefix("provider-") else {
        return false;
    };
    let Some((id, kind)) = rest.rsplit_once('-') else {
        return false;
    };
    (kind == "key" || kind == "oauth")
        && crate::providers::PROVIDERS.iter().any(|p| p.id == id)
}

fn entry(key: &str) -> Option<Entry> {
    if !allowed(key) {
        return None;
    }
    Entry::new(SERVICE, key).ok()
}

fn uses_blob(key: &str) -> bool {
    key.ends_with("-oauth")
}

// ── DPAPI blob store ──────────────────────────────────────────────────────────

fn blob_path(key: &str) -> Option<PathBuf> {
    // Keys are provider-namespace identifiers — already filename-safe.
    let dir = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    Some(dir.join("Coucou").join("secrets").join(format!("{key}.bin")))
}

fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(&input, windows::core::PCWSTR::null(), None, None, None, 0, &mut output)
            .map_err(|e| format!("DPAPI protect failed: {e}"))?;
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = windows::Win32::Foundation::LocalFree(Some(HLOCAL(output.pbData as *mut _)));
        Ok(bytes)
    }
}

fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(&input, None, None, None, None, 0, &mut output)
            .map_err(|e| format!("DPAPI unprotect failed: {e}"))?;
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = windows::Win32::Foundation::LocalFree(Some(HLOCAL(output.pbData as *mut _)));
        Ok(bytes)
    }
}

fn blob_set(key: &str, value: &str) -> Result<(), String> {
    let path = blob_path(key).ok_or("no LOCALAPPDATA")?;
    if value.is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    let bytes = protect(value.as_bytes())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // Write beside the target and rename: a crash mid-write leaves the old
    // credential readable instead of a half-file that decrypts to garbage.
    let temp = path.with_extension(format!("coucou-{}", std::process::id()));
    std::fs::write(&temp, &bytes).map_err(|e| format!("secret write failed: {e}"))?;
    std::fs::rename(&temp, &path).map_err(|e| format!("secret write failed: {e}"))?;
    Ok(())
}

fn blob_get(key: &str) -> Option<String> {
    let path = blob_path(key)?;
    let bytes = std::fs::read(&path).ok()?;
    let raw = unprotect(&bytes).ok()?;
    String::from_utf8(raw).ok().filter(|v| !v.is_empty())
}

fn blob_clear(key: &str) -> Result<(), String> {
    if let Some(path) = blob_path(key) {
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    } else {
        Ok(())
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn get(key: &str) -> Option<String> {
    if !allowed(key) {
        return None;
    }
    if uses_blob(key) {
        // A bundle written before the blob path existed still reads back from
        // Credential Manager — nothing is orphaned by the switch.
        return blob_get(key).or_else(|| entry(key)?.get_password().ok().filter(|v| !v.is_empty()));
    }
    entry(key)?.get_password().ok().filter(|v| !v.is_empty())
}

pub fn set(key: &str, value: &str) -> Result<(), String> {
    if !allowed(key) {
        return Err(format!("unknown key {key}"));
    }
    if uses_blob(key) {
        blob_set(key, value)?;
        // Clear any leftover CredMan copy so there is exactly one live bundle.
        if let Some(entry) = entry(key) {
            let _ = entry.delete_credential();
        }
        return Ok(());
    }
    let entry = entry(key).ok_or_else(|| format!("unknown key {key}"))?;
    if value.is_empty() {
        let _ = entry.delete_credential();
        return Ok(());
    }
    entry.set_password(value).map_err(|e| e.to_string())
}

pub fn clear(key: &str) -> Result<(), String> {
    if !allowed(key) {
        return Err(format!("unknown key {key}"));
    }
    if uses_blob(key) {
        blob_clear(key)?;
    }
    let entry = entry(key).ok_or_else(|| format!("unknown key {key}"))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn present(key: &str) -> bool {
    get(key).is_some()
}
