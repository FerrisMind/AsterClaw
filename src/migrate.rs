//! Migration utilities from legacy `.picoclaw` layout to `.asterclaw`.

use anyhow::{Result, anyhow};
use std::path::{Path, PathBuf};

use crate::config::{Config, ProviderConfig};

#[derive(Debug, Clone, Default)]
pub struct MigrateResult {
    pub files_copied: usize,
    pub files_skipped: usize,
    pub backups_created: usize,
    pub config_migrated: bool,
    pub warnings: Vec<String>,
}

pub fn migrate_from_openclaw(
    dry_run: bool,
    config_only: bool,
    workspace_only: bool,
    force: bool,
    openclaw_home: Option<&str>,
    asterclaw_home: Option<&str>,
) -> Result<MigrateResult> {
    if config_only && workspace_only {
        return Err(anyhow!(
            "--config-only and --workspace-only cannot be used together"
        ));
    }

    let source_home = resolve_home(openclaw_home, ".picoclaw")?;
    let target_home = resolve_home(asterclaw_home, ".asterclaw")?;
    let mut result = MigrateResult::default();

    if !workspace_only {
        migrate_config(&source_home, &target_home, dry_run, &mut result)?;
    }
    if !config_only {
        migrate_workspace(&source_home, &target_home, dry_run, force, &mut result)?;
    }

    Ok(result)
}

fn resolve_home(override_path: Option<&str>, default_suffix: &str) -> Result<PathBuf> {
    if let Some(path) = override_path {
        return Ok(expand_home(path));
    }
    // Respect ASTERCLAW_HOME override (consistent with config::resolve_home_dir).
    if let Ok(ph) = std::env::var("ASTERCLAW_HOME") {
        let trimmed = ph.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed).join(default_suffix));
        }
    }
    let home = dirs::home_dir().ok_or_else(|| anyhow!("Cannot resolve user home directory"))?;
    Ok(home.join(default_suffix))
}

fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(path)
}

fn migrate_config(
    source_home: &Path,
    target_home: &Path,
    dry_run: bool,
    result: &mut MigrateResult,
) -> Result<()> {
    let source_candidates = [
        source_home.join("config.json"),
        source_home.join("openclaw.json"),
    ];
    let source_path = source_candidates
        .iter()
        .find(|p| p.exists())
        .cloned()
        .ok_or_else(|| anyhow!("legacy config not found in {}", source_home.display()))?;

    let source_cfg = load_legacy_config(&source_path)?;
    let target_path = target_home.join("config.json");
    let mut final_cfg = source_cfg;

    if target_path.exists() {
        let existing = crate::config::load_config(&target_path)?;
        final_cfg = merge_config(existing, final_cfg);
    }

    if !dry_run {
        std::fs::create_dir_all(target_home)?;
        crate::config::save_config(&target_path, &final_cfg)?;
    }
    result.config_migrated = true;
    Ok(())
}

fn load_legacy_config(path: &Path) -> Result<Config> {
    let raw = std::fs::read_to_string(path)?;
    let value: serde_json::Value = serde_json::from_str(&raw)?;
    let normalized = normalize_keys(value);
    let mut cfg: Config = serde_json::from_value(normalized)?;
    rewrite_paths_for_asterclaw(&mut cfg);
    Ok(cfg)
}

fn normalize_keys(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let normalized = map
                .into_iter()
                .map(|(k, v)| (camel_to_snake(&k), normalize_keys(v)))
                .collect();
            serde_json::Value::Object(normalized)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(normalize_keys).collect())
        }
        other => other,
    }
}

fn camel_to_snake(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                let prev = chars[i - 1];
                let next = chars.get(i + 1).copied().unwrap_or_default();
                if prev.is_ascii_lowercase()
                    || prev.is_ascii_digit()
                    || (prev.is_ascii_uppercase() && next.is_ascii_lowercase())
                {
                    out.push('_');
                }
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn rewrite_paths_for_asterclaw(cfg: &mut Config) {
    cfg.agents.defaults.workspace = cfg
        .agents
        .defaults
        .workspace
        .replace(".picoclaw", ".asterclaw");
}

fn merge_provider(dst: ProviderConfig, src: ProviderConfig) -> ProviderConfig {
    ProviderConfig {
        api_key: if is_empty_opt(&dst.api_key) {
            src.api_key
        } else {
            dst.api_key
        },
        api_base: if is_empty_opt(&dst.api_base) {
            src.api_base
        } else {
            dst.api_base
        },
        proxy: if is_empty_opt(&dst.proxy) {
            src.proxy
        } else {
            dst.proxy
        },
        auth_method: if is_empty_opt(&dst.auth_method) {
            src.auth_method
        } else {
            dst.auth_method
        },
        connect_mode: if is_empty_opt(&dst.connect_mode) {
            src.connect_mode
        } else {
            dst.connect_mode
        },
    }
}

fn is_empty_opt(v: &Option<String>) -> bool {
    v.as_ref().map(|s| s.trim().is_empty()).unwrap_or(true)
}

fn merge_config(mut existing: Config, incoming: Config) -> Config {
    if existing.agents.defaults.provider.trim().is_empty() {
        existing.agents.defaults.provider = incoming.agents.defaults.provider;
    }
    if existing.agents.defaults.model.trim().is_empty() {
        existing.agents.defaults.model = incoming.agents.defaults.model;
    }
    if existing.agents.defaults.workspace.trim().is_empty() {
        existing.agents.defaults.workspace = incoming.agents.defaults.workspace;
    }

    existing.providers.anthropic =
        merge_provider(existing.providers.anthropic, incoming.providers.anthropic);
    existing.providers.openai =
        merge_provider(existing.providers.openai, incoming.providers.openai);
    existing.providers.openrouter =
        merge_provider(existing.providers.openrouter, incoming.providers.openrouter);
    existing.providers.groq = merge_provider(existing.providers.groq, incoming.providers.groq);
    existing.providers.zhipu = merge_provider(existing.providers.zhipu, incoming.providers.zhipu);
    existing.providers.deepseek =
        merge_provider(existing.providers.deepseek, incoming.providers.deepseek);

    if !existing.channels.telegram.enabled && incoming.channels.telegram.enabled {
        existing.channels.telegram = incoming.channels.telegram;
    }

    existing
}

fn migrate_workspace(
    source_home: &Path,
    target_home: &Path,
    dry_run: bool,
    force: bool,
    result: &mut MigrateResult,
) -> Result<()> {
    let source_workspace = source_home.join("workspace");
    if !source_workspace.exists() {
        result.warnings.push(format!(
            "source workspace not found: {}",
            source_workspace.display()
        ));
        return Ok(());
    }

    let target_workspace = target_home.join("workspace");
    let migrate_dirs = ["memory", "skills"];
    let migrate_files = [
        "AGENTS.md",
        "USER.md",
        "SOUL.md",
        "HEARTBEAT.md",
        "TOOLS.md",
    ];

    for dir in migrate_dirs {
        copy_tree(
            &source_workspace.join(dir),
            &target_workspace.join(dir),
            dry_run,
            force,
            result,
        )?;
    }
    for file in migrate_files {
        copy_file_with_backup(
            &source_workspace.join(file),
            &target_workspace.join(file),
            dry_run,
            force,
            result,
        )?;
    }
    Ok(())
}

fn copy_tree(
    source_root: &Path,
    target_root: &Path,
    dry_run: bool,
    force: bool,
    result: &mut MigrateResult,
) -> Result<()> {
    if !source_root.exists() {
        return Ok(());
    }
    for entry in walkdir::WalkDir::new(source_root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let rel = path
            .strip_prefix(source_root)
            .map_err(|e| anyhow!("failed to compute relative path: {}", e))?;
        let destination = target_root.join(rel);
        if entry.file_type().is_dir() {
            if !dry_run {
                std::fs::create_dir_all(&destination)?;
            }
            continue;
        }
        copy_file_with_backup(path, &destination, dry_run, force, result)?;
    }
    Ok(())
}

fn copy_file_with_backup(
    source: &Path,
    destination: &Path,
    dry_run: bool,
    force: bool,
    result: &mut MigrateResult,
) -> Result<()> {
    if !source.exists() {
        return Ok(());
    }

    if destination.exists() {
        if !force {
            result.files_skipped += 1;
            result.warnings.push(format!(
                "skip existing file without --force: {}",
                destination.display()
            ));
            return Ok(());
        }
        if !dry_run {
            let backup = destination.with_extension(format!(
                "{}.bak",
                destination
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("backup")
            ));
            std::fs::copy(destination, &backup)?;
        }
        result.backups_created += 1;
    }

    if !dry_run {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(source, destination)?;
    }
    result.files_copied += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_case_normalization_works() {
        let raw = serde_json::json!({
            "agents": {
                "defaults": {
                    "maxToolIterations": 7
                }
            }
        });
        let normalized = normalize_keys(raw);
        assert_eq!(normalized["agents"]["defaults"]["max_tool_iterations"], 7);
    }

    #[test]
    fn merge_keeps_existing_non_empty_fields() {
        let mut existing = Config::default();
        existing.providers.openai.api_key = Some("existing".to_string());

        let mut incoming = Config::default();
        incoming.providers.openai.api_key = Some("incoming".to_string());
        incoming.providers.deepseek.api_key = Some("new-deepseek".to_string());

        let merged = merge_config(existing, incoming);
        assert_eq!(merged.providers.openai.api_key.as_deref(), Some("existing"));
        assert_eq!(
            merged.providers.deepseek.api_key.as_deref(),
            Some("new-deepseek")
        );
    }

    #[test]
    fn dry_run_does_not_create_target_files() {
        let src_home = tempfile::tempdir().expect("src temp");
        let dst_home = tempfile::tempdir().expect("dst temp");

        std::fs::create_dir_all(src_home.path().join("workspace/memory")).expect("mkdir");
        std::fs::write(
            src_home.path().join("workspace/memory/MEMORY.md"),
            "legacy memory",
        )
        .expect("write");
        std::fs::write(src_home.path().join("config.json"), "{}").expect("cfg");

        let res = migrate_from_openclaw(
            true,
            false,
            false,
            true,
            Some(src_home.path().to_str().expect("src path")),
            Some(dst_home.path().to_str().expect("dst path")),
        )
        .expect("migrate");

        assert!(res.config_migrated);
        assert!(!dst_home.path().join("workspace/memory/MEMORY.md").exists());
        assert!(!dst_home.path().join("config.json").exists());
    }
}
