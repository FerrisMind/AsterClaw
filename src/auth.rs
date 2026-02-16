#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]
#![allow(clippy::map_flatten)]

//! Authentication module - Full OAuth2 and credential management
//! Ported from Go version

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Credential store
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
            expires_at: expires_at
                .map(|e| chrono::DateTime::from_timestamp(e, 0))
                .flatten(),
            account_id: None,
            auth_method: "token".to_string(),
        }
    }

    pub fn is_expired(&self) -> bool {
        if let Some(exp) = self.expires_at {
            return exp < chrono::Utc::now();
        }
        false
    }
}

/// Get credentials store path
fn get_store_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".picoclaw").join("credentials.json")
}

/// Load credential store
pub fn load_store() -> Result<CredentialStore> {
    let path = get_store_path();
    if !path.exists() {
        return Ok(CredentialStore::default());
    }
    let content = fs::read_to_string(&path)?;
    let store: CredentialStore = serde_json::from_str(&content)?;
    Ok(store)
}

/// Save credential store
fn save_store(store: &CredentialStore) -> Result<()> {
    let path = get_store_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(store)?;
    fs::write(&path, content)?;
    Ok(())
}

/// Set credential for provider
pub fn set_credential(provider: &str, cred: AuthCredential) -> Result<()> {
    let mut store = load_store()?;
    store.credentials.insert(provider.to_string(), cred);
    save_store(&store)
}

/// Get credential for provider
pub fn get_credential(provider: &str) -> Result<Option<AuthCredential>> {
    let store = load_store()?;
    Ok(store.credentials.get(provider).cloned())
}

/// Delete credential for provider
pub fn delete_credential(provider: &str) -> Result<()> {
    let mut store = load_store()?;
    store.credentials.remove(provider);
    save_store(&store)
}

/// Delete all credentials
pub fn delete_all_credentials() -> Result<()> {
    save_store(&CredentialStore::default())
}

/// Show auth status
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
        println!("  {}: {} ({})", provider, cred.auth_method, status);
    }
    Ok(())
}

/// Login to OpenAI (simplified - would need full OAuth2 implementation)
pub fn login_openai(_device_code: bool) -> Result<()> {
    // In full implementation, this would use OAuth2 device code flow
    // For now, prompt user to enter API key
    println!("OpenAI login:");
    println!("1. Get API key from https://platform.openai.com/api-keys");
    println!("2. Run: picors auth login --provider openai --token <your-key>");
    Ok(())
}

/// Login with paste token
pub fn login_paste_token(provider: &str) -> Result<()> {
    println!("Enter {} API key: ", provider);
    // In interactive mode, would read from stdin
    // For now, just show instructions
    println!(
        "Usage: picors auth login --provider {} --token <token>",
        provider
    );
    Ok(())
}
