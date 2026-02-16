//! Minimal heartbeat service for gateway runtime.

use crate::bus::{InboundMessage, MessageBus};
use anyhow::Result;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

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
        if !self.enabled || self.handle.lock().is_some() {
            return Ok(());
        }

        let bus = self.bus.lock().clone();
        let interval = self.interval.max(5);
        let workspace = self.workspace.clone();
        let (tx, mut rx) = oneshot::channel::<()>();

        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval));
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        tracing::debug!("heartbeat tick: workspace={}", workspace.display());
                        if let Some(bus) = &bus {
                            let _ = bus.publish_inbound(InboundMessage {
                                channel: "system".to_string(),
                                sender_id: "heartbeat".to_string(),
                                chat_id: "heartbeat".to_string(),
                                content: "heartbeat".to_string(),
                                media: None,
                                session_key: "system:heartbeat".to_string(),
                                metadata: None,
                            }).await;
                        }
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
        let handle = { self.handle.lock().take() };
        if let Some(handle) = handle {
            let _ = handle.await;
        }
    }
}
