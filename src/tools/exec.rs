//! Shell execution tool with hardened safety deny-list.

use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::Value;

use super::{Tool, ToolResult, arg_string};

pub struct ExecTool {
    workspace: PathBuf,
}

impl ExecTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
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

        let normalised = normalise_command(&command);

        for marker in DENY_LIST {
            if normalised.contains(marker) {
                return ToolResult::error("Command blocked by safety guard");
            }
        }

        let timeout = std::time::Duration::from_secs(30);

        let fut = if cfg!(target_os = "windows") {
            tokio::process::Command::new("cmd")
                .args(["/C", &command])
                .current_dir(&self.workspace)
                .output()
        } else {
            tokio::process::Command::new("sh")
                .args(["-c", &command])
                .current_dir(&self.workspace)
                .output()
        };

        let output = match tokio::time::timeout(timeout, fut).await {
            Ok(result) => result,
            Err(_) => return ToolResult::error("Command timed out after 30 seconds"),
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
