//! Persistent cron job store and basic runtime controls.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Schedule {
    Every(u64),
    Cron(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub name: String,
    pub schedule: Schedule,
    pub message: String,
    pub enabled: bool,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub chat_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CronStore {
    #[serde(default)]
    jobs: Vec<Job>,
}

pub struct CronService {
    jobs_path: PathBuf,
    jobs: Vec<Job>,
}

impl CronService {
    pub fn new(path: &Path, _config: Option<&crate::config::Config>) -> Self {
        let jobs_path = path.to_path_buf();
        let jobs = load_jobs(&jobs_path).unwrap_or_default();
        Self { jobs_path, jobs }
    }

    pub fn list_jobs(&self, enabled_only: bool) -> Vec<Job> {
        let mut jobs: Vec<Job> = self
            .jobs
            .iter()
            .filter(|j| !enabled_only || j.enabled)
            .cloned()
            .collect();
        jobs.sort_by(|a, b| a.name.cmp(&b.name));
        jobs
    }

    pub fn add_job(
        &mut self,
        name: &str,
        schedule: Schedule,
        message: &str,
        enabled: bool,
        channel: Option<&str>,
        chat_id: Option<&str>,
    ) -> Result<Job> {
        if name.trim().is_empty() {
            return Err(anyhow!("job name cannot be empty"));
        }
        if message.trim().is_empty() {
            return Err(anyhow!("job message cannot be empty"));
        }
        let job = Job {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.trim().to_string(),
            schedule,
            message: message.to_string(),
            enabled,
            channel: channel.map(|s| s.to_string()),
            chat_id: chat_id.map(|s| s.to_string()),
        };
        self.jobs.push(job.clone());
        self.save()?;
        Ok(job)
    }

    pub fn remove_job(&mut self, job_id: &str) -> bool {
        let before = self.jobs.len();
        self.jobs.retain(|j| j.id != job_id);
        let changed = self.jobs.len() != before;
        if changed {
            let _ = self.save();
        }
        changed
    }

    pub fn enable_job(&mut self, job_id: &str, enabled: bool) -> Option<Job> {
        let job = self.jobs.iter_mut().find(|j| j.id == job_id)?;
        job.enabled = enabled;
        let result = job.clone();
        let _ = self.save();
        Some(result)
    }

    pub fn start(&self) -> Result<()> {
        Ok(())
    }

    pub fn stop(&self) {}

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.jobs_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let store = CronStore {
            jobs: self.jobs.clone(),
        };
        let raw = serde_json::to_string_pretty(&store)?;
        let dir = self
            .jobs_path
            .parent()
            .ok_or_else(|| anyhow!("invalid jobs path"))?;
        let tmp = tempfile::NamedTempFile::new_in(dir)?;
        std::fs::write(tmp.path(), raw)?;
        tmp.persist(&self.jobs_path)?;
        Ok(())
    }
}

fn load_jobs(path: &Path) -> Result<Vec<Job>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(path)?;
    let parsed = serde_json::from_str::<CronStore>(&raw)
        .or_else(|_| serde_json::from_str::<Vec<Job>>(&raw).map(|jobs| CronStore { jobs }))?;
    Ok(parsed.jobs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn add_and_reload_jobs() {
        let tmp = TempDir::new().expect("tmp dir");
        let path = tmp.path().join("cron/jobs.json");

        let mut service = CronService::new(&path, None);
        let created = service
            .add_job(
                "ping",
                Schedule::Every(60),
                "hello",
                true,
                Some("telegram"),
                Some("123"),
            )
            .expect("add job");
        assert_eq!(service.list_jobs(false).len(), 1);
        assert_eq!(created.name, "ping");

        let service2 = CronService::new(&path, None);
        assert_eq!(service2.list_jobs(false).len(), 1);
    }
}
