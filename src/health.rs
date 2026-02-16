//! Health HTTP server with liveness/readiness endpoints.

use anyhow::Result;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use parking_lot::Mutex;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

#[derive(Clone)]
struct HealthState {
    ready: Arc<AtomicBool>,
}

async fn health_handler() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ok")
}

async fn ready_handler(State(state): State<HealthState>) -> (StatusCode, &'static str) {
    if state.ready.load(Ordering::SeqCst) {
        (StatusCode::OK, "ready")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "not-ready")
    }
}

pub struct HealthServer {
    host: String,
    port: i32,
    ready: Arc<AtomicBool>,
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl HealthServer {
    pub fn new(host: &str, port: i32) -> Self {
        Self {
            host: host.to_string(),
            port,
            ready: Arc::new(AtomicBool::new(false)),
            shutdown_tx: Mutex::new(None),
            handle: Mutex::new(None),
        }
    }

    pub async fn start(&self) -> Result<()> {
        if self.handle.lock().is_some() {
            return Ok(());
        }

        let state = HealthState {
            ready: self.ready.clone(),
        };
        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/ready", get(ready_handler))
            .with_state(state);

        let addr: SocketAddr = format!("{}:{}", self.host, self.port).parse()?;
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let (tx, rx) = oneshot::channel::<()>();

        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = rx.await;
        });

        let ready_flag = self.ready.clone();
        let handle = tokio::spawn(async move {
            ready_flag.store(true, Ordering::SeqCst);
            if let Err(err) = server.await {
                tracing::error!("health server failed: {}", err);
            }
        });

        *self.shutdown_tx.lock() = Some(tx);
        *self.handle.lock() = Some(handle);
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        self.ready.store(false, Ordering::SeqCst);

        if let Some(tx) = self.shutdown_tx.lock().take() {
            let _ = tx.send(());
        }

        let handle = { self.handle.lock().take() };
        if let Some(handle) = handle {
            let _ = handle.await;
        }

        Ok(())
    }
}
