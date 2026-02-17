//! Persistent memory store for long-term notes and daily notes.

use std::path::PathBuf;

pub struct MemoryStore {
    memory_dir: PathBuf,
    memory_file: PathBuf,
}

impl MemoryStore {
    pub fn new(workspace: PathBuf) -> Self {
        let memory_dir = workspace.join("memory");
        let memory_file = memory_dir.join("MEMORY.md");
        let _ = std::fs::create_dir_all(&memory_dir);
        Self {
            memory_dir,
            memory_file,
        }
    }

    fn today_file(&self) -> PathBuf {
        let today = chrono::Local::now().format("%Y%m%d").to_string();
        let month = &today[..6];
        self.memory_dir.join(month).join(format!("{today}.md"))
    }

    pub fn read_long_term(&self) -> String {
        std::fs::read_to_string(&self.memory_file).unwrap_or_default()
    }

    pub fn write_long_term(&self, content: &str) -> anyhow::Result<()> {
        if let Some(parent) = self.memory_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.memory_file, content)?;
        Ok(())
    }

    pub fn read_today(&self) -> String {
        std::fs::read_to_string(self.today_file()).unwrap_or_default()
    }

    pub fn append_today(&self, content: &str) -> anyhow::Result<()> {
        let today_file = self.today_file();
        if let Some(parent) = today_file.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let existing = std::fs::read_to_string(&today_file).unwrap_or_default();
        let next = if existing.is_empty() {
            format!(
                "# {}\n\n{}",
                chrono::Local::now().format("%Y-%m-%d"),
                content
            )
        } else {
            format!("{existing}\n{content}")
        };
        std::fs::write(today_file, next)?;
        Ok(())
    }

    pub fn get_recent_daily_notes(&self, days: usize) -> String {
        let mut notes = Vec::new();
        for i in 0..days {
            let date = chrono::Local::now() - chrono::Duration::days(i as i64);
            let ymd = date.format("%Y%m%d").to_string();
            let month = &ymd[..6];
            let path = self.memory_dir.join(month).join(format!("{ymd}.md"));
            let content = std::fs::read_to_string(path).unwrap_or_default();
            if !content.is_empty() {
                notes.push(content);
            }
        }
        notes.join("\n\n---\n\n")
    }

    pub fn get_memory_context(&self) -> String {
        let mut sections = Vec::new();

        let long_term = self.read_long_term();
        if !long_term.is_empty() {
            sections.push(format!("## Long-term Memory\n\n{long_term}"));
        }

        let recent = self.get_recent_daily_notes(3);
        if !recent.is_empty() {
            sections.push(format!("## Recent Daily Notes\n\n{recent}"));
        }

        if sections.is_empty() {
            return String::new();
        }

        format!("# Memory\n\n{}", sections.join("\n\n---\n\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::MemoryStore;

    #[test]
    fn memory_roundtrip_long_term_and_daily() {
        let dir = tempfile::tempdir().expect("tmp");
        let store = MemoryStore::new(dir.path().to_path_buf());

        store
            .write_long_term("remember this")
            .expect("write long term");
        assert_eq!(store.read_long_term(), "remember this");

        store.append_today("first").expect("append 1");
        store.append_today("second").expect("append 2");
        let today = store.read_today();
        assert!(today.contains("first"));
        assert!(today.contains("second"));
    }

    #[test]
    fn memory_context_contains_sections() {
        let dir = tempfile::tempdir().expect("tmp");
        let store = MemoryStore::new(dir.path().to_path_buf());
        store.write_long_term("LT").expect("write");
        store.append_today("D1").expect("append");
        let ctx = store.get_memory_context();
        assert!(ctx.contains("Long-term Memory"));
        assert!(ctx.contains("Recent Daily Notes"));
        assert!(ctx.contains("LT"));
        assert!(ctx.contains("D1"));
    }
}
