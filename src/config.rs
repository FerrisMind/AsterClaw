//! Configuration module for picors
//! Ported from Go version

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub agents: AgentsConfig,
    #[serde(default)]
    pub channels: ChannelsConfig,
    #[serde(default)]
    pub providers: ProvidersConfig,
    #[serde(default)]
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub heartbeat: HeartbeatConfig,
}

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentsConfig {
    #[serde(default)]
    pub defaults: AgentDefaults,
}

/// Agent defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefaults {
    #[serde(default = "default_workspace")]
    pub workspace: String,
    #[serde(default = "default_true")]
    pub restrict_to_workspace: bool,
    #[serde(default)]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: i32,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: i32,
}

impl Default for AgentDefaults {
    fn default() -> Self {
        Self {
            workspace: default_workspace(),
            restrict_to_workspace: true,
            provider: String::new(),
            model: default_model(),
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
            max_tool_iterations: default_max_tool_iterations(),
        }
    }
}

fn default_workspace() -> String {
    "~/.picors/workspace".to_string()
}

fn default_true() -> bool {
    true
}

fn default_model() -> String {
    "glm-4.7".to_string()
}

fn default_max_tokens() -> i32 {
    8192
}

fn default_temperature() -> f64 {
    0.7
}

fn default_max_tool_iterations() -> i32 {
    20
}

/// Channels configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelsConfig {
    #[serde(default)]
    pub telegram: TelegramConfig,
}


#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelegramConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub proxy: String,
    #[serde(default)]
    pub allow_from: Vec<String>,
}






/// Providers configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProvidersConfig {
    #[serde(default)]
    pub anthropic: ProviderConfig,
    #[serde(default)]
    pub openai: ProviderConfig,
    #[serde(default)]
    pub openrouter: ProviderConfig,
    #[serde(default)]
    pub groq: ProviderConfig,
    #[serde(default)]
    pub zhipu: ProviderConfig,
    #[serde(default)]
    pub gemini: ProviderConfig,
    #[serde(default)]
    pub deepseek: ProviderConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub api_base: Option<String>,
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub auth_method: Option<String>,
    #[serde(default)]
    pub connect_mode: Option<String>,
}

/// Gateway configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default = "default_gateway_host")]
    pub host: String,
    #[serde(default = "default_gateway_port")]
    pub port: i32,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 18790,
        }
    }
}

fn default_gateway_host() -> String {
    "0.0.0.0".to_string()
}

fn default_gateway_port() -> i32 {
    18790
}

/// Tools configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsConfig {
    #[serde(default)]
    pub web: WebToolsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebToolsConfig {
    #[serde(default)]
    pub brave: BraveConfig,
    #[serde(default)]
    pub duckduckgo: DuckDuckGoConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BraveConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_max_results")]
    pub max_results: i32,
}

impl Default for BraveConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: None,
            max_results: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuckDuckGoConfig {
    #[serde(default = "default_true_duck")]
    pub enabled: bool,
    #[serde(default = "default_max_results")]
    pub max_results: i32,
}

impl Default for DuckDuckGoConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_results: 5,
        }
    }
}

fn default_max_results() -> i32 {
    5
}

fn default_true_duck() -> bool {
    true
}

/// Heartbeat configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_interval")]
    pub interval: i32,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval: 30,
        }
    }
}

fn default_interval() -> i32 {
    30
}


/// Get the config path (~/.picors/config.json)
pub fn get_config_path() -> anyhow::Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
    Ok(home.join(".picors").join("config.json"))
}

/// Get legacy config path (~/.picoclaw/config.json).
pub fn get_legacy_config_path() -> anyhow::Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
    Ok(home.join(".picoclaw").join("config.json"))
}

/// Load config from file
pub fn load_config(path: &Path) -> anyhow::Result<Config> {
    if path.exists() {
        let data = std::fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&data)?;
        return Ok(config);
    }

    // Dual compatibility mode: fall back to legacy ~/.picoclaw/config.json
    let legacy = get_legacy_config_path()?;
    if legacy.exists() {
        let data = std::fs::read_to_string(&legacy)?;
        let config: Config = serde_json::from_str(&data)?;
        return Ok(config);
    }

    Ok(Config::default())
}

/// Save config to file
pub fn save_config(path: &Path, config: &Config) -> anyhow::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid path"))?;
    std::fs::create_dir_all(dir)?;

    let data = serde_json::to_string_pretty(config)?;
    std::fs::write(path, data)?;
    Ok(())
}

impl Config {
    /// Get workspace path with ~ expansion
    pub fn workspace_path(&self) -> PathBuf {
        expand_home(&self.agents.defaults.workspace)
    }

}

/// Expand ~ to home directory
fn expand_home(path: &str) -> PathBuf {
    if path.starts_with('~')
        && let Some(home) = dirs::home_dir()
    {
        if path.len() > 1 {
            return home.join(&path[2..]);
        }
        return home;
    }
    PathBuf::from(path)
}
