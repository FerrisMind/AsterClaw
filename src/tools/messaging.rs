//! Messaging tools: MessageTool, SpawnTool, SubagentTool.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::Value;

use super::{SubagentManager, Tool, ToolResult, arg_string};

// ── MessageTool ─────────────────────────────────────────────────────────

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

// ── SpawnTool ───────────────────────────────────────────────────────────

pub struct SpawnTool {
    handle: Arc<RwLock<Option<Arc<SubagentManager>>>>,
}

impl SpawnTool {
    pub fn new(handle: Arc<RwLock<Option<Arc<SubagentManager>>>>) -> Self {
        Self { handle }
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

// ── SubagentTool ────────────────────────────────────────────────────────

pub struct SubagentTool {
    handle: Arc<RwLock<Option<Arc<SubagentManager>>>>,
}

impl SubagentTool {
    pub fn new(handle: Arc<RwLock<Option<Arc<SubagentManager>>>>) -> Self {
        Self { handle }
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
