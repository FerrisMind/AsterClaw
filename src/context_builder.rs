use crate::memory::MemoryStore;
use crate::providers::Message;
use crate::skills::SkillsLoader;
use once_cell::sync::OnceCell;
use std::path::PathBuf;
pub struct ContextBuilder {
    workspace: PathBuf,
    skills_loader: SkillsLoader,
    memory: MemoryStore,
    cached_bootstrap: OnceCell<String>,
    cached_skills_summary: OnceCell<String>,
}
impl ContextBuilder {
    pub fn new(workspace: PathBuf) -> Self {
        let skills_loader = SkillsLoader::new(&workspace);
        let memory = MemoryStore::new(workspace.clone());
        Self {
            workspace,
            skills_loader,
            memory,
            cached_bootstrap: OnceCell::new(),
            cached_skills_summary: OnceCell::new(),
        }
    }
    pub fn get_skills_info(&self) -> serde_json::Value {
        let skills = self.skills_loader.list_skills();
        serde_json::json!({
            "total": skills.len(),
            "available": skills.len(),
            "names": skills.into_iter().map(|s| s.name).collect::<Vec<_>>()
        })
    }
    pub fn build_messages(
        &self,
        mut history: Vec<Message>,
        summary: String,
        current_message: &str,
        channel: &str,
        tool_summaries: &[String],
    ) -> Vec<Message> {
        let mut messages = Vec::new();
        let mut system_prompt = self.build_system_prompt(channel, tool_summaries);
        if !summary.is_empty() {
            system_prompt.push_str("\n\n## Summary of Previous Conversation\n\n");
            system_prompt.push_str(&summary);
        }
        let orphan_count = history.iter().take_while(|m| m.role == "tool").count();
        if orphan_count > 0 {
            history.drain(..orphan_count);
        }
        messages.push(Message::system(&system_prompt));
        messages.extend(history);
        messages.push(Message::user(current_message));
        messages
    }
    pub fn build_system_prompt(
        &self,
        channel: &str,
        tool_summaries: &[String],
    ) -> String {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M (%A)");
        let runtime = format!(
            "{} {}, Rust {}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            env!("CARGO_PKG_VERSION")
        );
        let channel_instructions = match channel {
            "telegram" => concat!(
                "\n\n## Channel\nYou are responding in **Telegram**.\n\n",
                "### Formatting Rules for Telegram\n",
                "Your response will be auto-converted to Telegram-safe HTML. You can use standard Markdown:\n",
                "- **bold**, *bold*, _italic_, ~~strikethrough~~\n",
                "- `inline code` and ```code blocks```\n",
                "- [links](url) work\n",
                "- > blockquotes work\n",
                "- Bullet lists (- item) work\n\n",
                "Style guidance:\n",
                "- Keep messages concise — Telegram is a chat, not a document\n",
                "- Avoid long walls of text; prefer short paragraphs\n",
                "- Use line breaks between sections for readability\n",
                "- # headings will be rendered as bold text\n",
            ).to_string(),
            "cli" => "\n\n## Channel\nYou are responding in the CLI terminal. Use standard Markdown formatting.".to_string(),
            _ => format!("\n\n## Channel\nYou are responding via the '{}' channel.", channel),
        };
        let mut prompt = format!(
            concat!(
                "# AsterClaw\n\nYou are AsterClaw, a helpful AI assistant.{}\n\n",
                "## Language\n",
                "**Always respond in the same language as the user's message** unless they explicitly ask for a different language.\n",
                "- If user writes in English → respond in English\n",
                "- If user writes in Russian → respond in Russian\n",
                "- If user writes in any other language → match that language\n\n",
                "## Citation Rules\n",
                "When you use web_search or web_fetch tools, you MUST:\n",
                "1. Synthesize the information into a clear, natural answer\n",
                "2. At the end of your answer, add a \"Sources\" section with hyperlinks: [Title](url)\n",
                "3. Never dump raw search snippets or fetched HTML to the user\n",
                "4. Keep sources compact — just the relevant ones you actually used\n\n",
                "## Current Time\n{}\n\n## Runtime\n{}\n\n",
                "## Workspace\nYour workspace is at: {}\n",
                "- Memory: {}/memory/MEMORY.md\n",
                "- Daily Notes: {}/memory/YYYYMM/YYYYMMDD.md\n",
                "- Skills: {}/skills/{{skill-name}}/SKILL.md",
            ),
            channel_instructions,
            now,
            runtime,
            self.workspace.display(),
            self.workspace.display(),
            self.workspace.display(),
            self.workspace.display(),
        );
        let tools_section = self.build_tools_section(tool_summaries);
        if !tools_section.is_empty() {
            prompt.push_str("\n\n---\n\n");
            prompt.push_str(&tools_section);
        }
        let bootstrap = self
            .cached_bootstrap
            .get_or_init(|| self.load_bootstrap_files());
        if !bootstrap.is_empty() {
            prompt.push_str("\n\n---\n\n");
            prompt.push_str(bootstrap);
        }
        let skills_summary = self
            .cached_skills_summary
            .get_or_init(|| self.skills_loader.build_skills_summary_xml());
        if !skills_summary.is_empty() {
            prompt.push_str("\n\n---\n\n");
            prompt.push_str(&format!(
                "# Skills\n\nThe following skills extend your capabilities. To use one, read SKILL.md via read_file.\n\n{}",
                skills_summary
            ));
        }
        let memory_context = self.memory.get_memory_context();
        if !memory_context.is_empty() {
            prompt.push_str("\n\n---\n\n");
            prompt.push_str(&memory_context);
        }
        prompt.push_str("\n\n---\n\n");
        prompt.push_str("## Important Rules\n\n1. **ALWAYS use tools** - When you need to perform an action, you MUST call the appropriate tool.\n2. **Be helpful and accurate** - Briefly explain tool actions.\n3. **Memory** - Write persistent facts to memory/MEMORY.md");
        if !channel.is_empty() {
            prompt.push_str("\n\n---\n\n");
            prompt.push_str(&format!(
                "## Current Session\nChannel: {}",
                channel
            ));
        }
        prompt
    }
    fn build_tools_section(&self, tool_summaries: &[String]) -> String {
        if tool_summaries.is_empty() {
            return String::new();
        }
        let mut out = String::from("## Available Tools\n\n");
        out.push_str("**CRITICAL**: You MUST use tools to perform actions. Do NOT pretend to execute commands or schedule tasks.\n\n");
        for line in tool_summaries {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(concat!(
            "\n### Web Research Workflow\n\n",
            "When you need current or external information, follow this pattern:\n",
            "1. **Search first** — call `web_search` with a focused query to get titles, URLs, and snippets.\n",
            "2. **Evaluate snippets** — if the snippets answer the question, respond directly.\n",
            "3. **Fetch selectively** — if you need more detail, call `web_fetch` on the 1-2 most relevant URLs.\n",
            "4. **Synthesize** — combine the fetched content into your answer, citing sources.\n\n",
            "Do NOT skip straight to `web_fetch` without first searching — you need URLs to fetch.\n",
            "Do NOT fetch all results — only the most relevant ones to stay within context limits.\n",
        ));
        out
    }
    fn load_bootstrap_files(&self) -> String {
        let files = ["AGENTS.md", "SOUL.md", "USER.md", "IDENTITY.md"];
        let mut out = String::new();
        for name in files {
            let path = self.workspace.join(name);
            let content = std::fs::read_to_string(path).unwrap_or_default();
            if content.is_empty() {
                continue;
            }
            out.push_str(&format!("## {}\n\n{}\n\n", name, content));
        }
        out.trim().to_string()
    }
}
#[cfg(test)]
mod tests {
    use super::ContextBuilder;
    #[test]
    fn system_prompt_contains_skills_memory_and_tools() {
        let tmp = tempfile::tempdir().expect("tmp");
        let ws = tmp.path().to_path_buf();
        std::fs::create_dir_all(ws.join("skills/demo")).expect("mkdir");
        std::fs::write(
            ws.join("skills/demo/SKILL.md"),
            "---\nname: demo\ndescription: sample\n---\ncontent",
        )
        .expect("write skill");
        std::fs::create_dir_all(ws.join("memory")).expect("mkdir");
        std::fs::write(ws.join("memory/MEMORY.md"), "remember").expect("write memory");
        std::fs::write(ws.join("AGENTS.md"), "agents cfg").expect("write bootstrap");
        let cb = ContextBuilder::new(ws.clone());
        let prompt = cb.build_system_prompt("telegram", "123", &["- tool a".to_string()]);
        assert!(prompt.contains("<skills>"));
        assert!(prompt.contains("Long-term Memory"));
        assert!(prompt.contains("Available Tools"));
        assert!(prompt.contains("AGENTS.md"));
        assert!(prompt.contains("Web Research Workflow"));
        assert!(prompt.contains("Telegram"));
        assert!(prompt.contains("Formatting Rules"));
    }
}
