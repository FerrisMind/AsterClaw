//! Agent tools for filesystem, shell, web, and messaging operations.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use parking_lot::{Mutex, RwLock};
use regex::Regex;
use serde_json::Value;
use url::Url;

use crate::config::WebToolsConfig;

use crate::bus::{InboundMessage, MessageBus};
use crate::cron::{CronService, Schedule};
use crate::providers::ToolDefinition;
use crate::providers::{LlmResponse, Message, Provider};

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub for_user: Option<String>,
    pub for_llm: Option<String>,
    pub silent: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolLoopResult {
    pub content: String,
    pub iterations: i32,
}

#[derive(Debug, Clone)]
pub struct SubagentTask {
    pub status: String,
    pub result: String,
}

pub struct SubagentManager {
    tasks: Arc<Mutex<HashMap<String, SubagentTask>>>,
    next_id: AtomicU64,
    provider: Arc<dyn Provider>,
    model: String,
    bus: Arc<MessageBus>,
    tools: ToolRegistry,
    max_iterations: i32,
}

#[derive(Clone)]
pub struct ToolLoopConfig<'a> {
    pub provider: &'a dyn Provider,
    pub model: &'a str,
    pub tools: &'a ToolRegistry,
    pub max_iterations: i32,
    pub options: HashMap<String, Value>,
    pub channel: &'a str,
    pub chat_id: &'a str,
}

impl SubagentManager {
    pub fn new(
        provider: Arc<dyn Provider>,
        model: String,
        bus: Arc<MessageBus>,
        tools: ToolRegistry,
        max_iterations: i32,
    ) -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(1),
            provider,
            model,
            bus,
            tools,
            max_iterations,
        }
    }

    pub fn spawn(
        self: &Arc<Self>,
        task: String,
        label: String,
        origin_channel: String,
        origin_chat_id: String,
    ) -> String {
        let ack = if label.trim().is_empty() {
            format!("Spawned subagent for task: {}", task)
        } else {
            format!("Spawned subagent '{}' for task: {}", label, task)
        };

        let id = format!("subagent-{}", self.next_id.fetch_add(1, Ordering::SeqCst));
        let rec = SubagentTask {
            status: "running".to_string(),
            result: String::new(),
        };
        self.tasks.lock().insert(id.clone(), rec);

        let manager = self.clone();
        let task_for_spawn = task.clone();
        let label_for_spawn = label.clone();
        let channel_for_spawn = origin_channel.clone();
        let chat_for_spawn = origin_chat_id.clone();
        tokio::spawn(async move {
            manager
                .run_task(
                    id,
                    task_for_spawn,
                    label_for_spawn,
                    channel_for_spawn,
                    chat_for_spawn,
                )
                .await;
        });

        ack
    }

    pub async fn run_sync(
        &self,
        task: String,
        label: String,
        origin_channel: String,
        origin_chat_id: String,
    ) -> anyhow::Result<ToolLoopResult> {
        let mut messages = vec![
            Message::system(
                "You are a subagent. Complete the given task independently and provide a clear, concise result.",
            ),
            Message::user(&task),
        ];
        let loop_result = run_tool_loop(
            ToolLoopConfig {
                provider: self.provider.as_ref(),
                model: &self.model,
                tools: &self.tools,
                max_iterations: self.max_iterations,
                options: HashMap::from([
                    ("max_tokens".to_string(), serde_json::json!(4096)),
                    ("temperature".to_string(), serde_json::json!(0.7)),
                ]),
                channel: &origin_channel,
                chat_id: &origin_chat_id,
            },
            &mut messages,
        )
        .await?;

        let _ = label;
        Ok(loop_result)
    }

    async fn run_task(
        self: Arc<Self>,
        task_id: String,
        task: String,
        label: String,
        origin_channel: String,
        origin_chat_id: String,
    ) {
        let result = self
            .run_sync(
                task.clone(),
                label.clone(),
                origin_channel.clone(),
                origin_chat_id.clone(),
            )
            .await;

        {
            let mut tasks = self.tasks.lock();
            if let Some(rec) = tasks.get_mut(&task_id) {
                match &result {
                    Ok(loop_result) => {
                        rec.status = "completed".to_string();
                        rec.result = loop_result.content.clone();
                    }
                    Err(err) => {
                        rec.status = "failed".to_string();
                        rec.result = format!("Error: {}", err);
                    }
                }
            }
        }

        let announce = match result {
            Ok(loop_result) => {
                let title = if label.trim().is_empty() {
                    task.clone()
                } else {
                    label
                };
                format!(
                    "Task '{}' completed.\n\nResult:\n{}",
                    title, loop_result.content
                )
            }
            Err(err) => format!("Subagent task failed: {}", err),
        };
        let _ = self
            .bus
            .publish_inbound(InboundMessage {
                channel: "system".to_string(),
                sender_id: format!("subagent:{}", task_id),
                chat_id: format!("{}:{}", origin_channel, origin_chat_id),
                content: announce,
                media: None,
                session_key: format!("subagent:{}", task_id),
                metadata: None,
            })
            .await;
    }
}

pub async fn run_tool_loop(
    cfg: ToolLoopConfig<'_>,
    messages: &mut Vec<Message>,
) -> anyhow::Result<ToolLoopResult> {
    let mut iterations = 0;
    let mut content = String::new();
    while iterations < cfg.max_iterations {
        iterations += 1;
        let defs = cfg.tools.to_provider_defs();
        let response: LlmResponse = cfg
            .provider
            .chat_with_options(messages, Some(&defs), cfg.model, cfg.options.clone())
            .await?;
        content = response.content.clone();
        if response.tool_calls.is_empty() {
            break;
        }
        messages.push(Message {
            role: "assistant".to_string(),
            content: response.content,
            tool_calls: response.tool_calls.clone(),
            tool_call_id: None,
        });
        for tc in response.tool_calls {
            let tool_name = tc.name.unwrap_or_default();
            let args = tc.arguments.unwrap_or_default();
            let result = if let Some(tool) = cfg.tools.get(&tool_name) {
                tool.execute(args, cfg.channel, cfg.chat_id).await
            } else {
                ToolResult::error(&format!("tool '{}' not found", tool_name))
            };
            let llm_content = result
                .error
                .or(result.for_llm)
                .unwrap_or_else(|| "tool executed".to_string());
            messages.push(Message::tool(&llm_content, &tc.id));
        }
    }
    Ok(ToolLoopResult {
        content,
        iterations,
    })
}

impl ToolResult {
    pub fn new(content: &str) -> Self {
        Self {
            for_user: None,
            for_llm: Some(content.to_string()),
            silent: false,
            error: None,
        }
    }

    pub fn error(msg: &str) -> Self {
        Self {
            for_user: None,
            for_llm: None,
            silent: false,
            error: Some(msg.to_string()),
        }
    }

    pub fn with_for_user(mut self, msg: &str) -> Self {
        self.for_user = Some(msg.to_string());
        self
    }

    pub fn with_for_llm(mut self, msg: &str) -> Self {
        self.for_llm = Some(msg.to_string());
        self
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    fn summary(&self) -> String {
        format!("- **{}**: {}", self.name(), self.description())
    }
    async fn execute(
        &self,
        args: HashMap<String, Value>,
        channel: &str,
        chat_id: &str,
    ) -> ToolResult;
}

#[derive(Clone)]
pub struct ToolRegistry {
    workspace: PathBuf,
    restrict_to_workspace: bool,
    tools: HashMap<String, Arc<dyn Tool>>,
    subagent_manager: Arc<RwLock<Option<Arc<SubagentManager>>>>,
    web_config: WebToolsConfig,
}

impl ToolRegistry {
    #[allow(dead_code)] // Used by tests
    pub fn new(workspace: PathBuf, restrict_to_workspace: bool) -> Self {
        Self::with_web_config(workspace, restrict_to_workspace, WebToolsConfig::default())
    }

    pub fn with_web_config(
        workspace: PathBuf,
        restrict_to_workspace: bool,
        web_config: WebToolsConfig,
    ) -> Self {
        let mut registry = Self {
            workspace,
            restrict_to_workspace,
            tools: HashMap::new(),
            subagent_manager: Arc::new(RwLock::new(None)),
            web_config,
        };
        registry.register_builtin_tools();
        registry
    }

    fn register_builtin_tools(&mut self) {
        self.register(ReadFileTool::new(
            self.workspace.clone(),
            self.restrict_to_workspace,
        ));
        self.register(WriteFileTool::new(
            self.workspace.clone(),
            self.restrict_to_workspace,
        ));
        self.register(ListDirTool::new(
            self.workspace.clone(),
            self.restrict_to_workspace,
        ));
        self.register(EditFileTool::new(
            self.workspace.clone(),
            self.restrict_to_workspace,
        ));
        self.register(AppendFileTool::new(
            self.workspace.clone(),
            self.restrict_to_workspace,
        ));
        self.register(ExecTool::new(self.workspace.clone()));
        self.register(WebSearchTool::from_config(&self.web_config));
        self.register(WebFetchTool::new(50_000));
        self.register(MessageTool::new());
        self.register(SpawnTool::new(self.subagent_manager.clone()));
        self.register(SubagentTool::new(self.subagent_manager.clone()));
        self.register(I2cTool);
        self.register(SpiTool);
        self.register(CronTool::new(self.workspace.clone()));
    }

    pub fn register<T: Tool + 'static>(&mut self, tool: T) {
        self.tools.insert(tool.name().to_string(), Arc::new(tool));
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn list_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tools.keys().cloned().collect();
        names.sort();
        names
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn set_subagent_manager(&self, manager: Arc<SubagentManager>) {
        *self.subagent_manager.write() = Some(manager);
    }

    pub fn get_summaries(&self) -> Vec<String> {
        let mut result: Vec<String> = self.tools.values().map(|t| t.summary()).collect();
        result.sort();
        result
    }

    pub fn to_provider_defs(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|tool| ToolDefinition {
                tool_type: "function".to_string(),
                function: crate::providers::ToolFunctionDefinition {
                    name: tool.name().to_string(),
                    description: tool.description().to_string(),
                    parameters: tool.parameters(),
                },
            })
            .collect()
    }
}

fn resolve_path(workspace: &Path, path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        p
    } else {
        workspace.join(p)
    }
}

fn canonicalize_for_check(path: &Path, allow_missing_leaf: bool) -> Result<PathBuf, String> {
    if path.exists() {
        return path.canonicalize().map_err(|e| e.to_string());
    }
    if !allow_missing_leaf {
        return Err(format!("Path not found: {}", path.display()));
    }

    let mut missing_parts = Vec::new();
    let mut cursor = path;
    while !cursor.exists() {
        let leaf = cursor
            .file_name()
            .ok_or_else(|| format!("Invalid path: {}", path.display()))?;
        missing_parts.push(leaf.to_os_string());
        cursor = cursor
            .parent()
            .ok_or_else(|| format!("Invalid path: {}", path.display()))?;
    }

    let mut resolved = cursor.canonicalize().map_err(|e| e.to_string())?;
    for part in missing_parts.iter().rev() {
        resolved.push(part);
    }
    Ok(resolved)
}

fn ensure_within_workspace(
    workspace: &Path,
    candidate_path: &Path,
    allow_missing_leaf: bool,
) -> Result<(), String> {
    let ws = workspace.canonicalize().map_err(|e| e.to_string())?;
    let path = canonicalize_for_check(candidate_path, allow_missing_leaf)?;
    if !path.starts_with(&ws) {
        return Err("Access denied: path is outside workspace".to_string());
    }
    Ok(())
}

fn arg_string(args: &HashMap<String, Value>, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn arg_i64(args: &HashMap<String, Value>, key: &str) -> Option<i64> {
    args.get(key).and_then(|v| v.as_i64())
}

pub struct ReadFileTool {
    workspace: PathBuf,
    restrict: bool,
}

impl ReadFileTool {
    pub fn new(workspace: PathBuf, restrict: bool) -> Self {
        Self {
            workspace,
            restrict,
        }
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read a file from the filesystem"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file to read" }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>, _: &str, _: &str) -> ToolResult {
        let path = match arg_string(&args, "path") {
            Some(v) if !v.is_empty() => v,
            _ => return ToolResult::error("Missing required parameter: path"),
        };
        let file_path = resolve_path(&self.workspace, &path);
        if self.restrict
            && let Err(err) = ensure_within_workspace(&self.workspace, &file_path, false)
        {
            return ToolResult::error(&err);
        }

        match std::fs::read_to_string(&file_path) {
            Ok(content) => ToolResult::new(&content),
            Err(e) => ToolResult::error(&format!("Failed to read file: {}", e)),
        }
    }
}

pub struct WriteFileTool {
    workspace: PathBuf,
    restrict: bool,
}

impl WriteFileTool {
    pub fn new(workspace: PathBuf, restrict: bool) -> Self {
        Self {
            workspace,
            restrict,
        }
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }
    fn description(&self) -> &str {
        "Write content to a file"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file to write" },
                "content": { "type": "string", "description": "Content to write to the file" }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>, _: &str, _: &str) -> ToolResult {
        let path = match arg_string(&args, "path") {
            Some(v) if !v.is_empty() => v,
            _ => return ToolResult::error("Missing required parameter: path"),
        };
        let content = arg_string(&args, "content").unwrap_or_default();

        let file_path = resolve_path(&self.workspace, &path);
        if self.restrict
            && let Err(err) = ensure_within_workspace(&self.workspace, &file_path, true)
        {
            return ToolResult::error(&err);
        }

        if let Some(parent) = file_path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            return ToolResult::error(&format!("Failed to create parent directory: {}", e));
        }

        match std::fs::write(&file_path, content) {
            Ok(_) => ToolResult::new(&format!(
                "File written successfully: {}",
                file_path.display()
            )),
            Err(e) => ToolResult::error(&format!("Failed to write file: {}", e)),
        }
    }
}

pub struct ListDirTool {
    workspace: PathBuf,
    restrict: bool,
}

impl ListDirTool {
    pub fn new(workspace: PathBuf, restrict: bool) -> Self {
        Self {
            workspace,
            restrict,
        }
    }
}

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }
    fn description(&self) -> &str {
        "List directory contents"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the directory to list" }
            }
        })
    }

    async fn execute(&self, args: HashMap<String, Value>, _: &str, _: &str) -> ToolResult {
        let path = arg_string(&args, "path").unwrap_or_else(|| ".".to_string());
        let dir_path = resolve_path(&self.workspace, &path);

        if self.restrict
            && let Err(err) = ensure_within_workspace(&self.workspace, &dir_path, false)
        {
            return ToolResult::error(&err);
        }

        let entries = match std::fs::read_dir(&dir_path) {
            Ok(v) => v,
            Err(e) => return ToolResult::error(&format!("Failed to read directory: {}", e)),
        };

        let mut lines = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let kind = if entry.file_type().map(|f| f.is_dir()).unwrap_or(false) {
                "[DIR]"
            } else {
                "[FILE]"
            };
            lines.push(format!("{} {}", kind, name));
        }
        lines.sort();
        ToolResult::new(&lines.join("\n"))
    }
}

pub struct EditFileTool {
    workspace: PathBuf,
    restrict: bool,
}

impl EditFileTool {
    pub fn new(workspace: PathBuf, restrict: bool) -> Self {
        Self {
            workspace,
            restrict,
        }
    }
}

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }
    fn description(&self) -> &str {
        "Edit a file by replacing old_text with new_text. old_text must match exactly once."
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "old_text": { "type": "string" },
                "new_text": { "type": "string" },
                "find": { "type": "string", "description": "compat alias for old_text" },
                "replace": { "type": "string", "description": "compat alias for new_text" }
            },
            "required": ["path", "old_text", "new_text"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>, _: &str, _: &str) -> ToolResult {
        let path = match arg_string(&args, "path") {
            Some(v) if !v.is_empty() => v,
            _ => return ToolResult::error("Missing required parameter: path"),
        };
        let old_text = match arg_string(&args, "old_text").or_else(|| arg_string(&args, "find")) {
            Some(v) if !v.is_empty() => v,
            _ => return ToolResult::error("Missing required parameter: old_text"),
        };
        let new_text = match arg_string(&args, "new_text").or_else(|| arg_string(&args, "replace"))
        {
            Some(v) => v,
            None => return ToolResult::error("Missing required parameter: new_text"),
        };

        let file_path = resolve_path(&self.workspace, &path);
        if self.restrict
            && let Err(err) = ensure_within_workspace(&self.workspace, &file_path, false)
        {
            return ToolResult::error(&err);
        }

        let content = match std::fs::read_to_string(&file_path) {
            Ok(v) => v,
            Err(e) => return ToolResult::error(&format!("Failed to read file: {}", e)),
        };

        if !content.contains(&old_text) {
            return ToolResult::error("old_text not found in file. Make sure it matches exactly");
        }

        let count = content.matches(&old_text).count();
        if count > 1 {
            return ToolResult::error(&format!(
                "old_text appears {} times. Please provide more context to make it unique",
                count
            ));
        }

        let updated = content.replacen(&old_text, &new_text, 1);
        match std::fs::write(&file_path, updated) {
            Ok(_) => ToolResult {
                for_user: None,
                for_llm: Some(format!("File edited: {}", path)),
                silent: true,
                error: None,
            },
            Err(e) => ToolResult::error(&format!("Failed to write file: {}", e)),
        }
    }
}

pub struct AppendFileTool {
    workspace: PathBuf,
    restrict: bool,
}

impl AppendFileTool {
    pub fn new(workspace: PathBuf, restrict: bool) -> Self {
        Self {
            workspace,
            restrict,
        }
    }
}

#[async_trait]
impl Tool for AppendFileTool {
    fn name(&self) -> &str {
        "append_file"
    }
    fn description(&self) -> &str {
        "Append content to a file"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>, _: &str, _: &str) -> ToolResult {
        let path = match arg_string(&args, "path") {
            Some(v) if !v.is_empty() => v,
            _ => return ToolResult::error("Missing required parameter: path"),
        };
        let content = arg_string(&args, "content").unwrap_or_default();

        let file_path = resolve_path(&self.workspace, &path);
        if self.restrict
            && let Err(err) = ensure_within_workspace(&self.workspace, &file_path, true)
        {
            return ToolResult::error(&err);
        }

        if let Some(parent) = file_path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            return ToolResult::error(&format!("Failed to create parent directory: {}", e));
        }

        match std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&file_path)
        {
            Ok(mut file) => {
                if let Err(e) = std::io::Write::write_all(&mut file, content.as_bytes()) {
                    return ToolResult::error(&format!("Failed to write file: {}", e));
                }
                ToolResult {
                    for_user: None,
                    for_llm: Some(format!("Appended to {}", path)),
                    silent: true,
                    error: None,
                }
            }
            Err(e) => ToolResult::error(&format!("Failed to open file: {}", e)),
        }
    }
}

pub struct ExecTool {
    workspace: PathBuf,
}

impl ExecTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for ExecTool {
    fn name(&self) -> &str {
        "exec"
    }
    fn description(&self) -> &str {
        "Execute a shell command in workspace"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Command to execute" }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>, _: &str, _: &str) -> ToolResult {
        let command = match arg_string(&args, "command") {
            Some(v) if !v.is_empty() => v,
            _ => return ToolResult::error("Missing required parameter: command"),
        };

        // Normalize whitespace to prevent trivial bypass (e.g. "r m  -rf").
        let command_normalized = command
            .to_ascii_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        let dangerous = [
            "rm -rf",
            "rm -fr",
            "del /f",
            "del /s",
            "rmdir /s",
            "format c:",
            "mkfs",
            "diskpart",
            "dd if=",
            "shutdown",
            "reboot",
            "poweroff",
            "halt",
            "init 0",
            "init 6",
            "chmod 777",
            "chmod -r 777",
            "chown -r",
            "> /dev/sd",
            "> /dev/null",
            ":(){ :|:&",
            "| sh",
            "| bash",
            "| zsh",
            "|sh",
            "|bash",
        ];
        for marker in dangerous {
            if command_normalized.contains(marker) {
                return ToolResult::error("Command blocked by safety guard");
            }
        }

        let output = if cfg!(target_os = "windows") {
            tokio::process::Command::new("cmd")
                .args(["/C", &command])
                .current_dir(&self.workspace)
                .output()
                .await
        } else {
            tokio::process::Command::new("sh")
                .args(["-c", &command])
                .current_dir(&self.workspace)
                .output()
                .await
        };

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                if out.status.success() {
                    ToolResult::new(&stdout)
                } else {
                    ToolResult::error(&format!("Command failed: {}\n{}", stderr, stdout))
                }
            }
            Err(e) => ToolResult::error(&format!("Failed to execute command: {}", e)),
        }
    }
}

/// Web search provider strategy.
#[derive(Debug, Clone)]
enum SearchProvider {
    /// Brave Search API (requires API key).
    Brave { api_key: String, max_results: usize },
    /// DuckDuckGo HTML scraping (no key required).
    DuckDuckGo { max_results: usize },
}

pub struct WebSearchTool {
    provider: SearchProvider,
    client: reqwest::Client,
}

impl WebSearchTool {
    /// Build from config — priority: Brave > DuckDuckGo > disabled.
    pub fn from_config(web: &WebToolsConfig) -> Self {
        let brave_key = web
            .brave
            .api_key
            .clone()
            .or_else(|| std::env::var("BRAVE_API_KEY").ok())
            .filter(|k| !k.trim().is_empty());

        let provider = if let Some(key) = brave_key.filter(|_| web.brave.enabled) {
            SearchProvider::Brave {
                api_key: key,
                max_results: (web.brave.max_results as usize).clamp(1, 10),
            }
        } else if web.duckduckgo.enabled {
            SearchProvider::DuckDuckGo {
                max_results: (web.duckduckgo.max_results as usize).clamp(1, 10),
            }
        } else {
            // Default fallback: DDG always works without a key
            SearchProvider::DuckDuckGo { max_results: 5 }
        };

        Self {
            provider,
            client: reqwest::Client::new(),
        }
    }

    /// Brave Search API call.
    async fn search_brave(
        &self,
        query: &str,
        api_key: &str,
        count: usize,
    ) -> Result<String, String> {
        let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        let url = format!(
            "https://api.search.brave.com/res/v1/web/search?q={}&count={}",
            encoded, count
        );
        let resp = self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .header("X-Subscription-Token", api_key)
            .send()
            .await
            .map_err(|e| format!("Brave request failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("Brave API error: {}", resp.status()));
        }

        let body: Value = resp
            .json()
            .await
            .map_err(|e| format!("Brave JSON parse failed: {e}"))?;

        let results = body.pointer("/web/results").and_then(|v| v.as_array());

        let mut lines = vec![format!("Results for: {}", query)];
        if let Some(items) = results {
            for (i, item) in items.iter().take(count).enumerate() {
                let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("");
                let desc = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                lines.push(format!("{}. {}\n   {}", i + 1, title, url));
                if !desc.is_empty() {
                    lines.push(format!("   {}", desc));
                }
            }
        }
        if lines.len() == 1 {
            lines.push("No results".to_string());
        }
        Ok(lines.join("\n"))
    }

    /// DuckDuckGo HTML scraping (no API key needed).
    async fn search_ddg(&self, query: &str, count: usize) -> Result<String, String> {
        let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        let url = format!("https://html.duckduckgo.com/html/?q={}", encoded);
        let resp = self.client
            .get(&url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .send()
            .await
            .map_err(|e| format!("DDG request failed: {e}"))?;

        let html = resp
            .text()
            .await
            .map_err(|e| format!("DDG response read failed: {e}"))?;

        static RE_LINK: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(
                r#"<a[^>]*class="[^"]*result__a[^"]*"[^>]*href="([^"]+)"[^>]*>([\s\S]*?)</a>"#,
            )
            .expect("valid regex")
        });
        static RE_SNIPPET: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"<a\s+class="result__snippet[^"]*".*?>([\s\S]*?)</a>"#)
                .expect("valid regex")
        });
        static RE_STRIP: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"<[^>]+>").expect("valid regex"));

        let link_caps: Vec<_> = RE_LINK.captures_iter(&html).collect();
        let snippet_caps: Vec<_> = RE_SNIPPET.captures_iter(&html).collect();

        let mut lines = vec![format!("Results for: {} (via DuckDuckGo)", query)];
        let mut seen = 0usize;

        for (i, caps) in link_caps.iter().enumerate() {
            if seen >= count {
                break;
            }
            let raw_url = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let raw_title = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            let title = RE_STRIP.replace_all(raw_title, "").trim().to_string();
            if title.is_empty() {
                continue;
            }

            // Decode DDG redirect URLs (uddg= parameter)
            let final_url = if raw_url.contains("uddg=") {
                url::form_urlencoded::parse(raw_url.as_bytes())
                    .find(|(k, _)| k == "uddg")
                    .map(|(_, v)| v.to_string())
                    .unwrap_or_else(|| raw_url.to_string())
            } else {
                raw_url.to_string()
            };

            lines.push(format!("{}. {}\n   {}", seen + 1, title, final_url));

            // Attach snippet if available
            if let Some(snippet_cap) = snippet_caps.get(i) {
                let snippet = RE_STRIP
                    .replace_all(snippet_cap.get(1).map(|m| m.as_str()).unwrap_or(""), "")
                    .trim()
                    .to_string();
                if !snippet.is_empty() {
                    lines.push(format!("   {}", snippet));
                }
            }
            seen += 1;
        }

        if seen == 0 {
            lines.push("No results".to_string());
        }
        Ok(lines.join("\n"))
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }
    fn description(&self) -> &str {
        "Search the web for current information. Returns titles, URLs, and snippets from search results."
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" },
                "count": { "type": "integer", "description": "Number of results (1-10)", "minimum": 1, "maximum": 10 }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>, _: &str, _: &str) -> ToolResult {
        let query = match arg_string(&args, "query") {
            Some(v) if !v.is_empty() => v,
            _ => return ToolResult::error("query is required"),
        };

        let result = match &self.provider {
            SearchProvider::Brave {
                api_key,
                max_results,
            } => {
                let count = arg_i64(&args, "count")
                    .map(|v| v.clamp(1, 10) as usize)
                    .unwrap_or(*max_results);
                self.search_brave(&query, api_key, count).await
            }
            SearchProvider::DuckDuckGo { max_results } => {
                let count = arg_i64(&args, "count")
                    .map(|v| v.clamp(1, 10) as usize)
                    .unwrap_or(*max_results);
                self.search_ddg(&query, count).await
            }
        };

        match result {
            Ok(out) => ToolResult::new(&out).with_for_llm(&out).with_for_user(&out),
            Err(e) => ToolResult::error(&format!("search failed: {}", e)),
        }
    }
}

pub struct WebFetchTool {
    max_chars: usize,
    client: reqwest::Client,
}

impl WebFetchTool {
    pub fn new(max_chars: usize) -> Self {
        Self {
            max_chars: max_chars.max(100),
            client: reqwest::Client::new(),
        }
    }
}

fn html_to_text(input: &str) -> String {
    static RE_SCRIPT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<script[\s\S]*?</script>").expect("valid regex"));
    static RE_STYLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<style[\s\S]*?</style>").expect("valid regex"));
    static RE_TAGS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<[^>]+>").expect("valid regex"));
    static RE_WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("valid regex"));
    let re_script = &*RE_SCRIPT;
    let re_style = &*RE_STYLE;
    let re_tags = &*RE_TAGS;
    let re_ws = &*RE_WS;

    let s = re_script.replace_all(input, "");
    let s = re_style.replace_all(&s, "");
    let s = re_tags.replace_all(&s, " ");
    re_ws.replace_all(&s, " ").trim().to_string()
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }
    fn description(&self) -> &str {
        "Fetch URL and extract readable text"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" },
                "max_chars": { "type": "integer", "minimum": 100 }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>, _: &str, _: &str) -> ToolResult {
        let input_url = match arg_string(&args, "url") {
            Some(v) if !v.is_empty() => v,
            _ => return ToolResult::error("url is required"),
        };
        let parsed = match Url::parse(&input_url) {
            Ok(v) => v,
            Err(e) => return ToolResult::error(&format!("invalid URL: {}", e)),
        };
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return ToolResult::error("only http/https URLs are allowed");
        }
        let limit = arg_i64(&args, "max_chars")
            .map(|v| v.max(100) as usize)
            .unwrap_or(self.max_chars);

        let resp = match self
            .client
            .get(parsed)
            .header("User-Agent", "Mozilla/5.0")
            .send()
            .await
        {
            Ok(v) => v,
            Err(e) => return ToolResult::error(&format!("request failed: {}", e)),
        };

        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let raw = match resp.text().await {
            Ok(v) => v,
            Err(e) => return ToolResult::error(&format!("failed to read response: {}", e)),
        };

        let mut text = if content_type.contains("application/json") {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
                serde_json::to_string_pretty(&json).unwrap_or(raw)
            } else {
                raw
            }
        } else if content_type.contains("text/html")
            || raw.starts_with("<!DOCTYPE")
            || raw.to_ascii_lowercase().starts_with("<html")
        {
            html_to_text(&raw)
        } else {
            raw
        };

        let truncated = text.chars().count() > limit;
        if truncated {
            text = text.chars().take(limit).collect();
        }

        let payload = serde_json::json!({
            "url": input_url,
            "status": status,
            "content_type": content_type,
            "truncated": truncated,
            "length": text.chars().count(),
            "text": text,
        });
        let for_user =
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string());
        let for_llm = format!(
            "Fetched URL (status={}, truncated={}, chars={})\n\n{}",
            status, truncated, payload["length"], text
        );

        ToolResult::new(&for_user)
            .with_for_user(&for_user)
            .with_for_llm(&for_llm)
    }
}

pub struct MessageTool;

impl MessageTool {
    pub fn new() -> Self {
        Self
    }
}

pub struct SpawnTool {
    handle: Arc<RwLock<Option<Arc<SubagentManager>>>>,
}
pub struct SubagentTool {
    handle: Arc<RwLock<Option<Arc<SubagentManager>>>>,
}
pub struct I2cTool;
pub struct SpiTool;
pub struct CronTool {
    workspace: PathBuf,
}

impl SpawnTool {
    pub fn new(handle: Arc<RwLock<Option<Arc<SubagentManager>>>>) -> Self {
        Self { handle }
    }
}

impl SubagentTool {
    pub fn new(handle: Arc<RwLock<Option<Arc<SubagentManager>>>>) -> Self {
        Self { handle }
    }
}

impl CronTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    fn jobs_path(&self) -> PathBuf {
        self.workspace.join("cron").join("jobs.json")
    }
}

#[async_trait]
impl Tool for SpawnTool {
    fn name(&self) -> &str {
        "spawn"
    }
    fn description(&self) -> &str {
        "Spawn an asynchronous subagent task"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": { "type": "string" },
                "label": { "type": "string" },
                "prompt": { "type": "string", "description": "compat alias for task" }
            },
            "required": ["task"]
        })
    }
    async fn execute(
        &self,
        args: HashMap<String, Value>,
        channel: &str,
        chat_id: &str,
    ) -> ToolResult {
        let task = match arg_string(&args, "task").or_else(|| arg_string(&args, "prompt")) {
            Some(v) if !v.trim().is_empty() => v,
            _ => return ToolResult::error("task is required"),
        };
        let label = arg_string(&args, "label").unwrap_or_default();
        let manager = match self.handle.read().clone() {
            Some(v) => v,
            None => return ToolResult::error("Subagent manager not configured"),
        };
        let response = manager.spawn(task, label, channel.to_string(), chat_id.to_string());
        ToolResult {
            for_user: None,
            for_llm: Some(response),
            silent: true,
            error: None,
        }
    }
}

#[async_trait]
impl Tool for SubagentTool {
    fn name(&self) -> &str {
        "subagent"
    }
    fn description(&self) -> &str {
        "Run a synchronous subagent task"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task": { "type": "string" },
                "label": { "type": "string" },
                "prompt": { "type": "string", "description": "compat alias for task" }
            },
            "required": ["task"]
        })
    }
    async fn execute(
        &self,
        args: HashMap<String, Value>,
        channel: &str,
        chat_id: &str,
    ) -> ToolResult {
        let task = match arg_string(&args, "task").or_else(|| arg_string(&args, "prompt")) {
            Some(v) if !v.trim().is_empty() => v,
            _ => return ToolResult::error("task is required"),
        };
        let label = arg_string(&args, "label").unwrap_or_default();
        let manager = match self.handle.read().clone() {
            Some(v) => v,
            None => return ToolResult::error("Subagent manager not configured"),
        };
        let result = manager
            .run_sync(
                task,
                label.clone(),
                channel.to_string(),
                chat_id.to_string(),
            )
            .await;
        match result {
            Ok(loop_result) => {
                let user_content = if loop_result.content.chars().count() > 500 {
                    format!(
                        "{}...",
                        loop_result.content.chars().take(500).collect::<String>()
                    )
                } else {
                    loop_result.content.clone()
                };
                let label_text = if label.trim().is_empty() {
                    "(unnamed)".to_string()
                } else {
                    label
                };
                ToolResult {
                    for_user: Some(user_content),
                    for_llm: Some(format!(
                        "Subagent task completed:\nLabel: {}\nIterations: {}\nResult: {}",
                        label_text, loop_result.iterations, loop_result.content
                    )),
                    silent: false,
                    error: None,
                }
            }
            Err(err) => ToolResult::error(&format!("Subagent execution failed: {}", err)),
        }
    }
}

#[async_trait]
impl Tool for I2cTool {
    fn name(&self) -> &str {
        "i2c"
    }
    fn description(&self) -> &str {
        "Interact with I2C bus devices. Actions: detect, scan, read, write (Linux only)"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["detect", "scan", "read", "write"] },
                "bus": { "type": "string" },
                "address": { "type": "integer" },
                "register": { "type": "integer" },
                "data": { "type": "array", "items": { "type": "integer" } },
                "length": { "type": "integer" },
                "confirm": { "type": "boolean" }
            },
            "required": ["action"]
        })
    }
    async fn execute(&self, args: HashMap<String, Value>, _: &str, _: &str) -> ToolResult {
        if !cfg!(target_os = "linux") {
            return ToolResult::error(
                "I2C is only supported on Linux. This tool requires /dev/i2c-* device files.",
            );
        }
        let action = match arg_string(&args, "action") {
            Some(v) => v,
            None => return ToolResult::error("action is required"),
        };
        match action.as_str() {
            "detect" => {
                let mut buses = Vec::new();
                if let Ok(paths) = glob::glob("/dev/i2c-*") {
                    for path in paths.flatten() {
                        buses.push(path.display().to_string());
                    }
                }
                if buses.is_empty() {
                    return ToolResult {
                        for_user: None,
                        for_llm: Some("No I2C buses found".to_string()),
                        silent: true,
                        error: None,
                    };
                }
                let payload =
                    serde_json::to_string_pretty(&buses).unwrap_or_else(|_| "[]".to_string());
                ToolResult {
                    for_user: None,
                    for_llm: Some(format!("Found {} I2C bus(es):\n{}", buses.len(), payload)),
                    silent: true,
                    error: None,
                }
            }
            "scan" => {
                let bus = match arg_string(&args, "bus") {
                    Some(v) if !v.is_empty() => v,
                    _ => return ToolResult::error("bus is required (e.g. \"1\" for /dev/i2c-1)"),
                };
                let dev = format!("/dev/i2c-{}", bus);
                if !std::path::Path::new(&dev).exists() {
                    return ToolResult::error(&format!("I2C bus not found: {}", dev));
                }
                match Command::new("i2cdetect").args(["-y", &bus]).output() {
                    Ok(out) if out.status.success() => ToolResult {
                        for_user: None,
                        for_llm: Some(String::from_utf8_lossy(&out.stdout).to_string()),
                        silent: true,
                        error: None,
                    },
                    Ok(out) => ToolResult::error(&String::from_utf8_lossy(&out.stderr)),
                    Err(_) => ToolResult::error("i2cdetect command not available"),
                }
            }
            "read" => {
                let bus = match arg_string(&args, "bus") {
                    Some(v) if !v.is_empty() => v,
                    _ => return ToolResult::error("bus is required"),
                };
                let addr = match arg_i64(&args, "address") {
                    Some(v) if (0x03..=0x77).contains(&v) => v as u8,
                    _ => {
                        return ToolResult::error(
                            "address must be in valid 7-bit range (0x03-0x77)",
                        );
                    }
                };
                let reg = arg_i64(&args, "register");
                let output = if let Some(reg_val) = reg {
                    Command::new("i2cget")
                        .args([
                            "-y",
                            &bus,
                            &format!("0x{addr:02x}"),
                            &format!("0x{:02x}", reg_val as u8),
                        ])
                        .output()
                } else {
                    Command::new("i2cget")
                        .args(["-y", &bus, &format!("0x{addr:02x}")])
                        .output()
                };
                match output {
                    Ok(out) if out.status.success() => ToolResult {
                        for_user: None,
                        for_llm: Some(format!(
                            "Read result: {}",
                            String::from_utf8_lossy(&out.stdout).trim()
                        )),
                        silent: true,
                        error: None,
                    },
                    Ok(out) => ToolResult::error(&String::from_utf8_lossy(&out.stderr)),
                    Err(_) => ToolResult::error("i2cget command not available"),
                }
            }
            "write" => {
                if args.get("confirm").and_then(|v| v.as_bool()) != Some(true) {
                    return ToolResult::error("confirm=true is required for write operations");
                }
                let bus = match arg_string(&args, "bus") {
                    Some(v) if !v.is_empty() => v,
                    _ => return ToolResult::error("bus is required"),
                };
                let addr = match arg_i64(&args, "address") {
                    Some(v) if (0x03..=0x77).contains(&v) => v as u8,
                    _ => {
                        return ToolResult::error(
                            "address must be in valid 7-bit range (0x03-0x77)",
                        );
                    }
                };
                let data = match args.get("data").and_then(|v| v.as_array()) {
                    Some(v) if !v.is_empty() => v,
                    _ => return ToolResult::error("data is required for write action"),
                };
                let reg = arg_i64(&args, "register");
                let mut cmd = Command::new("i2cset");
                cmd.args(["-y", &bus, &format!("0x{addr:02x}")]);
                if let Some(reg_val) = reg {
                    cmd.arg(format!("0x{:02x}", reg_val as u8));
                }
                for b in data {
                    if let Some(n) = b.as_i64() {
                        if !(0..=255).contains(&n) {
                            return ToolResult::error("data bytes must be 0..255");
                        }
                        cmd.arg(format!("0x{:02x}", n as u8));
                    } else {
                        return ToolResult::error("data bytes must be integers");
                    }
                }
                match cmd.output() {
                    Ok(out) if out.status.success() => ToolResult {
                        for_user: None,
                        for_llm: Some("I2C write completed".to_string()),
                        silent: true,
                        error: None,
                    },
                    Ok(out) => ToolResult::error(&String::from_utf8_lossy(&out.stderr)),
                    Err(_) => ToolResult::error("i2cset command not available"),
                }
            }
            _ => ToolResult::error("unknown action: valid actions are detect, scan, read, write"),
        }
    }
}

#[async_trait]
impl Tool for SpiTool {
    fn name(&self) -> &str {
        "spi"
    }
    fn description(&self) -> &str {
        "Interact with SPI bus devices. Actions: list, transfer, read (Linux only)"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["list", "transfer", "read"] },
                "device": { "type": "string" },
                "speed": { "type": "integer" },
                "mode": { "type": "integer" },
                "bits": { "type": "integer" },
                "data": { "type": "array", "items": { "type": "integer" } },
                "length": { "type": "integer" },
                "confirm": { "type": "boolean" }
            },
            "required": ["action"]
        })
    }
    async fn execute(&self, args: HashMap<String, Value>, _: &str, _: &str) -> ToolResult {
        if !cfg!(target_os = "linux") {
            return ToolResult::error(
                "SPI is only supported on Linux. This tool requires /dev/spidev* device files.",
            );
        }
        let action = match arg_string(&args, "action") {
            Some(v) => v,
            None => return ToolResult::error("action is required"),
        };
        match action.as_str() {
            "list" => {
                let mut devices = Vec::new();
                if let Ok(paths) = glob::glob("/dev/spidev*") {
                    for path in paths.flatten() {
                        devices.push(path.display().to_string());
                    }
                }
                if devices.is_empty() {
                    return ToolResult {
                        for_user: None,
                        for_llm: Some("No SPI devices found".to_string()),
                        silent: true,
                        error: None,
                    };
                }
                let payload =
                    serde_json::to_string_pretty(&devices).unwrap_or_else(|_| "[]".to_string());
                ToolResult {
                    for_user: None,
                    for_llm: Some(format!(
                        "Found {} SPI device(s):\n{}",
                        devices.len(),
                        payload
                    )),
                    silent: true,
                    error: None,
                }
            }
            "transfer" => {
                if args.get("confirm").and_then(|v| v.as_bool()) != Some(true) {
                    return ToolResult::error("confirm=true is required for transfer operations");
                }
                let device = match arg_string(&args, "device") {
                    Some(v) if !v.is_empty() => v,
                    _ => {
                        return ToolResult::error(
                            "device is required (e.g. \"2.0\" for /dev/spidev2.0)",
                        );
                    }
                };
                let data = match args.get("data").and_then(|v| v.as_array()) {
                    Some(v) if !v.is_empty() => v,
                    _ => return ToolResult::error("data is required for transfer"),
                };
                let tx_hex: Vec<String> = data
                    .iter()
                    .filter_map(|v| v.as_i64())
                    .map(|n| format!("{:02x}", (n.clamp(0, 255)) as u8))
                    .collect();
                if tx_hex.is_empty() {
                    return ToolResult::error("data bytes must be integers");
                }
                let output = Command::new("spidev_test")
                    .args([
                        "-D",
                        &format!("/dev/spidev{}", device),
                        "-p",
                        &tx_hex.join(""),
                    ])
                    .output();
                match output {
                    Ok(out) if out.status.success() => ToolResult {
                        for_user: None,
                        for_llm: Some(String::from_utf8_lossy(&out.stdout).to_string()),
                        silent: true,
                        error: None,
                    },
                    Ok(out) => ToolResult::error(&String::from_utf8_lossy(&out.stderr)),
                    Err(_) => ToolResult::error("spidev_test command not available"),
                }
            }
            "read" => {
                let device = match arg_string(&args, "device") {
                    Some(v) if !v.is_empty() => v,
                    _ => return ToolResult::error("device is required"),
                };
                let length = arg_i64(&args, "length").unwrap_or(1).clamp(1, 4096) as usize;
                let zeros = "00".repeat(length);
                let output = Command::new("spidev_test")
                    .args(["-D", &format!("/dev/spidev{}", device), "-p", &zeros])
                    .output();
                match output {
                    Ok(out) if out.status.success() => ToolResult {
                        for_user: None,
                        for_llm: Some(String::from_utf8_lossy(&out.stdout).to_string()),
                        silent: true,
                        error: None,
                    },
                    Ok(out) => ToolResult::error(&String::from_utf8_lossy(&out.stderr)),
                    Err(_) => ToolResult::error("spidev_test command not available"),
                }
            }
            _ => ToolResult::error("unknown action: valid actions are list, transfer, read"),
        }
    }
}

#[async_trait]
impl Tool for CronTool {
    fn name(&self) -> &str {
        "cron"
    }
    fn description(&self) -> &str {
        "Manage scheduled tasks (add/list/remove/enable/disable)"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["add", "list", "remove", "enable", "disable"] },
                "name": { "type": "string" },
                "message": { "type": "string" },
                "every": { "type": "integer", "minimum": 1 },
                "every_seconds": { "type": "integer", "minimum": 1, "description": "compat alias for every" },
                "cron": { "type": "string" },
                "cron_expr": { "type": "string", "description": "compat alias for cron" },
                "id": { "type": "string", "description": "job id for remove/enable/disable" },
                "job_id": { "type": "string", "description": "compat alias for id" },
                "enabled": { "type": "boolean" },
                "channel": { "type": "string" },
                "chat_id": { "type": "string" }
            },
            "required": ["action"]
        })
    }
    async fn execute(
        &self,
        args: HashMap<String, Value>,
        channel: &str,
        chat_id: &str,
    ) -> ToolResult {
        let action = match arg_string(&args, "action") {
            Some(v) if !v.is_empty() => v.to_lowercase(),
            _ => return ToolResult::error("action is required"),
        };
        let mut service = CronService::new(&self.jobs_path(), None);

        match action.as_str() {
            "list" => {
                let jobs = service.list_jobs(false);
                if jobs.is_empty() {
                    return ToolResult {
                        for_user: None,
                        for_llm: Some("No scheduled jobs".to_string()),
                        silent: true,
                        error: None,
                    };
                }
                let mut lines = vec!["Scheduled jobs:".to_string()];
                for job in jobs {
                    let schedule = match job.schedule {
                        Schedule::Every(sec) => format!("every {}s", sec),
                        Schedule::Cron(expr) => format!("cron {}", expr),
                    };
                    lines.push(format!(
                        "- {} ({}) [{}]",
                        job.name,
                        job.id,
                        if job.enabled { "enabled" } else { "disabled" }
                    ));
                    lines.push(format!("  schedule: {}", schedule));
                    if let Some(ch) = job.channel {
                        lines.push(format!("  channel: {}", ch));
                    }
                    if let Some(cid) = job.chat_id {
                        lines.push(format!("  chat_id: {}", cid));
                    }
                }
                let out = lines.join("\n");
                return ToolResult {
                    for_user: None,
                    for_llm: Some(out),
                    silent: true,
                    error: None,
                };
            }
            "add" => {
                let message = match arg_string(&args, "message") {
                    Some(v) if !v.trim().is_empty() => v,
                    _ => return ToolResult::error("message is required for add"),
                };
                let schedule = if let Some(every) =
                    arg_i64(&args, "every").or_else(|| arg_i64(&args, "every_seconds"))
                {
                    if every < 1 {
                        return ToolResult::error("every must be >= 1");
                    }
                    Schedule::Every(every as u64)
                } else if let Some(expr) =
                    arg_string(&args, "cron").or_else(|| arg_string(&args, "cron_expr"))
                {
                    if expr.trim().is_empty() {
                        return ToolResult::error("cron expression cannot be empty");
                    }
                    Schedule::Cron(expr)
                } else {
                    return ToolResult::error(
                        "add requires either every/every_seconds or cron/cron_expr",
                    );
                };
                let name = arg_string(&args, "name")
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| message.chars().take(30).collect());
                let enabled = args
                    .get("enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let channel_arg = arg_string(&args, "channel")
                    .or_else(|| (!channel.is_empty()).then(|| channel.to_string()));
                let chat_id_arg = arg_string(&args, "chat_id")
                    .or_else(|| (!chat_id.is_empty()).then(|| chat_id.to_string()));
                let created = match service.add_job(
                    &name,
                    schedule,
                    &message,
                    enabled,
                    channel_arg.as_deref(),
                    chat_id_arg.as_deref(),
                ) {
                    Ok(job) => job,
                    Err(e) => return ToolResult::error(&format!("failed to add cron job: {}", e)),
                };
                return ToolResult {
                    for_user: None,
                    for_llm: Some(format!("Cron job added: {} ({})", created.name, created.id)),
                    silent: true,
                    error: None,
                };
            }
            "remove" => {
                let id = match arg_string(&args, "id").or_else(|| arg_string(&args, "job_id")) {
                    Some(v) if !v.trim().is_empty() => v,
                    _ => return ToolResult::error("id (or job_id) is required for remove"),
                };
                if !service.remove_job(&id) {
                    return ToolResult::error(&format!("cron job not found: {}", id));
                }
                return ToolResult {
                    for_user: None,
                    for_llm: Some(format!("Cron job removed: {}", id)),
                    silent: true,
                    error: None,
                };
            }
            "enable" | "disable" => {
                let id = match arg_string(&args, "id").or_else(|| arg_string(&args, "job_id")) {
                    Some(v) if !v.trim().is_empty() => v,
                    _ => return ToolResult::error("id (or job_id) is required for enable/disable"),
                };
                let target = action == "enable";
                if let Some(job) = service.enable_job(&id, target) {
                    return ToolResult {
                        for_user: None,
                        for_llm: Some(format!(
                            "Cron job {}: {}",
                            if target { "enabled" } else { "disabled" },
                            job.name
                        )),
                        silent: true,
                        error: None,
                    };
                }
                return ToolResult::error(&format!("cron job not found: {}", id));
            }
            _ => ToolResult::error("unknown cron action"),
        }
    }
}

#[async_trait]
impl Tool for MessageTool {
    fn name(&self) -> &str {
        "message"
    }
    fn description(&self) -> &str {
        "Send a message to the current chat"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "content": { "type": "string" },
                "channel": { "type": "string" },
                "chat_id": { "type": "string" }
            },
            "required": ["content"]
        })
    }

    async fn execute(
        &self,
        args: HashMap<String, Value>,
        current_channel: &str,
        current_chat_id: &str,
    ) -> ToolResult {
        let content = match arg_string(&args, "content") {
            Some(v) if !v.trim().is_empty() => v,
            _ => return ToolResult::error("content is required"),
        };
        let requested_channel =
            arg_string(&args, "channel").unwrap_or_else(|| current_channel.to_string());
        let requested_chat =
            arg_string(&args, "chat_id").unwrap_or_else(|| current_chat_id.to_string());

        if requested_channel != current_channel || requested_chat != current_chat_id {
            return ToolResult::error("cross-channel message routing is not supported in MVP");
        }
        if requested_channel.is_empty() || requested_chat.is_empty() {
            return ToolResult::error("no target channel/chat specified");
        }

        ToolResult {
            for_user: Some(content),
            for_llm: Some(format!(
                "Message sent to {}:{}",
                requested_channel, requested_chat
            )),
            silent: true,
            error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::MessageBus;
    use crate::cron::CronService;
    use crate::providers::{LlmResponse, Provider, ToolDefinition};
    use std::sync::Arc;
    use tempfile::TempDir;

    async fn edit_call(ws: &std::path::Path, args: HashMap<String, Value>) -> ToolResult {
        let registry = ToolRegistry::new(ws.to_path_buf(), true);
        let tool = registry.get("edit_file").expect("tool should exist");
        tool.execute(args, "", "").await
    }

    struct MockProvider;

    #[async_trait::async_trait]
    impl Provider for MockProvider {
        async fn chat_with_options(
            &self,
            _messages: &mut Vec<Message>,
            _tools: Option<&[ToolDefinition]>,
            _model: &str,
            _options: HashMap<String, Value>,
        ) -> anyhow::Result<LlmResponse> {
            Ok(LlmResponse {
                content: "subagent result".to_string(),
                tool_calls: vec![],
                finish_reason: Some("stop".to_string()),
                usage: None,
            })
        }
    }

    #[tokio::test]
    async fn write_file_allows_new_file_inside_workspace() {
        let tmp = TempDir::new().expect("tmp dir");
        let ws = tmp.path().to_path_buf();
        let registry = ToolRegistry::new(ws.clone(), true);
        let tool = registry.get("write_file").expect("tool should exist");

        let mut args = HashMap::new();
        args.insert(
            "path".to_string(),
            Value::String("notes/test.txt".to_string()),
        );
        args.insert("content".to_string(), Value::String("hello".to_string()));

        let result = tool.execute(args, "", "").await;
        assert!(result.error.is_none(), "{:?}", result.error);
        assert_eq!(
            std::fs::read_to_string(ws.join("notes/test.txt")).expect("written"),
            "hello"
        );
    }

    #[tokio::test]
    async fn write_file_blocks_escape_outside_workspace() {
        let tmp = TempDir::new().expect("tmp dir");
        let ws = tmp.path().to_path_buf();
        let registry = ToolRegistry::new(ws, true);
        let tool = registry.get("write_file").expect("tool should exist");

        let mut args = HashMap::new();
        args.insert("path".to_string(), Value::String("../evil.txt".to_string()));
        args.insert("content".to_string(), Value::String("x".to_string()));

        let result = tool.execute(args, "", "").await;
        assert!(result.error.is_some());
    }

    #[tokio::test]
    async fn web_fetch_rejects_invalid_scheme() {
        let tool = WebFetchTool::new(200);
        let mut args = HashMap::new();
        args.insert(
            "url".to_string(),
            Value::String("file:///etc/passwd".to_string()),
        );
        let result = tool.execute(args, "", "").await;
        assert!(result.error.is_some());
    }

    #[tokio::test]
    async fn message_tool_is_silent_and_for_user() {
        let tool = MessageTool::new();
        let mut args = HashMap::new();
        args.insert("content".to_string(), Value::String("ping".to_string()));
        let result = tool.execute(args, "telegram", "123").await;
        assert!(result.error.is_none());
        assert!(result.silent);
        assert_eq!(result.for_user.as_deref(), Some("ping"));
    }

    #[tokio::test]
    async fn edit_file_success_replace_once() {
        let tmp = TempDir::new().expect("tmp dir");
        let path = tmp.path().join("test.txt");
        std::fs::write(&path, "Hello World\nThis is a test").expect("write");

        let mut args = HashMap::new();
        args.insert(
            "path".to_string(),
            Value::String(path.to_string_lossy().to_string()),
        );
        args.insert("old_text".to_string(), Value::String("World".to_string()));
        args.insert(
            "new_text".to_string(),
            Value::String("Universe".to_string()),
        );

        let result = edit_call(tmp.path(), args).await;
        assert!(result.error.is_none(), "{:?}", result.error);
        assert!(result.silent);
        let content = std::fs::read_to_string(path).expect("read");
        assert!(content.contains("Hello Universe"));
        assert!(!content.contains("Hello World"));
    }

    #[tokio::test]
    async fn edit_file_not_found() {
        let tmp = TempDir::new().expect("tmp dir");
        let path = tmp.path().join("missing.txt");
        let mut args = HashMap::new();
        args.insert(
            "path".to_string(),
            Value::String(path.to_string_lossy().to_string()),
        );
        args.insert("old_text".to_string(), Value::String("a".to_string()));
        args.insert("new_text".to_string(), Value::String("b".to_string()));

        let result = edit_call(tmp.path(), args).await;
        assert!(result.error.is_some());
        assert!(
            result
                .error
                .as_ref()
                .expect("err")
                .contains("Failed to read file")
                || result.error.as_ref().expect("err").contains("not found")
        );
    }

    #[tokio::test]
    async fn edit_file_old_text_missing() {
        let tmp = TempDir::new().expect("tmp dir");
        let path = tmp.path().join("test.txt");
        std::fs::write(&path, "Hello World").expect("write");

        let mut args = HashMap::new();
        args.insert(
            "path".to_string(),
            Value::String(path.to_string_lossy().to_string()),
        );
        args.insert("old_text".to_string(), Value::String("Goodbye".to_string()));
        args.insert("new_text".to_string(), Value::String("Hello".to_string()));

        let result = edit_call(tmp.path(), args).await;
        assert!(result.error.is_some());
        assert!(result.error.as_ref().expect("err").contains("not found"));
    }

    #[tokio::test]
    async fn edit_file_blocks_multiple_matches() {
        let tmp = TempDir::new().expect("tmp dir");
        let path = tmp.path().join("test.txt");
        std::fs::write(&path, "test test test").expect("write");

        let mut args = HashMap::new();
        args.insert(
            "path".to_string(),
            Value::String(path.to_string_lossy().to_string()),
        );
        args.insert("old_text".to_string(), Value::String("test".to_string()));
        args.insert("new_text".to_string(), Value::String("done".to_string()));

        let result = edit_call(tmp.path(), args).await;
        assert!(result.error.is_some());
        assert!(result.error.as_ref().expect("err").contains("appears"));
    }

    #[tokio::test]
    async fn edit_file_missing_required_params() {
        let tmp = TempDir::new().expect("tmp dir");
        let mut args = HashMap::new();
        args.insert("path".to_string(), Value::String("a.txt".to_string()));
        let result = edit_call(tmp.path(), args).await;
        assert!(result.error.is_some());
        assert!(
            result
                .error
                .as_ref()
                .expect("err")
                .contains("Missing required parameter")
        );
    }

    #[tokio::test]
    async fn edit_file_blocks_outside_workspace() {
        let ws = TempDir::new().expect("ws");
        let out = TempDir::new().expect("out");
        let path = out.path().join("x.txt");
        std::fs::write(&path, "a").expect("write");

        let mut args = HashMap::new();
        args.insert(
            "path".to_string(),
            Value::String(path.to_string_lossy().to_string()),
        );
        args.insert("old_text".to_string(), Value::String("a".to_string()));
        args.insert("new_text".to_string(), Value::String("b".to_string()));

        let result = edit_call(ws.path(), args).await;
        assert!(result.error.is_some());
        assert!(
            result
                .error
                .as_ref()
                .expect("err")
                .contains("outside workspace")
        );
    }

    #[tokio::test]
    async fn append_file_success() {
        let tmp = TempDir::new().expect("tmp");
        let path = tmp.path().join("append.txt");
        std::fs::write(&path, "Initial content").expect("write");
        let registry = ToolRegistry::new(tmp.path().to_path_buf(), true);
        let tool = registry.get("append_file").expect("tool");

        let mut args = HashMap::new();
        args.insert(
            "path".to_string(),
            Value::String(path.to_string_lossy().to_string()),
        );
        args.insert(
            "content".to_string(),
            Value::String("\nAppended content".to_string()),
        );

        let result = tool.execute(args, "", "").await;
        assert!(result.error.is_none(), "{:?}", result.error);
        assert!(result.silent);
        let content = std::fs::read_to_string(path).expect("read");
        assert!(content.contains("Initial content"));
        assert!(content.contains("Appended content"));
    }

    #[tokio::test]
    async fn cron_tool_add_list_remove_flow() {
        let tmp = TempDir::new().expect("tmp");
        let ws = tmp.path().to_path_buf();
        let registry = ToolRegistry::new(ws.clone(), true);
        let tool = registry.get("cron").expect("cron tool");

        let mut add_args = HashMap::new();
        add_args.insert("action".to_string(), Value::String("add".to_string()));
        add_args.insert("message".to_string(), Value::String("Ping".to_string()));
        add_args.insert("every".to_string(), Value::Number(60.into()));
        let add_res = tool.execute(add_args, "telegram", "123").await;
        assert!(add_res.error.is_none(), "{:?}", add_res.error);
        assert!(add_res.silent);

        let mut list_args = HashMap::new();
        list_args.insert("action".to_string(), Value::String("list".to_string()));
        let list_res = tool.execute(list_args, "telegram", "123").await;
        assert!(list_res.error.is_none(), "{:?}", list_res.error);
        let list_text = list_res.for_llm.unwrap_or_default();
        assert!(list_text.contains("Scheduled jobs"));
        assert!(list_text.contains("Ping"));

        let jobs_path = ws.join("cron").join("jobs.json");
        let service = CronService::new(&jobs_path, None);
        let jobs = service.list_jobs(false);
        assert_eq!(jobs.len(), 1);
        let job_id = jobs[0].id.clone();

        let mut remove_args = HashMap::new();
        remove_args.insert("action".to_string(), Value::String("remove".to_string()));
        remove_args.insert("id".to_string(), Value::String(job_id));
        let remove_res = tool.execute(remove_args, "telegram", "123").await;
        assert!(remove_res.error.is_none(), "{:?}", remove_res.error);
        assert!(remove_res.silent);
    }

    #[tokio::test]
    async fn subagent_tool_executes_synchronously() {
        let tmp = TempDir::new().expect("tmp");
        let registry = ToolRegistry::new(tmp.path().to_path_buf(), true);
        let bus = Arc::new(MessageBus::new());
        let manager = Arc::new(SubagentManager::new(
            Arc::new(MockProvider),
            "test-model".to_string(),
            bus,
            registry.clone(),
            3,
        ));
        registry.set_subagent_manager(manager);
        let tool = registry.get("subagent").expect("subagent tool");

        let mut args = HashMap::new();
        args.insert("task".to_string(), Value::String("do thing".to_string()));
        let result = tool.execute(args, "telegram", "123").await;
        assert!(result.error.is_none(), "{:?}", result.error);
        assert_eq!(result.for_user.as_deref(), Some("subagent result"));
        assert!(
            result
                .for_llm
                .unwrap_or_default()
                .contains("Subagent task completed")
        );
    }

    #[tokio::test]
    async fn spawn_tool_runs_async_and_announces_result() {
        let tmp = TempDir::new().expect("tmp");
        let registry = ToolRegistry::new(tmp.path().to_path_buf(), true);
        let bus = Arc::new(MessageBus::new());
        let mut inbound_rx = bus.take_inbound_receiver().expect("inbound rx");
        let manager = Arc::new(SubagentManager::new(
            Arc::new(MockProvider),
            "test-model".to_string(),
            bus.clone(),
            registry.clone(),
            3,
        ));
        registry.set_subagent_manager(manager);
        let tool = registry.get("spawn").expect("spawn tool");

        let mut args = HashMap::new();
        args.insert("task".to_string(), Value::String("background".to_string()));
        args.insert("label".to_string(), Value::String("bg1".to_string()));
        let result = tool.execute(args, "telegram", "777").await;
        assert!(result.error.is_none(), "{:?}", result.error);
        assert!(result.silent);

        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), inbound_rx.recv())
            .await
            .expect("timeout waiting inbound")
            .expect("inbound should exist");
        assert_eq!(msg.channel, "system");
        assert_eq!(msg.chat_id, "telegram:777");
        assert!(msg.content.contains("completed") || msg.content.contains("failed"));
    }

    #[tokio::test]
    async fn i2c_requires_action() {
        let tool = I2cTool;
        let res = tool.execute(HashMap::new(), "", "").await;
        assert!(res.error.is_some());
    }

    #[tokio::test]
    async fn spi_requires_action() {
        let tool = SpiTool;
        let res = tool.execute(HashMap::new(), "", "").await;
        assert!(res.error.is_some());
    }
}
