//! Session module - Session management for conversations
//! Ported from Go version

use std::collections::HashMap;
use std::path::PathBuf;

use crate::providers::Message as ProviderMessage;

/// Session manager
pub struct SessionManager {
    sessions_dir: PathBuf,
    sessions: HashMap<String, Session>,
}

struct Session {
    messages: Vec<ProviderMessage>,
    summary: String,
}

impl SessionManager {
    pub fn new(sessions_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&sessions_dir).ok();
        Self {
            sessions_dir,
            sessions: HashMap::new(),
        }
    }

    /// Get or create a session
    fn get_or_create(&mut self, key: &str) -> &mut Session {
        self.sessions
            .entry(key.to_string())
            .or_insert_with(|| Session {
                messages: Vec::new(),
                summary: String::new(),
            })
    }

    /// Add a message to a session
    pub fn add_message(&mut self, key: &str, role: &str, content: &str) {
        let session = self.get_or_create(key);
        session.messages.push(ProviderMessage {
            role: role.to_string(),
            content: content.to_string(),
            tool_calls: vec![],
            tool_call_id: None,
        });
    }

    /// Add a full message (with tool calls)
    pub fn add_full_message(&mut self, key: &str, msg: ProviderMessage) {
        let session = self.get_or_create(key);
        session.messages.push(msg);
    }

    /// Get session history
    pub fn get_history(&self, key: &str) -> Vec<ProviderMessage> {
        self.sessions
            .get(key)
            .map(|s| s.messages.clone())
            .unwrap_or_default()
    }

    /// Get session summary
    pub fn get_summary(&self, key: &str) -> String {
        self.sessions
            .get(key)
            .map(|s| s.summary.clone())
            .unwrap_or_default()
    }

    /// Save session to disk
    pub fn save(&self, key: &str) -> anyhow::Result<()> {
        if let Some(session) = self.sessions.get(key) {
            // Only save if there are messages
            if session.messages.is_empty() {
                return Ok(());
            }

            let path = self.session_file_path(key);

            // Create directory if needed
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let data = serde_json::to_string_pretty(&session.messages)?;
            let temp = tempfile::NamedTempFile::new_in(&self.sessions_dir)?;
            std::fs::write(temp.path(), data)?;
            temp.persist(path)?;
        }
        Ok(())
    }

    fn session_file_path(&self, key: &str) -> PathBuf {
        let safe = sanitize_session_key(key);
        self.sessions_dir.join(format!("{}.json", safe))
    }
}

fn sanitize_session_key(key: &str) -> String {
    key.replace([':', '/', '\\'], "_")
}
