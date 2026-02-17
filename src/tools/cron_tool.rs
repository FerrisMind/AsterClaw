//! Cron job management tool.

use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::Value;

use crate::cron::{CronService, Schedule};

use super::{Tool, ToolResult, arg_i64, arg_string};

pub struct CronTool {
    workspace: PathBuf,
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
                "every_seconds": { "type": "integer", "minimum": 1 },
                "cron": { "type": "string" },
                "cron_expr": { "type": "string" },
                "id": { "type": "string" },
                "job_id": { "type": "string" },
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
                        for_llm: Some("No scheduled jobs".into()),
                        silent: true,
                        error: None,
                    };
                }
                let mut lines = vec!["Scheduled jobs:".to_string()];
                for job in jobs {
                    let sched = match job.schedule {
                        Schedule::Every(s) => format!("every {}s", s),
                        Schedule::Cron(e) => format!("cron {}", e),
                    };
                    lines.push(format!(
                        "- {} ({}) [{}]",
                        job.name,
                        job.id,
                        if job.enabled { "enabled" } else { "disabled" }
                    ));
                    lines.push(format!("  schedule: {}", sched));
                    if let Some(ch) = job.channel {
                        lines.push(format!("  channel: {}", ch));
                    }
                    if let Some(cid) = job.chat_id {
                        lines.push(format!("  chat_id: {}", cid));
                    }
                }
                ToolResult {
                    for_user: None,
                    for_llm: Some(lines.join("\n")),
                    silent: true,
                    error: None,
                }
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
                let ch = arg_string(&args, "channel")
                    .or_else(|| (!channel.is_empty()).then(|| channel.to_string()));
                let cid = arg_string(&args, "chat_id")
                    .or_else(|| (!chat_id.is_empty()).then(|| chat_id.to_string()));
                match service.add_job(
                    &name,
                    schedule,
                    &message,
                    enabled,
                    ch.as_deref(),
                    cid.as_deref(),
                ) {
                    Ok(job) => ToolResult {
                        for_user: None,
                        for_llm: Some(format!("Cron job added: {} ({})", job.name, job.id)),
                        silent: true,
                        error: None,
                    },
                    Err(e) => ToolResult::error(&format!("failed to add cron job: {}", e)),
                }
            }
            "remove" => {
                let id = match arg_string(&args, "id").or_else(|| arg_string(&args, "job_id")) {
                    Some(v) if !v.trim().is_empty() => v,
                    _ => return ToolResult::error("id (or job_id) is required for remove"),
                };
                if !service.remove_job(&id) {
                    return ToolResult::error(&format!("cron job not found: {}", id));
                }
                ToolResult {
                    for_user: None,
                    for_llm: Some(format!("Cron job removed: {}", id)),
                    silent: true,
                    error: None,
                }
            }
            "enable" | "disable" => {
                let id = match arg_string(&args, "id").or_else(|| arg_string(&args, "job_id")) {
                    Some(v) if !v.trim().is_empty() => v,
                    _ => return ToolResult::error("id (or job_id) is required"),
                };
                let target = action == "enable";
                if let Some(job) = service.enable_job(&id, target) {
                    ToolResult {
                        for_user: None,
                        for_llm: Some(format!(
                            "Cron job {}: {}",
                            if target { "enabled" } else { "disabled" },
                            job.name
                        )),
                        silent: true,
                        error: None,
                    }
                } else {
                    ToolResult::error(&format!("cron job not found: {}", id))
                }
            }
            _ => ToolResult::error("unknown cron action"),
        }
    }
}
