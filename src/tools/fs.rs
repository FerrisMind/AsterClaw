use super::{Tool, ToolResult, arg_string, ensure_within_workspace, resolve_path};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
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
