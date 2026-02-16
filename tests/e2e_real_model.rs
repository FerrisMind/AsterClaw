use std::path::{Path, PathBuf};
use std::process::Command;

fn picors_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_picors"))
}

fn write_config(home: &Path, workspace: &Path, api_base: &str, model: &str) -> anyhow::Result<()> {
    let cfg_dir = home.join(".picors");
    std::fs::create_dir_all(&cfg_dir)?;
    std::fs::create_dir_all(workspace)?;

    let cfg = serde_json::json!({
        "agents": {
            "defaults": {
                "provider": "openai",
                "model": model,
                "workspace": workspace.to_string_lossy().to_string(),
                "restrictToWorkspace": true,
                "maxToolIterations": 4,
                "maxTokens": 256,
                "temperature": 0.0
            }
        },
        "providers": {
            "openai": {
                "apiKey": "lm-studio",
                "apiBase": api_base
            }
        },
        "channels": { "telegram": { "enabled": false, "token": "" } },
        "heartbeat": { "enabled": false, "interval": 30 },
        "devices": { "enabled": false, "monitorUsb": false },
        "gateway": { "host": "127.0.0.1", "port": 18790 }
    });
    std::fs::write(
        cfg_dir.join("config.json"),
        serde_json::to_vec_pretty(&cfg)?,
    )?;
    Ok(())
}

#[test]
fn real_llm_smoke_agent_cli() -> anyhow::Result<()> {
    if std::env::var("PICORS_REAL_LLM").ok().as_deref() != Some("1") {
        return Ok(());
    }

    let api_base = std::env::var("REAL_LLM_API_BASE")
        .unwrap_or_else(|_| "http://127.0.0.1:1234/v1".to_string());
    let model = std::env::var("REAL_LLM_MODEL").unwrap_or_else(|_| "gguf".to_string());

    let tmp = tempfile::tempdir()?;
    let home = tmp.path();
    let workspace = home.join("workspace");
    write_config(home, &workspace, &api_base, &model)?;

    let out = Command::new(picors_bin())
        .args([
            "agent",
            "-m",
            "Return EXACT token: PICORS_REAL_LLM_SMOKE_OK",
            "-s",
            "cli:real-llm-smoke",
        ])
        .env("PICORS_HOME", home)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .output()?;

    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("PICORS_REAL_LLM_SMOKE_OK"),
        "real llm smoke token not found in output:\n{}",
        stdout
    );
    Ok(())
}
