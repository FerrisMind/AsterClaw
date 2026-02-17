use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub heartbeat: HeartbeatConfig,
    #[serde(default)]
    pub devices: DevicesConfig,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentsConfig {
    #[serde(default)]
    pub defaults: AgentDefaults,
}
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
    "~/.asterclaw/workspace".to_string()
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
            host: "127.0.0.1".to_string(),
            port: 18790,
        }
    }
}
fn default_gateway_host() -> String {
    "127.0.0.1".to_string()
}
fn default_gateway_port() -> i32 {
    18790
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default = "default_runtime_worker_threads")]
    pub worker_threads: usize,
    #[serde(default = "default_runtime_max_blocking_threads")]
    pub max_blocking_threads: usize,
}
impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            worker_threads: default_runtime_worker_threads(),
            max_blocking_threads: default_runtime_max_blocking_threads(),
        }
    }
}
fn default_runtime_worker_threads() -> usize {
    2
}
fn default_runtime_max_blocking_threads() -> usize {
    16
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default)]
    pub web: WebToolsConfig,
    #[serde(default)]
    pub exec: ExecToolsConfig,
    #[serde(default = "default_tool_output_max_chars")]
    pub tool_output_max_chars: usize,
}
impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            web: WebToolsConfig::default(),
            exec: ExecToolsConfig::default(),
            tool_output_max_chars: default_tool_output_max_chars(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebToolsConfig {
    #[serde(default)]
    pub brave: BraveConfig,
    #[serde(default)]
    pub duckduckgo: DuckDuckGoConfig,
    #[serde(default = "default_web_fetch_default_max_chars")]
    pub fetch_default_max_chars: usize,
    #[serde(default = "default_web_fetch_hard_max_chars")]
    pub fetch_hard_max_chars: usize,
    #[serde(default = "default_web_fetch_hard_max_bytes")]
    pub fetch_hard_max_bytes: usize,
}
impl Default for WebToolsConfig {
    fn default() -> Self {
        Self {
            brave: BraveConfig::default(),
            duckduckgo: DuckDuckGoConfig::default(),
            fetch_default_max_chars: default_web_fetch_default_max_chars(),
            fetch_hard_max_chars: default_web_fetch_hard_max_chars(),
            fetch_hard_max_bytes: default_web_fetch_hard_max_bytes(),
        }
    }
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecToolsConfig {
    #[serde(default = "default_true")]
    pub confirm_unknown: bool,
    #[serde(default = "default_exec_auto_allow_prefixes")]
    pub auto_allow_prefixes: Vec<String>,
    #[serde(default = "default_exec_require_confirm_prefixes")]
    pub require_confirm_prefixes: Vec<String>,
    #[serde(default = "default_exec_always_deny_prefixes")]
    pub always_deny_prefixes: Vec<String>,
    #[serde(default = "default_exec_stdout_max_bytes")]
    pub stdout_max_bytes: usize,
    #[serde(default = "default_exec_stderr_max_bytes")]
    pub stderr_max_bytes: usize,
}
impl Default for ExecToolsConfig {
    fn default() -> Self {
        Self {
            confirm_unknown: true,
            auto_allow_prefixes: default_exec_auto_allow_prefixes(),
            require_confirm_prefixes: default_exec_require_confirm_prefixes(),
            always_deny_prefixes: default_exec_always_deny_prefixes(),
            stdout_max_bytes: default_exec_stdout_max_bytes(),
            stderr_max_bytes: default_exec_stderr_max_bytes(),
        }
    }
}
fn default_exec_auto_allow_prefixes() -> Vec<String> {
    [
        "ls",
        "dir",
        "pwd",
        "echo",
        "whoami",
        "date",
        "uname",
        "cat",
        "type",
        "head",
        "tail",
        "grep",
        "find",
        "findstr",
        "rg",
        "wc",
        "tree",
        "git status",
        "git log",
        "git diff",
        "git show",
        "git branch",
        "git rev-parse",
        "cargo build",
        "cargo check",
        "cargo test",
        "cargo fmt",
        "cargo clippy",
        "cargo run",
        "cargo doc",
        "cargo metadata",
        "python",
        "python3",
    ]
    .into_iter()
    .map(|s| s.to_string())
    .collect()
}
fn default_exec_require_confirm_prefixes() -> Vec<String> {
    [
        "git commit",
        "git push",
        "npm install",
        "npm i",
        "npm add",
        "npm update",
        "npm uninstall",
        "pnpm install",
        "pnpm add",
        "pnpm update",
        "pnpm remove",
        "yarn install",
        "yarn add",
        "yarn up",
        "yarn remove",
        "pip install",
        "pip3 install",
        "python -m pip install",
        "python3 -m pip install",
        "cargo install",
        "cargo add",
        "cargo remove",
        "cp",
        "mv",
        "touch",
        "mkdir",
        "rmdir",
        "tee",
    ]
    .into_iter()
    .map(|s| s.to_string())
    .collect()
}
fn default_exec_always_deny_prefixes() -> Vec<String> {
    [
        "powershell",
        "pwsh",
        "cmd /c",
        "curl",
        "wget",
        "nc",
        "ncat",
        "telnet",
        "ssh",
        "scp",
        "sftp",
        "ftp",
        "crontab",
        "schtasks",
    ]
    .into_iter()
    .map(|s| s.to_string())
    .collect()
}
fn default_exec_stdout_max_bytes() -> usize {
    256 * 1024
}
fn default_exec_stderr_max_bytes() -> usize {
    256 * 1024
}
fn default_tool_output_max_chars() -> usize {
    200_000
}
fn default_web_fetch_default_max_chars() -> usize {
    120_000
}
fn default_web_fetch_hard_max_chars() -> usize {
    200_000
}
fn default_web_fetch_hard_max_bytes() -> usize {
    1_000_000
}
fn default_max_results() -> i32 {
    5
}
fn default_true_duck() -> bool {
    true
}
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevicesConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub monitor_usb: bool,
}
impl Default for DevicesConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            monitor_usb: true,
        }
    }
}
pub fn get_config_path() -> anyhow::Result<PathBuf> {
    let home = resolve_home_dir()?;
    Ok(home.join(".asterclaw").join("config.json"))
}
pub fn get_legacy_config_path() -> anyhow::Result<PathBuf> {
    let home = resolve_home_dir()?;
    Ok(home.join(".picoclaw").join("config.json"))
}
fn resolve_home_dir() -> anyhow::Result<PathBuf> {
    if let Ok(path) = std::env::var("ASTERCLAW_HOME") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))
}
pub fn load_config(path: &Path) -> anyhow::Result<Config> {
    if path.exists() {
        let data = std::fs::read_to_string(path)?;
        let config: Config = parse_compat_json(&data)?;
        return Ok(config);
    }
    let legacy = get_legacy_config_path()?;
    if legacy.exists() {
        let data = std::fs::read_to_string(&legacy)?;
        let config: Config = parse_compat_json(&data)?;
        return Ok(config);
    }
    Ok(Config::default())
}
pub fn save_config(path: &Path, config: &Config) -> anyhow::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid path"))?;
    std::fs::create_dir_all(dir)?;
    let data = serde_json::to_string_pretty(config)?;
    let temp = tempfile::NamedTempFile::new_in(dir)?;
    std::fs::write(temp.path(), &data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o600))?;
    }
    temp.persist(path)?;
    Ok(())
}
impl Config {
    pub fn workspace_path(&self) -> PathBuf {
        expand_home(&self.agents.defaults.workspace)
    }
}
fn expand_home(path: &str) -> PathBuf {
    if path.starts_with('~')
        && let Ok(home) = resolve_home_dir()
    {
        if path.len() > 1 {
            return home.join(&path[2..]);
        }
        return home;
    }
    PathBuf::from(path)
}
fn parse_compat_json(data: &str) -> anyhow::Result<Config> {
    let value: serde_json::Value = serde_json::from_str(data)?;
    let normalized = normalize_keys(value);
    let mut config: Config = serde_json::from_value(normalized)?;
    config.agents.defaults.workspace = normalize_workspace_path(&config.agents.defaults.workspace);
    Ok(config)
}
fn normalize_workspace_path(workspace: &str) -> String {
    workspace
        .replace(".picors", ".asterclaw")
        .replace(".femtors", ".asterclaw")
        .replace(".picoclaw", ".asterclaw")
}
fn normalize_keys(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let normalized = map
                .into_iter()
                .map(|(k, v)| (camel_to_snake(&k), normalize_keys(v)))
                .collect();
            serde_json::Value::Object(normalized)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(normalize_keys).collect())
        }
        other => other,
    }
}
fn camel_to_snake(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                let prev = chars[i - 1];
                let next = chars.get(i + 1).copied().unwrap_or_default();
                if prev.is_ascii_lowercase()
                    || prev.is_ascii_digit()
                    || (prev.is_ascii_uppercase() && next.is_ascii_lowercase())
                {
                    out.push('_');
                }
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loads_camel_case_legacy_keys() {
        let raw = r#"{
            "agents": { "defaults": { "maxToolIterations": 7 } },
            "devices": { "monitorUsb": false }
        }"#;
        let parsed = parse_compat_json(raw).expect("parse");
        assert_eq!(parsed.agents.defaults.max_tool_iterations, 7);
        assert!(!parsed.devices.monitor_usb);
    }
    #[test]
    fn rewrites_legacy_workspace_brands_to_asterclaw() {
        let raw = r#"{
            "agents": { "defaults": { "workspace": "~/.picors/workspace" } }
        }"#;
        let parsed = parse_compat_json(raw).expect("parse");
        assert_eq!(parsed.agents.defaults.workspace, "~/.asterclaw/workspace");
        let raw_femtors = r#"{
            "agents": { "defaults": { "workspace": "~/.femtors/workspace" } }
        }"#;
        let parsed_femtors = parse_compat_json(raw_femtors).expect("parse");
        assert_eq!(
            parsed_femtors.agents.defaults.workspace,
            "~/.asterclaw/workspace"
        );
    }
    #[test]
    fn loads_exec_tool_policy_from_config() {
        let raw = r#"{
            "tools": {
                "exec": {
                    "confirmUnknown": false,
                    "autoAllowPrefixes": ["echo", "ls"],
                    "requireConfirmPrefixes": ["git commit"],
                    "alwaysDenyPrefixes": ["curl"],
                    "stdoutMaxBytes": 12345,
                    "stderrMaxBytes": 23456
                }
            }
        }"#;
        let parsed = parse_compat_json(raw).expect("parse");
        assert!(!parsed.tools.exec.confirm_unknown);
        assert_eq!(parsed.tools.exec.auto_allow_prefixes, vec!["echo", "ls"]);
        assert_eq!(
            parsed.tools.exec.require_confirm_prefixes,
            vec!["git commit"]
        );
        assert_eq!(parsed.tools.exec.always_deny_prefixes, vec!["curl"]);
        assert_eq!(parsed.tools.exec.stdout_max_bytes, 12345);
        assert_eq!(parsed.tools.exec.stderr_max_bytes, 23456);
    }
    #[test]
    fn loads_tool_limits_from_config() {
        let raw = r#"{
            "tools": {
                "toolOutputMaxChars": 7777,
                "web": {
                    "fetchDefaultMaxChars": 11111,
                    "fetchHardMaxChars": 22222,
                    "fetchHardMaxBytes": 33333
                }
            }
        }"#;
        let parsed = parse_compat_json(raw).expect("parse");
        assert_eq!(parsed.tools.tool_output_max_chars, 7777);
        assert_eq!(parsed.tools.web.fetch_default_max_chars, 11111);
        assert_eq!(parsed.tools.web.fetch_hard_max_chars, 22222);
        assert_eq!(parsed.tools.web.fetch_hard_max_bytes, 33333);
    }
    #[test]
    fn loads_runtime_config_from_config() {
        let raw = r#"{
            "runtime": {
                "workerThreads": 3,
                "maxBlockingThreads": 12
            }
        }"#;
        let parsed = parse_compat_json(raw).expect("parse");
        assert_eq!(parsed.runtime.worker_threads, 3);
        assert_eq!(parsed.runtime.max_blocking_threads, 12);
    }
}
