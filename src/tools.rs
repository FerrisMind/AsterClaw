//! Agent tools for filesystem, shell, web, and messaging operations.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use async_trait::async_trait;
use regex::Regex;
use serde_json::Value;
use url::Url;

use crate::providers::ToolDefinition;

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub for_user: Option<String>,
    pub for_llm: Option<String>,
    pub silent: bool,
    pub error: Option<String>,
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

pub struct ToolRegistry {
    workspace: PathBuf,
    restrict_to_workspace: bool,
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new(workspace: PathBuf, restrict_to_workspace: bool) -> Self {
        let mut registry = Self {
            workspace,
            restrict_to_workspace,
            tools: HashMap::new(),
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
        self.register(WebSearchTool::new(5));
        self.register(WebFetchTool::new(50_000));
        self.register(MessageTool::new());
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
                function: crate::providers::ToolFunctionDefinition::Simple {
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
        "Edit a file by replacing text"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "find": { "type": "string" },
                "replace": { "type": "string" }
            },
            "required": ["path", "find"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>, _: &str, _: &str) -> ToolResult {
        let path = match arg_string(&args, "path") {
            Some(v) if !v.is_empty() => v,
            _ => return ToolResult::error("Missing required parameter: path"),
        };
        let find = match arg_string(&args, "find") {
            Some(v) if !v.is_empty() => v,
            _ => return ToolResult::error("Missing required parameter: find"),
        };
        let replace = arg_string(&args, "replace").unwrap_or_default();

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

        if !content.contains(&find) {
            return ToolResult::error("Text not found in file");
        }

        let updated = content.replace(&find, &replace);
        match std::fs::write(&file_path, updated) {
            Ok(_) => ToolResult::new("File updated successfully"),
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
                ToolResult::new(&format!("Content appended to: {}", file_path.display()))
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

        let dangerous = [
            "rm -rf", "del /f", "rmdir /s", "format", "mkfs", "diskpart", "dd if=", "shutdown",
            "reboot", "poweroff",
        ];
        for marker in dangerous {
            if command.contains(marker) {
                return ToolResult::error("Command blocked by safety guard");
            }
        }

        let output = if cfg!(target_os = "windows") {
            Command::new("cmd")
                .args(["/C", &command])
                .current_dir(&self.workspace)
                .output()
        } else {
            Command::new("sh")
                .args(["-c", &command])
                .current_dir(&self.workspace)
                .output()
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

pub struct WebSearchTool {
    max_results: usize,
    client: reqwest::Client,
}

impl WebSearchTool {
    pub fn new(max_results: usize) -> Self {
        Self {
            max_results: max_results.clamp(1, 10),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }
    fn description(&self) -> &str {
        "Search the web and return titles/urls/snippets"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "count": { "type": "integer", "minimum": 1, "maximum": 10 }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>, _: &str, _: &str) -> ToolResult {
        let query = match arg_string(&args, "query") {
            Some(v) if !v.is_empty() => v,
            _ => return ToolResult::error("query is required"),
        };
        let count = arg_i64(&args, "count")
            .map(|v| v.clamp(1, 10) as usize)
            .unwrap_or(self.max_results);

        let encoded_query: String =
            url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        let url = format!("https://html.duckduckgo.com/html/?q={}", encoded_query);
        let resp = match self
            .client
            .get(&url)
            .header("User-Agent", "Mozilla/5.0")
            .send()
            .await
        {
            Ok(v) => v,
            Err(e) => return ToolResult::error(&format!("search request failed: {}", e)),
        };

        let html = match resp.text().await {
            Ok(v) => v,
            Err(e) => return ToolResult::error(&format!("search response read failed: {}", e)),
        };

        let re = Regex::new(
            r#"<a[^>]*class="[^"]*result__a[^"]*"[^>]*href="([^"]+)"[^>]*>([\s\S]*?)</a>"#,
        )
        .expect("valid regex");
        let strip = Regex::new(r"<[^>]+>").expect("valid regex");

        let mut lines = vec![format!("Results for: {}", query)];
        let mut seen = 0usize;
        for caps in re.captures_iter(&html) {
            if seen >= count {
                break;
            }
            let raw_url = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let raw_title = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            let title = strip.replace_all(raw_title, "").trim().to_string();
            if title.is_empty() {
                continue;
            }
            lines.push(format!("{}. {}\n   {}", seen + 1, title, raw_url));
            seen += 1;
        }

        if seen == 0 {
            lines.push("No results".to_string());
        }

        let out = lines.join("\n");
        ToolResult::new(&out).with_for_llm(&out).with_for_user(&out)
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
    let re_script = Regex::new(r"(?is)<script[\s\S]*?</script>").expect("valid regex");
    let re_style = Regex::new(r"(?is)<style[\s\S]*?</style>").expect("valid regex");
    let re_tags = Regex::new(r"(?is)<[^>]+>").expect("valid regex");
    let re_ws = Regex::new(r"\s+").expect("valid regex");

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
            "Fetched URL (status={}, truncated={}, chars={})",
            status, truncated, payload["length"]
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
    use tempfile::TempDir;

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
}
