use crate::bus::{MessageBus, OutboundMessage};
use anyhow::{Result, anyhow};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "value")]
pub enum Schedule {
    At(i64),
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
    #[serde(default = "default_true")]
    pub deliver: bool,
    #[serde(default)]
    pub delete_after_run: bool,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub chat_id: Option<String>,
    #[serde(default)]
    pub next_run_at_ms: Option<i64>,
    #[serde(default)]
    pub last_run_at_ms: Option<i64>,
}
fn default_true() -> bool {
    true
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
    #[allow(clippy::too_many_arguments)]
    pub fn add_job(
        &mut self,
        name: &str,
        schedule: Schedule,
        message: &str,
        enabled: bool,
        deliver: bool,
        channel: Option<&str>,
        chat_id: Option<&str>,
    ) -> Result<Job> {
        if name.trim().is_empty() {
            return Err(anyhow!("job name cannot be empty"));
        }
        if message.trim().is_empty() {
            return Err(anyhow!("job message cannot be empty"));
        }
        let now_ms = now_ms();
        let delete_after_run = matches!(schedule, Schedule::At(_));
        let next_run = compute_next_run(&schedule, now_ms);
        let job = Job {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.trim().to_string(),
            schedule,
            message: message.to_string(),
            enabled,
            deliver,
            delete_after_run,
            channel: channel.map(|s| s.to_string()),
            chat_id: chat_id.map(|s| s.to_string()),
            next_run_at_ms: next_run,
            last_run_at_ms: None,
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
        if enabled {
            job.next_run_at_ms = compute_next_run(&job.schedule, now_ms());
        } else {
            job.next_run_at_ms = None;
        }
        let result = job.clone();
        let _ = self.save();
        Some(result)
    }
    pub fn take_due_jobs(&mut self) -> Vec<Job> {
        let now = now_ms();
        let mut due = Vec::new();
        for job in &mut self.jobs {
            if job.enabled
                && let Some(next) = job.next_run_at_ms
                && next <= now
            {
                due.push(job.clone());
                job.next_run_at_ms = None;
            }
        }
        if !due.is_empty() {
            let _ = self.save();
        }
        due
    }
    pub fn mark_executed(&mut self, job_id: &str, success: bool) {
        let now = now_ms();
        let should_delete = self
            .jobs
            .iter()
            .find(|j| j.id == job_id)
            .map(|j| j.delete_after_run)
            .unwrap_or(false);
        if should_delete {
            self.jobs.retain(|j| j.id != job_id);
        } else if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job_id) {
            job.last_run_at_ms = Some(now);
            if success {
                job.next_run_at_ms = compute_next_run(&job.schedule, now);
                if matches!(job.schedule, Schedule::At(_)) {
                    job.enabled = false;
                    job.next_run_at_ms = None;
                }
            }
        }
        let _ = self.save();
    }
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
pub struct CronRunner {
    service: Arc<Mutex<CronService>>,
    bus: Arc<MessageBus>,
    running: Arc<AtomicBool>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}
impl CronRunner {
    pub fn new(service: Arc<Mutex<CronService>>, bus: Arc<MessageBus>) -> Self {
        Self {
            service,
            bus,
            running: Arc::new(AtomicBool::new(false)),
            task: Mutex::new(None),
        }
    }
    pub fn start(&self) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let service = self.service.clone();
        let bus = self.bus.clone();
        let running = self.running.clone();
        let handle = tokio::spawn(async move {
            tracing::info!("cron runner started");
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            while running.load(Ordering::Relaxed) {
                interval.tick().await;
                let due_jobs = service.lock().take_due_jobs();
                for job in due_jobs {
                    let channel = job.channel.clone().unwrap_or_else(|| "cli".to_string());
                    let chat_id = job.chat_id.clone().unwrap_or_else(|| "direct".to_string());
                    tracing::info!(
                        "cron firing job '{}' (id={}) → {}:{}",
                        job.name,
                        job.id,
                        channel,
                        chat_id
                    );
                    if job.deliver {
                        let content = format!("⏰ {}", job.message);
                        if let Err(err) = bus
                            .publish_outbound(OutboundMessage {
                                channel: channel.clone(),
                                chat_id: chat_id.clone(),
                                content,
                            })
                            .await
                        {
                            tracing::error!("cron delivery failed: {}", err);
                            service.lock().mark_executed(&job.id, false);
                            continue;
                        }
                    } else {
                        let content = format!("[Scheduled task '{}'] {}", job.name, job.message);
                        if let Err(err) = bus
                            .publish_inbound(crate::bus::InboundMessage {
                                channel: channel.clone(),
                                sender_id: "system:cron".to_string(),
                                chat_id: chat_id.clone(),
                                content,
                                session_key: format!("cron-{}", job.id),
                                media: None,
                                metadata: None,
                            })
                            .await
                        {
                            tracing::error!("cron inbound publish failed: {}", err);
                            service.lock().mark_executed(&job.id, false);
                            continue;
                        }
                    }
                    service.lock().mark_executed(&job.id, true);
                }
            }
            tracing::info!("cron runner stopped");
        });
        *self.task.lock() = Some(handle);
    }
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.task.lock().take() {
            handle.abort();
        }
    }
}
fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
fn compute_next_run(schedule: &Schedule, now_ms: i64) -> Option<i64> {
    match schedule {
        Schedule::At(at_ms) => {
            if *at_ms > now_ms {
                Some(*at_ms)
            } else {
                Some(now_ms)
            }
        }
        Schedule::Every(interval_ms) => {
            if *interval_ms == 0 {
                return None;
            }
            Some(now_ms + *interval_ms as i64)
        }
        Schedule::Cron(expr) => match expr.parse::<cron::Schedule>() {
            Ok(sched) => sched
                .upcoming(chrono::Utc)
                .next()
                .map(|dt| dt.timestamp_millis()),
            Err(err) => {
                tracing::warn!("invalid cron expression '{}': {}", expr, err);
                None
            }
        },
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
                Schedule::Every(60_000),
                "hello",
                true,
                true,
                Some("telegram"),
                Some("123"),
            )
            .expect("add job");
        assert_eq!(service.list_jobs(false).len(), 1);
        assert_eq!(created.name, "ping");
        assert!(created.next_run_at_ms.is_some());
        let service2 = CronService::new(&path, None);
        assert_eq!(service2.list_jobs(false).len(), 1);
    }
    #[test]
    fn at_schedule_fires_and_deletes() {
        let tmp = TempDir::new().expect("tmp dir");
        let path = tmp.path().join("cron/jobs.json");
        let mut service = CronService::new(&path, None);
        let past = now_ms() - 1000;
        let job = service
            .add_job("once", Schedule::At(past), "do it", true, true, None, None)
            .expect("add");
        assert!(job.delete_after_run);
        let due = service.take_due_jobs();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, job.id);
        service.mark_executed(&job.id, true);
        assert_eq!(service.list_jobs(false).len(), 0);
    }
    #[test]
    fn every_schedule_recomputes_next_run() {
        let tmp = TempDir::new().expect("tmp dir");
        let path = tmp.path().join("cron/jobs.json");
        let mut service = CronService::new(&path, None);
        let job = service
            .add_job(
                "recurring",
                Schedule::Every(60_000),
                "hello",
                true,
                true,
                None,
                None,
            )
            .expect("add");
        service.jobs[0].next_run_at_ms = Some(now_ms() - 100);
        let due = service.take_due_jobs();
        assert_eq!(due.len(), 1);
        service.mark_executed(&job.id, true);
        let jobs = service.list_jobs(false);
        assert_eq!(jobs.len(), 1);
        assert!(jobs[0].next_run_at_ms.is_some());
    }
    #[test]
    fn enable_disable_cycle() {
        let tmp = TempDir::new().expect("tmp dir");
        let path = tmp.path().join("cron/jobs.json");
        let mut service = CronService::new(&path, None);
        let job = service
            .add_job(
                "test",
                Schedule::Every(60_000),
                "msg",
                true,
                true,
                None,
                None,
            )
            .expect("add");
        let disabled = service.enable_job(&job.id, false).expect("disable");
        assert!(!disabled.enabled);
        assert!(disabled.next_run_at_ms.is_none());
        let enabled = service.enable_job(&job.id, true).expect("enable");
        assert!(enabled.enabled);
        assert!(enabled.next_run_at_ms.is_some());
    }
}
