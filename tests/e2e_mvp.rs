use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;

fn femtors_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_femtors"))
}

fn run_femtors(home: &Path, args: &[&str]) -> anyhow::Result<std::process::Output> {
    let mut child = Command::new(femtors_bin())
        .args(args)
        .env("FEMTORS_HOME", home)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match child.try_wait()? {
            Some(_status) => return Ok(child.wait_with_output()?),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let out = child.wait_with_output()?;
                anyhow::bail!(
                    "femtors timed out after 15s.\nstdout: {}\nstderr: {}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn write_config(
    home: &Path,
    workspace: &Path,
    api_base: Option<&str>,
    gateway_port: i32,
) -> anyhow::Result<()> {
    let cfg_dir = home.join(".femtors");
    std::fs::create_dir_all(&cfg_dir)?;
    std::fs::create_dir_all(workspace)?;
    let cfg = json!({
        "agents": {
            "defaults": {
                "provider": "openai",
                "model": "gpt-4o-mini",
                "workspace": workspace.to_string_lossy().to_string(),
                "maxToolIterations": 5
            }
        },
        "providers": {
            "openai": {
                "apiKey": "test-key",
                "apiBase": api_base.unwrap_or("http://127.0.0.1:1")
            }
        },
        "channels": {
            "telegram": {
                "enabled": false,
                "token": ""
            }
        },
        "heartbeat": {
            "enabled": false,
            "interval": 30
        },
        "devices": {
            "enabled": false,
            "monitorUsb": false
        },
        "gateway": {
            "host": "127.0.0.1",
            "port": gateway_port
        }
    });
    std::fs::write(
        cfg_dir.join("config.json"),
        serde_json::to_string_pretty(&cfg)?,
    )?;
    Ok(())
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    listener.local_addr().expect("local addr").port()
}

async fn start_mock_openai_server() -> anyhow::Result<(String, tokio::sync::oneshot::Sender<()>)> {
    async fn chat() -> Json<serde_json::Value> {
        Json(json!({
            "choices": [{
                "message": { "content": "mock-e2e-response" },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 1,
                "completion_tokens": 1,
                "total_tokens": 2
            }
        }))
    }

    let app = Router::new().route("/chat/completions", post(chat));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = rx.await;
        });
        let _ = server.await;
    });
    Ok((format!("http://{}", addr), tx))
}

#[test]
fn e2e_onboard_and_status() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path();

    let onboard = run_femtors(home, &["onboard"])?;
    assert!(
        onboard.status.success(),
        "{}",
        String::from_utf8_lossy(&onboard.stderr)
    );

    assert!(home.join(".femtors/config.json").exists());
    assert!(home.join(".femtors/workspace/memory/MEMORY.md").exists());
    assert!(home.join(".femtors/workspace/cron").exists());

    let status = run_femtors(home, &["status"])?;
    assert!(status.status.success());
    let stdout = String::from_utf8_lossy(&status.stdout);
    assert!(stdout.contains("Config:"));
    assert!(stdout.contains("Workspace:"));
    Ok(())
}

#[test]
fn e2e_auth_primary_and_legacy_fallback() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path();

    let legacy_dir = home.join(".femtors");
    std::fs::create_dir_all(&legacy_dir)?;
    std::fs::write(
        legacy_dir.join("credentials.json"),
        json!({
            "credentials": {
                "openai": {
                    "provider": "openai",
                    "api_key": "legacy-key",
                    "access_token": null,
                    "refresh_token": null,
                    "expires_at": null,
                    "account_id": null,
                    "auth_method": "token"
                }
            }
        })
        .to_string(),
    )?;

    let status_legacy = run_femtors(home, &["auth", "status"])?;
    assert!(status_legacy.status.success());
    assert!(String::from_utf8_lossy(&status_legacy.stdout).contains("openai"));

    let login = run_femtors(
        home,
        &[
            "auth",
            "login",
            "--provider",
            "openai",
            "--token",
            "new-key",
        ],
    )?;
    assert!(login.status.success());
    assert!(home.join(".femtors/credentials.json").exists());

    let logout = run_femtors(home, &["auth", "logout", "--provider", "openai"])?;
    assert!(logout.status.success());
    let status_after = run_femtors(home, &["auth", "status"])?;
    assert!(status_after.status.success());
    assert!(String::from_utf8_lossy(&status_after.stdout).contains("No authenticated providers."));
    Ok(())
}

#[test]
fn e2e_cron_lifecycle() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path();
    let workspace = home.join("ws");
    write_config(home, &workspace, None, 18790)?;

    let add = run_femtors(
        home,
        &[
            "cron",
            "add",
            "--name",
            "job1",
            "--message",
            "ping",
            "--every",
            "60",
            "--channel",
            "telegram",
            "--chat-id",
            "123",
        ],
    )?;
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    let added = String::from_utf8_lossy(&add.stdout);
    assert!(added.contains("Added cron job: job1"));

    let id = added
        .split('(')
        .nth(1)
        .and_then(|s| s.split(')').next())
        .unwrap_or("")
        .trim()
        .to_string();
    assert!(
        !id.is_empty(),
        "cron add output did not contain id: {added}"
    );

    let list = run_femtors(home, &["cron", "list"])?;
    assert!(list.status.success());
    assert!(String::from_utf8_lossy(&list.stdout).contains("job1"));

    let disable = run_femtors(home, &["cron", "disable", &id])?;
    assert!(disable.status.success());
    let enable = run_femtors(home, &["cron", "enable", &id])?;
    assert!(enable.status.success());

    let remove = run_femtors(home, &["cron", "remove", &id])?;
    assert!(remove.status.success());
    assert!(String::from_utf8_lossy(&remove.stdout).contains("Removed cron job"));
    Ok(())
}

#[tokio::test]
async fn e2e_agent_with_mock_provider() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().to_path_buf();
    let workspace = home.join("ws");
    let (api_base, shutdown) = start_mock_openai_server().await?;
    write_config(&home, &workspace, Some(&api_base), 18790)?;

    let home_clone = home.clone();
    let out = tokio::task::spawn_blocking(move || {
        run_femtors(&home_clone, &["agent", "-m", "hello", "-s", "cli:e2e"])
    })
    .await??;
    let _ = shutdown.send(());
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("mock-e2e-response"));
    Ok(())
}

#[tokio::test]
async fn e2e_gateway_health_ready() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path();
    let workspace = home.join("ws");
    let (api_base, shutdown) = start_mock_openai_server().await?;
    let port = free_port() as i32;
    write_config(home, &workspace, Some(&api_base), port)?;

    let mut child = Command::new(femtors_bin())
        .args(["gateway"])
        .env("FEMTORS_HOME", home)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let client = reqwest::Client::new();
    let health_url = format!("http://127.0.0.1:{port}/health");
    let ready_url = format!("http://127.0.0.1:{port}/ready");
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut ok = false;
    while Instant::now() < deadline {
        let h = client.get(&health_url).send().await;
        let r = client.get(&ready_url).send().await;
        if let (Ok(h), Ok(r)) = (h, r)
            && h.status().is_success()
            && r.status().is_success()
        {
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let _ = child.kill();
    let _ = child.wait();
    let _ = shutdown.send(());
    assert!(
        ok,
        "gateway health/ready endpoints did not become ready in time"
    );
    Ok(())
}

#[test]
fn e2e_migrate_dry_run() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path();

    let source = home.join(".picoclaw");
    std::fs::create_dir_all(source.join("workspace/memory"))?;
    std::fs::write(source.join("workspace/memory/MEMORY.md"), "legacy")?;
    std::fs::write(source.join("config.json"), "{}")?;

    let out = run_femtors(home, &["migrate", "--dry-run"])?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Migration summary:"));
    assert!(stdout.contains("Config migrated: true"));
    // dry-run must NOT create the target config
    assert!(!home.join(".femtors/config.json").exists());
    Ok(())
}
