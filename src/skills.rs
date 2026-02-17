//! Skills loader/installer support.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SkillInfo {
    pub name: String,
    pub path: PathBuf,
    pub source: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct SkillsLoader {
    workspace_skills: PathBuf,
    global_skills: PathBuf,
    builtin_skills: PathBuf,
}

impl SkillsLoader {
    pub fn new(workspace: &Path) -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::new_with_paths(
            workspace.join("skills"),
            home.join(".femtors").join("skills"),
            cwd.join("skills"),
        )
    }

    pub fn new_with_paths(
        workspace_skills: PathBuf,
        global_skills: PathBuf,
        builtin_skills: PathBuf,
    ) -> Self {
        Self {
            workspace_skills,
            global_skills,
            builtin_skills,
        }
    }

    pub fn list_skills(&self) -> Vec<SkillInfo> {
        let mut out = Vec::new();
        self.collect_skills(&self.workspace_skills, "workspace", &mut out);
        self.collect_skills(&self.global_skills, "global", &mut out);
        self.collect_skills(&self.builtin_skills, "builtin", &mut out);
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    fn collect_skills(&self, dir: &Path, source: &str, out: &mut Vec<SkillInfo>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(v) => v,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }
            let skill_name = entry.file_name().to_string_lossy().to_string();
            if out.iter().any(|s| s.name == skill_name) {
                continue;
            }
            let skill_md = entry.path().join("SKILL.md");
            if !skill_md.exists() {
                continue;
            }
            let raw = std::fs::read_to_string(&skill_md).unwrap_or_default();
            let (name, description) = parse_frontmatter(&raw)
                .map(|fm| {
                    (
                        fm.name.unwrap_or(skill_name.clone()),
                        fm.description.unwrap_or_default(),
                    )
                })
                .unwrap_or((skill_name.clone(), String::new()));
            out.push(SkillInfo {
                name,
                path: skill_md,
                source: source.to_string(),
                description,
            });
        }
    }

    pub fn build_skills_summary_xml(&self) -> String {
        let skills = self.list_skills();
        if skills.is_empty() {
            return String::new();
        }
        let mut out = String::from("<skills>\n");
        for s in skills {
            out.push_str("  <skill>\n");
            out.push_str(&format!("    <name>{}</name>\n", escape_xml(&s.name)));
            out.push_str(&format!(
                "    <description>{}</description>\n",
                escape_xml(&s.description)
            ));
            out.push_str(&format!(
                "    <location>{}</location>\n",
                escape_xml(&s.path.display().to_string())
            ));
            out.push_str(&format!("    <source>{}</source>\n", escape_xml(&s.source)));
            out.push_str("  </skill>\n");
        }
        out.push_str("</skills>");
        out
    }

    pub fn load_skill(&self, name: &str) -> Option<String> {
        for root in [
            &self.workspace_skills,
            &self.global_skills,
            &self.builtin_skills,
        ] {
            let p = root.join(name).join("SKILL.md");
            if p.exists() {
                let raw = std::fs::read_to_string(p).ok()?;
                return Some(strip_frontmatter(&raw));
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct SkillInstaller {
    workspace: PathBuf,
}

impl SkillInstaller {
    pub fn new(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
        }
    }

    pub async fn install_from_github(&self, repo: &str) -> anyhow::Result<String> {
        let repo_name = repo
            .rsplit('/')
            .next()
            .ok_or_else(|| anyhow::anyhow!("invalid github repo path"))?;
        let target = self.workspace.join("skills").join(repo_name);
        if target.exists() {
            return Err(anyhow::anyhow!("skill '{repo_name}' already exists"));
        }
        let url = format!("https://raw.githubusercontent.com/{repo}/main/SKILL.md");
        let body = reqwest::get(&url).await?.text().await?;
        std::fs::create_dir_all(&target)?;
        std::fs::write(target.join("SKILL.md"), body)?;
        Ok(repo_name.to_string())
    }

    pub fn uninstall(&self, skill_name: &str) -> anyhow::Result<()> {
        let skill_dir = self.workspace.join("skills").join(skill_name);
        if !skill_dir.exists() {
            return Err(anyhow::anyhow!("skill '{skill_name}' not found"));
        }
        std::fs::remove_dir_all(skill_dir)?;
        Ok(())
    }

    pub async fn list_available_skills(&self) -> anyhow::Result<Vec<AvailableSkill>> {
        let url = "https://raw.githubusercontent.com/sipeed/femtors-skills/main/skills.json";
        let body = reqwest::get(url).await?.text().await?;
        let parsed: Vec<AvailableSkill> = serde_json::from_str(&body)?;
        Ok(parsed)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AvailableSkill {
    pub name: String,
    pub repository: String,
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct FrontMatter {
    name: Option<String>,
    description: Option<String>,
}

fn parse_frontmatter(raw: &str) -> Option<FrontMatter> {
    let mut lines = raw.lines();
    if lines.next()? != "---" {
        return None;
    }
    let mut block = String::new();
    for l in lines {
        if l.trim() == "---" {
            break;
        }
        block.push_str(l);
        block.push('\n');
    }

    let name = block
        .lines()
        .find_map(|l| l.strip_prefix("name:"))
        .map(|v| v.trim().trim_matches('"').trim_matches('\'').to_string());
    let description = block
        .lines()
        .find_map(|l| l.strip_prefix("description:"))
        .map(|v| v.trim().trim_matches('"').trim_matches('\'').to_string());
    Some(FrontMatter { name, description })
}

fn strip_frontmatter(raw: &str) -> String {
    if !raw.starts_with("---\n") {
        return raw.to_string();
    }
    let mut parts = raw.splitn(3, "---\n");
    let _ = parts.next();
    let _ = parts.next();
    parts.next().unwrap_or(raw).to_string()
}

fn escape_xml(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::SkillsLoader;

    #[test]
    fn skills_precedence_workspace_over_global_over_builtin() {
        let root = tempfile::tempdir().expect("tmp");
        let ws = root.path().join("ws");
        let global = root.path().join("global");
        let builtin = root.path().join("builtin");

        std::fs::create_dir_all(ws.join("demo")).expect("mkdir ws");
        std::fs::create_dir_all(global.join("demo")).expect("mkdir global");
        std::fs::create_dir_all(builtin.join("demo")).expect("mkdir builtin");

        std::fs::write(ws.join("demo/SKILL.md"), "workspace").expect("write ws");
        std::fs::write(global.join("demo/SKILL.md"), "global").expect("write global");
        std::fs::write(builtin.join("demo/SKILL.md"), "builtin").expect("write builtin");

        let loader = SkillsLoader::new_with_paths(ws, global, builtin);
        let skill = loader.load_skill("demo").expect("skill");
        assert_eq!(skill.trim(), "workspace");
    }
}
