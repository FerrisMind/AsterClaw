//! Shell execution tool with hardened safety deny-list.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;

use async_trait::async_trait;
use serde_json::Value;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;

use crate::config::ExecToolsConfig;

use super::{Tool, ToolResult, arg_string};

pub struct ExecTool {
    workspace: PathBuf,
    policy: ExecPolicyConfig,
    stdout_max_bytes: usize,
    stderr_max_bytes: usize,
}

impl ExecTool {
    pub fn new(workspace: PathBuf, config: ExecToolsConfig) -> Self {
        let stdout_max_bytes = config.stdout_max_bytes.max(1024);
        let stderr_max_bytes = config.stderr_max_bytes.max(1024);
        Self {
            workspace,
            policy: ExecPolicyConfig::from_config(config),
            stdout_max_bytes,
            stderr_max_bytes,
        }
    }
}

/// Normalise whitespace and case so trivial deny-list bypasses
/// (e.g. `R M  -rf`) are caught.
fn normalise_command(cmd: &str) -> String {
    cmd.to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Static list of substrings that must not appear in the normalised command.
const DENY_LIST: &[&str] = &[
    // destructive FS operations
    "rm -rf",
    "rm -fr",
    "rm -rf /",
    "del /f",
    "del /s",
    "rmdir /s",
    "format c:",
    "mkfs",
    "diskpart",
    "dd if=",
    // system power
    "shutdown",
    "reboot",
    "poweroff",
    "halt",
    "init 0",
    "init 6",
    // dangerous permissions
    "chmod 777",
    "chmod -r 777",
    "chown -r",
    // device writes
    "> /dev/sd",
    "> /dev/null",
    // fork bombs & shell injection
    ":(){ :|:&",
    "| sh",
    "| bash",
    "| zsh",
    "|sh",
    "|bash",
    // encoded payload execution
    "| base64 -d",
    "|base64 -d",
    "base64 -d |",
    "base64 --decode |",
    "python -c",
    "python3 -c",
    "perl -e",
    "ruby -e",
    "node -e",
    // network exfiltration / reverse shells
    "curl | sh",
    "curl | bash",
    "wget | sh",
    "wget | bash",
    "nc -e",
    "ncat -e",
    "/dev/tcp/",
    // environment manipulation
    "env -i",
    "sudo ",
    "su -",
    "doas ",
];

enum ExecPolicy {
    Allow,
    RequireConfirm(&'static str),
    Deny(&'static str),
}

struct ExecPolicyConfig {
    confirm_unknown: bool,
    auto_allow_prefixes: Vec<String>,
    require_confirm_prefixes: Vec<String>,
    always_deny_prefixes: Vec<String>,
}

impl ExecPolicyConfig {
    fn from_config(config: ExecToolsConfig) -> Self {
        Self {
            confirm_unknown: config.confirm_unknown,
            auto_allow_prefixes: config
                .auto_allow_prefixes
                .iter()
                .map(|s| normalise_command(s))
                .collect(),
            require_confirm_prefixes: config
                .require_confirm_prefixes
                .iter()
                .map(|s| normalise_command(s))
                .collect(),
            always_deny_prefixes: config
                .always_deny_prefixes
                .iter()
                .map(|s| normalise_command(s))
                .collect(),
        }
    }
}

fn starts_with_command(command: &str, prefix: &str) -> bool {
    command == prefix
        || command
            .strip_prefix(prefix)
            .map(|rest| rest.starts_with(' '))
            .unwrap_or(false)
}

fn starts_with_any(command: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|p| starts_with_command(command, p))
}

fn has_write_redirection(command: &str) -> bool {
    command.contains(" >")
        || command.contains(" >>")
        || command.starts_with(">")
        || command.contains("| tee")
}

fn classify_command(normalised: &str, config: &ExecPolicyConfig) -> ExecPolicy {
    if DENY_LIST.iter().any(|marker| normalised.contains(marker)) {
        return ExecPolicy::Deny("Command blocked by safety guard");
    }

    if starts_with_any(normalised, &config.always_deny_prefixes)
        || normalised.contains(" invoke-webrequest ")
        || normalised.starts_with("invoke-webrequest ")
        || normalised.contains(" iwr ")
        || normalised.starts_with("iwr ")
    {
        return ExecPolicy::Deny("Command blocked: high-risk command is not allowed");
    }

    if has_write_redirection(normalised)
        || normalised.contains(" && ")
        || normalised.contains(" || ")
        || normalised.contains(" $(")
        || normalised.contains('`')
        || starts_with_any(normalised, &config.require_confirm_prefixes)
    {
        return ExecPolicy::RequireConfirm(
            "Command requires explicit confirmation due to side effects",
        );
    }

    if starts_with_any(normalised, &config.auto_allow_prefixes) {
        return ExecPolicy::Allow;
    }

    if config.confirm_unknown {
        ExecPolicy::RequireConfirm("Unknown command requires explicit confirmation")
    } else {
        ExecPolicy::Allow
    }
}

async fn read_stream_limited<R>(mut reader: R, max_bytes: usize) -> std::io::Result<(Vec<u8>, bool)>
where
    R: AsyncRead + Unpin,
{
    let mut captured = Vec::with_capacity(max_bytes.min(8192));
    let mut chunk = [0u8; 4096];
    let mut truncated = false;

    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }

        let remaining = max_bytes.saturating_sub(captured.len());
        if remaining == 0 {
            truncated = true;
            continue;
        }

        let take = remaining.min(read);
        captured.extend_from_slice(&chunk[..take]);
        if take < read {
            truncated = true;
        }
    }

    Ok((captured, truncated))
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
                "command": { "type": "string", "description": "Command to execute" },
                "confirm": {
                    "type": "boolean",
                    "description": "Set true to run commands that can change system state"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>, _: &str, _: &str) -> ToolResult {
        let command = match arg_string(&args, "command") {
            Some(v) if !v.is_empty() => v,
            _ => return ToolResult::error("Missing required parameter: command"),
        };

        let normalised = normalise_command(&command);
        let confirm = args
            .get("confirm")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        match classify_command(&normalised, &self.policy) {
            ExecPolicy::Deny(reason) => return ToolResult::error(reason),
            ExecPolicy::RequireConfirm(reason) if !confirm => {
                return ToolResult::error(&format!("{reason}. Re-run with confirm=true."));
            }
            ExecPolicy::Allow | ExecPolicy::RequireConfirm(_) => {}
        }

        let timeout = std::time::Duration::from_secs(30);
        let start = std::time::Instant::now();

        let child = if cfg!(target_os = "windows") {
            let mut cmd = tokio::process::Command::new("cmd");
            cmd.args(["/C", &command]);
            cmd.current_dir(&self.workspace)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            cmd.spawn()
        } else {
            let mut cmd = tokio::process::Command::new("sh");
            cmd.args(["-c", &command]);
            cmd.current_dir(&self.workspace)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            cmd.spawn()
        };

        let mut child = match child {
            Ok(child) => child,
            Err(e) => return ToolResult::error(&format!("Failed to execute command: {}", e)),
        };

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let (Some(stdout), Some(stderr)) = (stdout, stderr) else {
            let _ = child.kill().await;
            return ToolResult::error("Failed to capture command output");
        };
        let stdout_cap = self.stdout_max_bytes;
        let stderr_cap = self.stderr_max_bytes;

        let stdout_task =
            tokio::spawn(async move { read_stream_limited(stdout, stdout_cap).await });
        let stderr_task =
            tokio::spawn(async move { read_stream_limited(stderr, stderr_cap).await });

        let status = match tokio::time::timeout(timeout, child.wait()).await {
            Ok(result) => match result {
                Ok(status) => status,
                Err(e) => return ToolResult::error(&format!("Failed to execute command: {}", e)),
            },
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return ToolResult::error("Command timed out after 30 seconds");
            }
        };

        let (stdout_bytes, stdout_truncated) = match stdout_task.await {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => return ToolResult::error(&format!("Failed to read stdout: {}", e)),
            Err(e) => return ToolResult::error(&format!("stdout task join error: {}", e)),
        };
        let (stderr_bytes, stderr_truncated) = match stderr_task.await {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => return ToolResult::error(&format!("Failed to read stderr: {}", e)),
            Err(e) => return ToolResult::error(&format!("stderr task join error: {}", e)),
        };

        tracing::debug!(
            "exec: elapsed_ms={}, stdout_bytes={}, stderr_bytes={}, stdout_truncated={}, stderr_truncated={}",
            start.elapsed().as_millis(),
            stdout_bytes.len(),
            stderr_bytes.len(),
            stdout_truncated,
            stderr_truncated
        );

        let mut stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
        let mut stderr = String::from_utf8_lossy(&stderr_bytes).to_string();
        if stdout_truncated {
            stdout.push_str("\n[stdout truncated]");
        }
        if stderr_truncated {
            stderr.push_str("\n[stderr truncated]");
        }

        if status.success() {
            ToolResult::new(&stdout)
        } else {
            let mut err_msg = String::from("Command failed");
            if !stderr.trim().is_empty() {
                err_msg.push_str(": ");
                err_msg.push_str(stderr.trim());
            }
            if !stdout.trim().is_empty() {
                err_msg.push('\n');
                err_msg.push_str(stdout.trim());
            }
            ToolResult::error(&err_msg)
        }
    }
}
