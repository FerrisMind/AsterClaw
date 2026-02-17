use super::{Tool, ToolResult, arg_i64, arg_string};
use crate::cron::{CronService, Schedule};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
pub struct CronTool {
    service: Arc<Mutex<CronService>>,
}
impl CronTool {
    pub fn new(service: Arc<Mutex<CronService>>) -> Self {
        Self { service }
    }
}
#[async_trait]
impl Tool for CronTool {
    fn name(&self) -> &str {
        "cron"
    }
    fn description(&self) -> &str {
        concat!(
            "Schedule reminders, tasks, or recurring jobs. ",
            "Use 'at_seconds' for one-time reminders (e.g., 'remind in 10 min' → at_seconds=600). ",
            "Use 'every_seconds' for recurring tasks (e.g., 'every 2 hours' → every_seconds=7200). ",
            "Use 'cron_expr' for complex schedules (e.g., '0 0 9 * * *' for daily at 9am). ",
            "Set deliver=true (default) to send message directly, or deliver=false to process through agent."
        )
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["add", "list", "remove", "enable", "disable"],
                    "description": "Action to perform. Use 'add' when user wants to schedule a reminder or task."
                },
                "name": { "type": "string", "description": "Short name for the job" },
                "message": {
                    "type": "string",
                    "description": "The reminder/task message to display when triggered"
                },
                "at_seconds": {
                    "type": "integer",
                    "description": "One-time: seconds from now when to trigger (e.g., 600 for '10 minutes')",
                    "minimum": 1
                },
                "every_seconds": {
                    "type": "integer",
                    "description": "Recurring: interval in seconds (e.g., 3600 for 'every hour')",
                    "minimum": 1
                },
                "cron_expr": {
                    "type": "string",
                    "description": "Cron expression for complex schedules (e.g., '0 0 9 * * *' for daily at 9am). Uses 6-field format: sec min hour day month weekday."
                },
                "deliver": {
                    "type": "boolean",
                    "description": "If true (default), send message directly to chat. If false, let agent process the message."
                },
                "job_id": { "type": "string", "description": "Job ID (for remove/enable/disable)" },
                "id": { "type": "string", "description": "Alias for job_id" }
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
        match action.as_str() {
            "list" => self.list_jobs(),
            "add" => self.add_job(&args, channel, chat_id),
            "remove" => self.remove_job(&args),
            "enable" => self.toggle_job(&args, true),
            "disable" => self.toggle_job(&args, false),
            _ => ToolResult::error("unknown cron action"),
        }
    }
}
impl CronTool {
    fn list_jobs(&self) -> ToolResult {
        let service = self.service.lock();
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
            let sched = match &job.schedule {
                Schedule::At(ms) => {
                    let dt = chrono::DateTime::from_timestamp_millis(*ms)
                        .map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                        .unwrap_or_else(|| format!("at {}ms", ms));
                    format!("one-time at {}", dt)
                }
                Schedule::Every(ms) => {
                    let secs = *ms / 1000;
                    if secs >= 3600 {
                        format!("every {}h", secs / 3600)
                    } else if secs >= 60 {
                        format!("every {}m", secs / 60)
                    } else {
                        format!("every {}s", secs)
                    }
                }
                Schedule::Cron(e) => format!("cron {}", e),
            };
            let status = if job.enabled { "✅" } else { "⏸" };
            lines.push(format!(
                "- {} {} (id: {}) [{}]",
                status, job.name, job.id, sched
            ));
            if let (Some(ch), Some(cid)) = (&job.channel, &job.chat_id) {
                lines.push(format!("  → {}:{}", ch, cid));
            }
        }
        ToolResult {
            for_user: None,
            for_llm: Some(lines.join("\n")),
            silent: true,
            error: None,
        }
    }
    fn add_job(&self, args: &HashMap<String, Value>, channel: &str, chat_id: &str) -> ToolResult {
        let message = match arg_string(args, "message") {
            Some(v) if !v.trim().is_empty() => v,
            _ => return ToolResult::error("message is required for add"),
        };
        let now_ms = chrono::Utc::now().timestamp_millis();
        let schedule = if let Some(at_secs) = arg_i64(args, "at_seconds") {
            if at_secs < 1 {
                return ToolResult::error("at_seconds must be >= 1");
            }
            Schedule::At(now_ms + at_secs * 1000)
        } else if let Some(every) =
            arg_i64(args, "every_seconds").or_else(|| arg_i64(args, "every"))
        {
            if every < 1 {
                return ToolResult::error("every_seconds must be >= 1");
            }
            Schedule::Every(every as u64 * 1000)
        } else if let Some(expr) =
            arg_string(args, "cron_expr").or_else(|| arg_string(args, "cron"))
        {
            if expr.trim().is_empty() {
                return ToolResult::error("cron expression cannot be empty");
            }
            if expr.parse::<cron::Schedule>().is_err() {
                return ToolResult::error(&format!(
                    "invalid cron expression: '{}'. Use 6-field format: sec min hour day month weekday",
                    expr
                ));
            }
            Schedule::Cron(expr)
        } else {
            return ToolResult::error("one of at_seconds, every_seconds, or cron_expr is required");
        };
        let name = arg_string(args, "name")
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| message.chars().take(30).collect());
        let deliver = args
            .get("deliver")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let ch = if !channel.is_empty() {
            Some(channel)
        } else {
            None
        };
        let cid = if !chat_id.is_empty() {
            Some(chat_id)
        } else {
            None
        };
        match self
            .service
            .lock()
            .add_job(&name, schedule, &message, true, deliver, ch, cid)
        {
            Ok(job) => {
                let when = if let Some(next) = job.next_run_at_ms {
                    let dt = chrono::DateTime::from_timestamp_millis(next)
                        .map(|d| d.format("%H:%M:%S UTC").to_string())
                        .unwrap_or_else(|| "soon".to_string());
                    format!(", next run: {}", dt)
                } else {
                    String::new()
                };
                ToolResult {
                    for_user: None,
                    for_llm: Some(format!(
                        "Cron job added: '{}' (id: {}{})",
                        job.name, job.id, when
                    )),
                    silent: true,
                    error: None,
                }
            }
            Err(e) => ToolResult::error(&format!("failed to add cron job: {}", e)),
        }
    }
    fn remove_job(&self, args: &HashMap<String, Value>) -> ToolResult {
        let id = match arg_string(args, "job_id").or_else(|| arg_string(args, "id")) {
            Some(v) if !v.trim().is_empty() => v,
            _ => return ToolResult::error("job_id is required for remove"),
        };
        if !self.service.lock().remove_job(&id) {
            return ToolResult::error(&format!("cron job not found: {}", id));
        }
        ToolResult {
            for_user: None,
            for_llm: Some(format!("Cron job removed: {}", id)),
            silent: true,
            error: None,
        }
    }
    fn toggle_job(&self, args: &HashMap<String, Value>, enable: bool) -> ToolResult {
        let id = match arg_string(args, "job_id").or_else(|| arg_string(args, "id")) {
            Some(v) if !v.trim().is_empty() => v,
            _ => return ToolResult::error("job_id is required"),
        };
        if let Some(job) = self.service.lock().enable_job(&id, enable) {
            ToolResult {
                for_user: None,
                for_llm: Some(format!(
                    "Cron job {}: {}",
                    if enable { "enabled" } else { "disabled" },
                    job.name
                )),
                silent: true,
                error: None,
            }
        } else {
            ToolResult::error(&format!("cron job not found: {}", id))
        }
    }
}
