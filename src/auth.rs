//! Authentication credential store and basic auth flows.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

fn auth_home_dir() -> PathBuf {
    if let Ok(path) = std::env::var("ASTERCLAW_HOME") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CredentialStore {
    pub credentials: HashMap<String, AuthCredential>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthCredential {
    pub provider: String,
    pub api_key: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub account_id: Option<String>,
    pub auth_method: String,
}

impl AuthCredential {
    pub fn from_token(provider: &str, token: String, expires_at: Option<i64>) -> Self {
        Self {
            provider: provider.to_string(),
            api_key: Some(token),
            access_token: None,
            refresh_token: None,
            expires_at: expires_at.and_then(|e| chrono::DateTime::from_timestamp(e, 0)),
            account_id: None,
            auth_method: "token".to_string(),
        }
    }

    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|exp| exp < chrono::Utc::now())
            .unwrap_or(false)
    }
}

fn primary_store_path() -> PathBuf {
    let home = auth_home_dir();
    home.join(".asterclaw").join("credentials.json")
}

fn legacy_store_path() -> PathBuf {
    let home = auth_home_dir();
    home.join(".asterclaw").join("credentials.json")
}

fn load_store_from_path(path: &PathBuf) -> Result<CredentialStore> {
    let content = std::fs::read_to_string(path)?;
    let store: CredentialStore = serde_json::from_str(&content)?;
    Ok(store)
}

pub fn load_store() -> Result<CredentialStore> {
    let primary = primary_store_path();
    if primary.exists() {
        return load_store_from_path(&primary);
    }

    // Fallback read from legacy location.
    let legacy = legacy_store_path();
    if legacy.exists() {
        return load_store_from_path(&legacy);
    }

    Ok(CredentialStore::default())
}

fn save_store(store: &CredentialStore) -> Result<()> {
    let path = primary_store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(store)?;
    std::fs::write(path, content)?;
    Ok(())
}

pub fn set_credential(provider: &str, cred: AuthCredential) -> Result<()> {
    let mut store = load_store()?;
    store.credentials.insert(provider.to_string(), cred);
    save_store(&store)
}

pub fn delete_credential(provider: &str) -> Result<()> {
    let mut store = load_store()?;
    store.credentials.remove(provider);
    save_store(&store)
}

pub fn delete_all_credentials() -> Result<()> {
    save_store(&CredentialStore::default())
}

pub fn show_status() -> Result<()> {
    let store = load_store()?;

    if store.credentials.is_empty() {
        println!("No authenticated providers.");
        return Ok(());
    }

    println!("\nAuthenticated Providers:");
    println!("----------------------");
    for (provider, cred) in &store.credentials {
        let status = if cred.is_expired() {
            "expired"
        } else {
            "active"
        };
        println!("  {provider}: {} ({status})", cred.auth_method);
    }
    Ok(())
}

pub fn login_openai(_device_code: bool) -> Result<()> {
    println!("AsterClaw OpenAI login:");
    println!("1. Get API key from https://platform.openai.com/api-keys");
    println!("2. Run: asterclaw auth login --provider openai --token <your-key>");
    Ok(())
}

pub fn login_paste_token(provider: &str) -> Result<()> {
    println!("AsterClaw: enter {provider} API key:");
    println!("Usage: asterclaw auth login --provider {provider} --token <token>");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    static AUTH_TEST_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        AUTH_TEST_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|err| err.into_inner())
    }

    #[test]
    fn write_uses_asterclaw_path() {
        let _g = lock();
        let home = tempfile::tempdir().expect("tmp home");
        // SAFETY: guarded by lock for process-wide env mutation.
        unsafe { std::env::set_var("ASTERCLAW_HOME", home.path()) };

        let cred = AuthCredential::from_token("openai", "key-1".to_string(), None);
        set_credential("openai", cred).expect("set cred");

        assert!(home.path().join(".asterclaw/credentials.json").exists());
        assert!(!home.path().join(".picoclaw/credentials.json").exists());
    }

    #[test]
    fn fallback_reads_legacy_if_primary_missing() {
        let _g = lock();
        let home = tempfile::tempdir().expect("tmp home");
        // SAFETY: guarded by lock for process-wide env mutation.
        unsafe { std::env::set_var("ASTERCLAW_HOME", home.path()) };

        let legacy_dir = home.path().join(".asterclaw");
        std::fs::create_dir_all(&legacy_dir).expect("mkdir legacy");
        let mut creds = HashMap::new();
        creds.insert(
            "openai".to_string(),
            AuthCredential::from_token("openai", "legacy".to_string(), None),
        );
        let store = CredentialStore { credentials: creds };
        std::fs::write(
            legacy_dir.join("credentials.json"),
            serde_json::to_string_pretty(&store).expect("serialize"),
        )
        .expect("write");

        let loaded = load_store().expect("load");
        assert_eq!(
            loaded
                .credentials
                .get("openai")
                .and_then(|c| c.api_key.as_ref())
                .map(|s| s.as_str()),
            Some("legacy")
        );
    }
}
