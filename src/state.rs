//! Persistent session state management.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedState {
    #[serde(default)]
    last_channel: Option<String>,
}

pub struct Manager {
    workspace: PathBuf,
    state_file: PathBuf,
    data: Mutex<PersistedState>,
}

impl Manager {
    pub fn new(workspace: PathBuf) -> Self {
        let state_dir = workspace.join("state");
        let state_file = state_dir.join("state.json");
        let _ = std::fs::create_dir_all(&state_dir);

        let data = if state_file.exists() {
            std::fs::read_to_string(&state_file)
                .ok()
                .and_then(|raw| serde_json::from_str::<PersistedState>(&raw).ok())
                .unwrap_or_default()
        } else {
            PersistedState::default()
        };

        Self {
            workspace,
            state_file,
            data: Mutex::new(data),
        }
    }

    pub fn set_last_channel(&self, channel: &str) {
        self.data.lock().last_channel = Some(channel.to_string());
        let _ = self.save_atomic();
    }

    pub fn get_last_channel(&self) -> String {
        self.data
            .lock()
            .last_channel
            .as_ref()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    fn save_atomic(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.state_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let snapshot = self.data.lock().clone();
        let raw = serde_json::to_string_pretty(&snapshot)?;
        let temp = tempfile::NamedTempFile::new_in(
            self.state_file
                .parent()
                .ok_or_else(|| anyhow::anyhow!("invalid state path"))?,
        )?;
        std::fs::write(temp.path(), raw)?;
        temp.persist(&self.state_file)?;
        Ok(())
    }
}

impl Clone for Manager {
    fn clone(&self) -> Self {
        Self::new(self.workspace.clone())
    }
}

/// Parse a `"platform:user_id"` string into `(platform, user_id)`.
/// Returns `None` if the format is invalid.
pub fn parse_last_channel(raw: &str) -> Option<(&str, &str)> {
    if raw.is_empty() {
        return None;
    }
    let (platform, user_id) = raw.split_once(':')?;
    if platform.is_empty() || user_id.is_empty() {
        return None;
    }
    Some((platform, user_id))
}
