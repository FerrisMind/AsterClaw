use crate::providers::Message as ProviderMessage;
use std::collections::HashMap;
use std::path::PathBuf;
const MAX_HISTORY: usize = 200;
pub struct SessionManager {
    sessions_dir: PathBuf,
    sessions: HashMap<String, Session>,
}
struct Session {
    messages: Vec<ProviderMessage>,
    summary: String,
    dirty: bool,
}
impl SessionManager {
    pub fn new(sessions_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&sessions_dir).ok();
        Self {
            sessions_dir,
            sessions: HashMap::new(),
        }
    }
    fn get_or_create(&mut self, key: &str) -> &mut Session {
        if !self.sessions.contains_key(key) {
            let session = self.try_load_from_disk(key).unwrap_or(Session {
                messages: Vec::new(),
                summary: String::new(),
                dirty: false,
            });
            self.sessions.insert(key.to_string(), session);
        }
        self.sessions.get_mut(key).expect("just inserted")
    }
    fn try_load_from_disk(&self, key: &str) -> Option<Session> {
        let path = self.session_file_path(key);
        let data = std::fs::read_to_string(&path).ok()?;
        let messages: Vec<ProviderMessage> = serde_json::from_str(&data).ok()?;
        Some(Session {
            messages,
            summary: String::new(),
            dirty: false,
        })
    }
    fn trim_history(messages: &mut Vec<ProviderMessage>) {
        if messages.len() > MAX_HISTORY {
            let drain_count = messages.len() - MAX_HISTORY;
            messages.drain(..drain_count);
        }
    }
    pub fn add_message(&mut self, key: &str, role: &str, content: &str) {
        let session = self.get_or_create(key);
        session.messages.push(ProviderMessage {
            role: role.to_string(),
            content: content.to_string(),
            tool_calls: vec![],
            tool_call_id: None,
        });
        Self::trim_history(&mut session.messages);
        session.dirty = true;
    }
    pub fn add_full_message(&mut self, key: &str, msg: ProviderMessage) {
        let session = self.get_or_create(key);
        session.messages.push(msg);
        Self::trim_history(&mut session.messages);
        session.dirty = true;
    }
    pub fn get_history(&mut self, key: &str) -> Vec<ProviderMessage> {
        let session = self.get_or_create(key);
        session.messages.clone()
    }
    pub fn get_summary(&mut self, key: &str) -> String {
        let session = self.get_or_create(key);
        session.summary.clone()
    }
    pub fn save(&mut self, key: &str) -> anyhow::Result<()> {
        let path = self.session_file_path(key);
        if let Some(session) = self.sessions.get_mut(key) {
            if !session.dirty {
                return Ok(());
            }
            if session.messages.is_empty() {
                session.dirty = false;
                return Ok(());
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let data = serde_json::to_vec(&session.messages)?;
            let temp = tempfile::NamedTempFile::new_in(&self.sessions_dir)?;
            std::fs::write(temp.path(), data)?;
            temp.persist(path)?;
            session.dirty = false;
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
