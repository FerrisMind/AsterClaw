//! Memory tool — semantic interface for the agent to read/write persistent memory.

use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::Value;

use super::{Tool, ToolResult, arg_string};
use crate::memory::MemoryStore;

pub struct MemoryTool {
    store: MemoryStore,
}

impl MemoryTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            store: MemoryStore::new(workspace),
        }
    }
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Read or write persistent memory (long-term notes and daily journal)"
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["read", "write", "append", "read_daily", "append_daily"],
                    "description": "Action: read/write/append long-term memory, or read_daily/append_daily for today's journal"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write or append (required for write/append/append_daily)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>, _: &str, _: &str) -> ToolResult {
        let action = match arg_string(&args, "action") {
            Some(a) if !a.is_empty() => a,
            _ => return ToolResult::error("Missing required parameter: action"),
        };

        match action.as_str() {
            "read" => {
                let content = self.store.read_long_term();
                if content.is_empty() {
                    ToolResult::new("(memory is empty)")
                } else {
                    ToolResult::new(&content)
                }
            }
            "write" => {
                let content = match arg_string(&args, "content") {
                    Some(c) => c,
                    None => return ToolResult::error("Missing required parameter: content"),
                };
                match self.store.write_long_term(&content) {
                    Ok(()) => ToolResult::new("Memory updated"),
                    Err(e) => ToolResult::error(&format!("Failed to write memory: {e}")),
                }
            }
            "append" => {
                let content = match arg_string(&args, "content") {
                    Some(c) => c,
                    None => return ToolResult::error("Missing required parameter: content"),
                };
                let existing = self.store.read_long_term();
                let merged = if existing.is_empty() {
                    content
                } else {
                    format!("{existing}\n{content}")
                };
                match self.store.write_long_term(&merged) {
                    Ok(()) => ToolResult::new("Appended to memory"),
                    Err(e) => ToolResult::error(&format!("Failed to append memory: {e}")),
                }
            }
            "read_daily" => {
                let content = self.store.read_today();
                if content.is_empty() {
                    ToolResult::new("(no daily notes for today)")
                } else {
                    ToolResult::new(&content)
                }
            }
            "append_daily" => {
                let content = match arg_string(&args, "content") {
                    Some(c) => c,
                    None => return ToolResult::error("Missing required parameter: content"),
                };
                match self.store.append_today(&content) {
                    Ok(()) => ToolResult::new("Appended to daily notes"),
                    Err(e) => ToolResult::error(&format!("Failed to append daily notes: {e}")),
                }
            }
            other => ToolResult::error(&format!(
                "Unknown action: {other}. Use: read, write, append, read_daily, append_daily"
            )),
        }
    }
}
