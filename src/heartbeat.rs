//! Heartbeat service with HEARTBEAT.md prompt execution.

use crate::bus::{InboundMessage, MessageBus};
use crate::constants;
use crate::state;
use anyhow::Result;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const MIN_INTERVAL_MINUTES: u64 = 5;
const DEFAULT_INTERVAL_MINUTES: u64 = 30;

pub struct HeartbeatService {
    workspace: PathBuf,
    interval: u64,
    enabled: bool,
    bus: Mutex<Option<Arc<MessageBus>>>,
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl HeartbeatService {
    pub fn new(workspace: PathBuf, interval: u64, enabled: bool) -> Self {
        let interval = if interval == 0 {
            DEFAULT_INTERVAL_MINUTES
        } else {
            interval.max(MIN_INTERVAL_MINUTES)
        };
        Self {
            workspace,
            interval,
            enabled,
            bus: Mutex::new(None),
            shutdown_tx: Mutex::new(None),
            handle: Mutex::new(None),
        }
    }

    pub fn set_bus(&self, bus: &Arc<MessageBus>) {
        *self.bus.lock() = Some(bus.clone());
    }

    pub fn start(&self) -> Result<()> {
        if self.handle.lock().is_some() {
            return Ok(());
        }
        if !self.enabled {
            tracing::info!("heartbeat disabled");
            return Ok(());
        }

        let workspace = self.workspace.clone();
        let state_manager = state::Manager::new(self.workspace.clone());
        let bus = self.bus.lock().clone();
        let interval = self.interval;
        let (tx, mut rx) = oneshot::channel::<()>();

        let handle = tokio::spawn(async move {
            // Initial run with short delay.
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let _ = execute_heartbeat(&workspace, &state_manager, bus.as_ref()).await;

            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval * 60));
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let _ = execute_heartbeat(&workspace, &state_manager, bus.as_ref()).await;
                    }
                    _ = &mut rx => break,
                }
            }
        });

        *self.shutdown_tx.lock() = Some(tx);
        *self.handle.lock() = Some(handle);
        Ok(())
    }

    pub async fn stop(&self) {
        if let Some(tx) = self.shutdown_tx.lock().take() {
            let _ = tx.send(());
        }
        let handle = self.handle.lock().take();
        if let Some(handle) = handle {
            let _ = handle.await;
        }
    }
}

async fn execute_heartbeat(
    workspace: &std::path::Path,
    state: &state::Manager,
    bus: Option<&Arc<MessageBus>>,
) -> Result<()> {
    log_info(workspace, "Executing heartbeat");
    let prompt = build_prompt(workspace)?;
    if prompt.is_empty() {
        log_info(
            workspace,
            "No heartbeat prompt (HEARTBEAT.md empty or missing)",
        );
        return Ok(());
    }
    let Some(bus) = bus else {
        tracing::debug!("heartbeat bus not configured");
        log_info(
            workspace,
            "No message bus configured, heartbeat result not sent",
        );
        return Ok(());
    };

    let last_channel = state.get_last_channel();
    let Some((platform, user_id)) = state::parse_last_channel(&last_channel) else {
        tracing::debug!("heartbeat no routable channel");
        log_info(
            workspace,
            "No last channel recorded, heartbeat result not sent",
        );
        return Ok(());
    };
    if constants::is_internal_channel(platform) {
        tracing::debug!("heartbeat internal channel");
        log_info(workspace, "Internal channel, heartbeat result not sent");
        return Ok(());
    }

    let msg = InboundMessage {
        channel: "system".to_string(),
        sender_id: "heartbeat".to_string(),
        chat_id: format!("{platform}:{user_id}"),
        content: prompt,
        media: None,
        session_key: format!("system:heartbeat:{platform}:{user_id}"),
        metadata: None,
    };
    if let Err(err) = bus.publish_inbound(msg).await {
        log_error(
            workspace,
            &format!("Failed to publish heartbeat message: {}", err),
        );
    } else {
        log_info(workspace, &format!("Heartbeat prompt sent to {}", platform));
    }
    Ok(())
}

fn build_prompt(workspace: &std::path::Path) -> Result<String> {
    let heartbeat_path = workspace.join("HEARTBEAT.md");
    if !heartbeat_path.exists() {
        create_default_template(&heartbeat_path)?;
        return Ok(String::new());
    }

    let content = std::fs::read_to_string(&heartbeat_path).unwrap_or_default();
    if content.trim().is_empty() {
        return Ok(String::new());
    }

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    Ok(format!(
        "# Heartbeat Check\n\nCurrent time: {now}\n\nYou are a proactive AI assistant. This is a scheduled heartbeat check.\nReview the following tasks and execute any necessary actions using available skills.\nIf there is nothing that requires attention, respond ONLY with: HEARTBEAT_OK\n\n{content}\n"
    ))
}

fn create_default_template(path: &std::path::Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let template = "# Heartbeat Check List\n\nThis file contains tasks for the heartbeat service to check periodically.\n\n## Examples\n\n- Check for unread messages\n- Review upcoming calendar events\n- Check device status (e.g., MaixCam)\n\n## Instructions\n\n- Execute ALL tasks listed below. Do NOT skip any task.\n- For simple tasks (e.g., report current time), respond directly.\n- For complex tasks that may take time, use the spawn tool to create a subagent.\n- The spawn tool is async - subagent results will be sent to the user automatically.\n- After spawning a subagent, CONTINUE to process remaining tasks.\n- Only respond with HEARTBEAT_OK when ALL tasks are done AND nothing needs attention.\n\n---\n\nAdd your heartbeat tasks below this line:\n";
    std::fs::write(path, template)?;
    if let Some(parent) = path.parent() {
        log_info(parent, "Created default HEARTBEAT.md template");
    }
    Ok(())
}

fn log(workspace: &std::path::Path, level: &str, msg: &str) {
    let path = workspace.join("heartbeat.log");
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let line = format!("[{}] [{}] {}\n", ts, level, msg);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
    {
        let _ = std::io::Write::write_all(&mut f, line.as_bytes());
    }
}

fn log_info(workspace: &std::path::Path, msg: &str) {
    log(workspace, "INFO", msg);
}

fn log_error(workspace: &std::path::Path, msg: &str) {
    log(workspace, "ERROR", msg);
}

#[cfg(test)]
mod tests {
    use super::build_prompt;
    use crate::state;

    #[test]
    fn parse_last_channel_works() {
        let (p, u) = state::parse_last_channel("telegram:123").expect("valid");
        assert_eq!(p, "telegram");
        assert_eq!(u, "123");
    }

    #[test]
    fn missing_heartbeat_file_creates_template() {
        let dir = tempfile::tempdir().expect("tmp");
        let prompt = build_prompt(dir.path()).expect("build");
        assert!(prompt.is_empty());
        assert!(dir.path().join("HEARTBEAT.md").exists());
    }
}
