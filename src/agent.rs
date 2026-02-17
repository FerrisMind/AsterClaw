//! Core AI agent loop.

use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::bus::{InboundMessage, MessageBus, OutboundMessage};
use crate::channels::ChannelManager;
use crate::config::Config;
use crate::constants;
use crate::context_builder::ContextBuilder;
use crate::providers::{Message, ProcessOptions, Provider};
use crate::session::SessionManager;
use crate::state::Manager as StateManager;
use crate::tools::{SubagentManager, ToolRegistry, ToolResult};

pub struct AgentLoop {
    bus: Arc<MessageBus>,
    provider: Arc<dyn Provider>,
    model: String,
    context_window: i32,
    max_iterations: i32,
    sessions: Arc<Mutex<SessionManager>>,
    state: Arc<Mutex<StateManager>>,
    tools: Arc<Mutex<ToolRegistry>>,
    context_builder: ContextBuilder,
    running: AtomicBool,
    channel_manager: Arc<RwLock<Option<Arc<ChannelManager>>>>,
    tool_output_max_chars: usize,
}

impl AgentLoop {
    pub fn new(config: &Config, msg_bus: &Arc<MessageBus>, provider: Arc<dyn Provider>) -> Self {
        let workspace = config.workspace_path();
        let sessions = Arc::new(Mutex::new(SessionManager::new(workspace.join("sessions"))));
        let state = Arc::new(Mutex::new(StateManager::new(workspace.clone())));
        let tool_registry = ToolRegistry::with_tool_config(
            workspace.clone(),
            config.agents.defaults.restrict_to_workspace,
            config.tools.web.clone(),
            config.tools.exec.clone(),
        );
        let tool_output_max_chars = config.tools.tool_output_max_chars;
        let subagent_manager = Arc::new(SubagentManager::new(
            provider.clone(),
            config.agents.defaults.model.clone(),
            msg_bus.clone(),
            tool_registry.clone(),
            config.agents.defaults.max_tool_iterations,
            tool_output_max_chars,
        ));
        tool_registry.set_subagent_manager(subagent_manager);
        let tools = Arc::new(Mutex::new(tool_registry));
        let context_builder = ContextBuilder::new(workspace.clone());

        Self {
            bus: msg_bus.clone(),
            provider,
            model: config.agents.defaults.model.clone(),
            context_window: config.agents.defaults.max_tokens,
            max_iterations: config.agents.defaults.max_tool_iterations,
            sessions,
            state,
            tools,
            context_builder,
            running: AtomicBool::new(false),
            channel_manager: Arc::new(RwLock::new(None)),
            tool_output_max_chars,
        }
    }

    pub fn set_channel_manager(&self, manager: Arc<ChannelManager>) {
        *self.channel_manager.write() = Some(manager);
    }

    pub fn cron_service(&self) -> Arc<Mutex<crate::cron::CronService>> {
        self.tools.lock().cron_service()
    }

    pub async fn process_direct(&self, content: &str, session_key: &str) -> anyhow::Result<String> {
        let msg = InboundMessage {
            channel: "cli".to_string(),
            sender_id: "cli".to_string(),
            chat_id: "direct".to_string(),
            content: content.to_string(),
            session_key: session_key.to_string(),
            media: None,
            metadata: None,
        };
        self.process_message(msg).await
    }

    pub async fn process_message(&self, msg: InboundMessage) -> anyhow::Result<String> {
        tracing::info!("Processing message from {}:{}", msg.channel, msg.sender_id);

        if msg.channel == "system" {
            return self.process_system_message(msg).await;
        }

        if msg.content.starts_with('/') {
            let response = self.handle_command(&msg).await?;
            if !response.is_empty()
                && let Err(err) = self
                    .bus
                    .publish_outbound(OutboundMessage {
                        channel: msg.channel.clone(),
                        chat_id: msg.chat_id.clone(),
                        content: response.clone(),
                    })
                    .await
            {
                tracing::error!("failed to publish command response: {}", err);
            }
            return Ok(response);
        }

        let opts = ProcessOptions {
            session_key: msg.session_key.clone(),
            channel: msg.channel.clone(),
            chat_id: msg.chat_id.clone(),
            user_message: msg.content.clone(),
            default_response: "I've completed processing but have no response to give.".to_string(),
            enable_summary: true,
            no_history: false,
        };

        let response = self.run_agent_loop(opts).await?;

        if !response.is_empty()
            && let Err(err) = self
                .bus
                .publish_outbound(OutboundMessage {
                    channel: msg.channel,
                    chat_id: msg.chat_id,
                    content: response.clone(),
                })
                .await
        {
            tracing::error!("failed to publish outbound response: {}", err);
        }

        Ok(response)
    }

    async fn process_system_message(&self, msg: InboundMessage) -> anyhow::Result<String> {
        // heartbeat/system path can carry origin as "channel:chat_id"
        let (origin_channel, origin_chat) = if let Some(idx) = msg.chat_id.find(':') {
            (
                msg.chat_id[..idx].to_string(),
                msg.chat_id[idx + 1..].to_string(),
            )
        } else {
            ("cli".to_string(), "direct".to_string())
        };

        if constants::is_internal_channel(&origin_channel) {
            tracing::info!("System message on internal channel: {}", origin_channel);
            return Ok(String::new());
        }

        let prompt = if msg.content.trim().is_empty() {
            "HEARTBEAT_OK".to_string()
        } else {
            msg.content.clone()
        };

        let response = self
            .run_agent_loop(ProcessOptions {
                session_key: format!("system:{}:{}", origin_channel, origin_chat),
                channel: origin_channel.clone(),
                chat_id: origin_chat.clone(),
                user_message: prompt,
                default_response: "HEARTBEAT_OK".to_string(),
                enable_summary: false,
                no_history: true,
            })
            .await?;

        if !response.is_empty() && response != "HEARTBEAT_OK" {
            let _ = self
                .bus
                .publish_outbound(OutboundMessage {
                    channel: origin_channel,
                    chat_id: origin_chat,
                    content: response.clone(),
                })
                .await;
        }

        Ok(response)
    }

    async fn run_agent_loop(&self, opts: ProcessOptions) -> anyhow::Result<String> {
        if !opts.channel.is_empty()
            && !opts.chat_id.is_empty()
            && !constants::is_internal_channel(&opts.channel)
        {
            let channel_key = format!("{}:{}", opts.channel, opts.chat_id);
            self.state.lock().set_last_channel(&channel_key);
        }

        self.update_tool_contexts(&opts.channel, &opts.chat_id);

        let history = if !opts.no_history {
            self.sessions.lock().get_history(&opts.session_key)
        } else {
            vec![]
        };

        let summary = if !opts.no_history {
            self.sessions.lock().get_summary(&opts.session_key)
        } else {
            String::new()
        };

        let mut messages = self.build_messages(
            history,
            summary,
            &opts.user_message,
            None,
            &opts.channel,
            &opts.chat_id,
        );

        if !opts.no_history {
            self.sessions
                .lock()
                .add_message(&opts.session_key, "user", &opts.user_message);
        }

        let (final_content, iteration, sent_message_tool) =
            self.run_llm_iteration(&mut messages, &opts).await?;

        let final_content = if final_content.is_empty() && !sent_message_tool {
            opts.default_response.clone()
        } else {
            final_content
        };

        if !opts.no_history {
            self.sessions
                .lock()
                .add_message(&opts.session_key, "assistant", &final_content);
            let _ = self.sessions.lock().save(&opts.session_key);
        }

        if opts.enable_summary {
            self.maybe_summarize(&opts.session_key, &opts.channel, &opts.chat_id);
        }

        tracing::info!(
            "Response: {} (iterations: {})",
            final_content.chars().take(120).collect::<String>(),
            iteration
        );

        Ok(final_content)
    }

    fn build_messages(
        &self,
        history: Vec<Message>,
        summary: String,
        current_message: &str,
        _media: Option<&[String]>,
        channel: &str,
        chat_id: &str,
    ) -> Vec<Message> {
        let tool_summaries = self.tools.lock().get_summaries();
        self.context_builder.build_messages(
            history,
            summary,
            current_message,
            channel,
            chat_id,
            &tool_summaries,
        )
    }

    async fn run_llm_iteration(
        &self,
        messages: &mut Vec<Message>,
        opts: &ProcessOptions,
    ) -> anyhow::Result<(String, i32, bool)> {
        let mut iteration = 0;
        let mut final_content = String::new();
        let mut sent_message_tool = false;

        while iteration < self.max_iterations {
            iteration += 1;
            tracing::debug!("LLM iteration {}/{}", iteration, self.max_iterations);

            let tool_defs = self.tools.lock().to_provider_defs();

            let mut options = HashMap::new();
            options.insert("max_tokens".to_string(), serde_json::json!(8192));
            options.insert("temperature".to_string(), serde_json::json!(0.7));

            let response = self
                .provider
                .chat_with_options(messages, Some(&tool_defs), &self.model, options)
                .await?;

            if response.tool_calls.is_empty() {
                final_content = response.content;
                if sent_message_tool {
                    final_content.clear();
                }
                tracing::info!("LLM response without tool calls (direct answer)");
                break;
            }

            let tool_names: Vec<String> = response
                .tool_calls
                .iter()
                .filter_map(|tc| tc.name.clone())
                .collect();
            tracing::info!("LLM requested tool calls: {:?}", tool_names);

            messages.push(Message {
                role: "assistant".to_string(),
                content: response.content.clone(),
                tool_calls: response.tool_calls.clone(),
                tool_call_id: None,
            });

            if !opts.no_history
                && let Some(last) = messages.last()
            {
                self.sessions
                    .lock()
                    .add_full_message(&opts.session_key, last.clone());
            }

            for tc in &response.tool_calls {
                let tool_name = tc.name.clone().unwrap_or_default();
                let tool_args = tc.arguments.clone().unwrap_or_default();
                tracing::info!("Executing tool: {}", tool_name);

                let tool = { self.tools.lock().get(&tool_name) };
                let result: ToolResult = if let Some(tool) = tool {
                    tool.execute(tool_args, &opts.channel, &opts.chat_id).await
                } else {
                    ToolResult::error(&format!("Tool not found: {}", tool_name))
                };

                if tool_name == "message" && result.error.is_none() && result.silent {
                    sent_message_tool = true;
                }

                if let Some(for_user) = result.for_user.as_ref()
                    && !for_user.is_empty()
                {
                    let _ = self
                        .bus
                        .publish_outbound(OutboundMessage {
                            channel: opts.channel.clone(),
                            chat_id: opts.chat_id.clone(),
                            content: for_user.clone(),
                        })
                        .await;
                }

                let content_for_llm = if let Some(err) = result.error.as_ref() {
                    err.clone()
                } else {
                    result.for_llm.clone().unwrap_or_default()
                };
                let content_for_llm = self.truncate_tool_message(content_for_llm);

                messages.push(Message::tool(&content_for_llm, &tc.id));

                if !opts.no_history
                    && let Some(last) = messages.last()
                {
                    self.sessions
                        .lock()
                        .add_full_message(&opts.session_key, last.clone());
                }
            }
        }

        Ok((final_content, iteration, sent_message_tool))
    }

    fn update_tool_contexts(&self, _channel: &str, _chat_id: &str) {}

    fn maybe_summarize(&self, session_key: &str, _channel: &str, _chat_id: &str) {
        let history = self.sessions.lock().get_history(session_key);
        let token_estimate = self.estimate_tokens(&history);
        let threshold = self.context_window * 75 / 100;
        if history.len() > 20 || token_estimate > threshold {
            tracing::debug!("Session {} exceeds threshold, would summarize", session_key);
        }
    }

    fn estimate_tokens(&self, messages: &[Message]) -> i32 {
        let total_chars: usize = messages.iter().map(|m| m.content.chars().count()).sum();
        (total_chars as f32 * 0.4) as i32
    }

    fn truncate_tool_message(&self, content: String) -> String {
        if self.tool_output_max_chars == 0 {
            return content;
        }
        let total = content.chars().count();
        if total <= self.tool_output_max_chars {
            return content;
        }
        let kept: String = content.chars().take(self.tool_output_max_chars).collect();
        let omitted = total - self.tool_output_max_chars;
        format!("{kept}\n\n[tool output truncated: omitted {omitted} chars]")
    }

    async fn handle_command(&self, msg: &InboundMessage) -> anyhow::Result<String> {
        let content = msg.content.trim();
        if !content.starts_with('/') {
            return Ok(String::new());
        }

        let parts: Vec<&str> = content.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(String::new());
        }

        let cmd = parts[0];
        let args = &parts[1..];
        match cmd {
            "/help" | "/start" => {
                Ok("Available commands: /help, /model, /status, /show, /list, /switch".to_string())
            }
            "/model" => Ok(format!("Current model: {}", self.model)),
            "/status" => Ok("Agent is running".to_string()),
            "/show" => self.handle_show_command(args).await,
            "/list" => self.handle_list_command(args).await,
            "/switch" => self.handle_switch_command(args).await,
            _ => Ok(format!("Unknown command: {}", cmd)),
        }
    }

    async fn handle_show_command(&self, args: &[&str]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok("Usage: /show [model|channel]".to_string());
        }
        match args[0] {
            "model" => Ok(format!("Current model: {}", self.model)),
            "channel" => Ok("Current channel: cli".to_string()),
            _ => Ok(format!("Unknown show target: {}", args[0])),
        }
    }

    async fn handle_list_command(&self, args: &[&str]) -> anyhow::Result<String> {
        if args.is_empty() {
            return Ok("Usage: /list [models|channels]".to_string());
        }

        match args[0] {
            "models" => Ok(
                "Available models: glm-4.7, claude-3-5-sonnet, gpt-4o (configured in config)"
                    .to_string(),
            ),
            "channels" => {
                let manager = self.channel_manager.read();
                if let Some(cm) = manager.as_ref() {
                    let channels = cm.get_enabled_channels();
                    if channels.is_empty() {
                        return Ok("No channels enabled".to_string());
                    }
                    Ok(format!("Enabled channels: {}", channels.join(", ")))
                } else {
                    Ok("Channel manager not initialized".to_string())
                }
            }
            _ => Ok(format!("Unknown list target: {}", args[0])),
        }
    }

    async fn handle_switch_command(&self, args: &[&str]) -> anyhow::Result<String> {
        if args.len() < 3 || args[1] != "to" {
            return Ok("Usage: /switch [model|channel] to <name>".to_string());
        }
        match args[0] {
            "model" => Ok(format!("Switch requested: {} -> {}", self.model, args[2])),
            "channel" => Ok(format!("Switched target channel to {}", args[2])),
            _ => Ok(format!("Unknown switch target: {}", args[0])),
        }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        self.running.store(true, Ordering::SeqCst);
        let mut rx = self.bus.take_inbound_receiver()?;

        while let Some(msg) = rx.recv().await {
            if !self.running.load(Ordering::SeqCst) {
                break;
            }
            if let Err(err) = self.process_message(msg).await {
                tracing::error!("Error processing message: {}", err);
            }
        }

        tracing::info!("Agent loop stopped");
        Ok(())
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn get_startup_info(&self) -> serde_json::Value {
        let tools = self.tools.lock();
        let skills = self.context_builder.get_skills_info();
        serde_json::json!({
            "tools": {
                "count": tools.len(),
                "names": tools.list_names()
            },
            "skills": skills
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct NoopProvider;

    #[async_trait::async_trait]
    impl Provider for NoopProvider {
        async fn chat_with_options(
            &self,
            _messages: &mut Vec<Message>,
            _tools: Option<&[crate::providers::ToolDefinition]>,
            _model: &str,
            _options: HashMap<String, serde_json::Value>,
        ) -> anyhow::Result<crate::providers::LlmResponse> {
            Ok(crate::providers::LlmResponse {
                content: "ok".to_string(),
                tool_calls: Vec::new(),
                finish_reason: Some("stop".to_string()),
                usage: None,
            })
        }
    }

    fn test_agent() -> AgentLoop {
        let tmp = TempDir::new().expect("tempdir");
        let mut cfg = Config::default();
        cfg.agents.defaults.workspace = tmp.path().to_string_lossy().to_string();
        let bus = Arc::new(MessageBus::new());
        AgentLoop::new(&cfg, &bus, Arc::new(NoopProvider))
    }

    #[tokio::test]
    async fn start_command_returns_help() {
        let agent = test_agent();
        let response = agent
            .process_message(InboundMessage {
                channel: "telegram".to_string(),
                sender_id: "u1".to_string(),
                chat_id: "c1".to_string(),
                content: "/start".to_string(),
                media: None,
                session_key: "telegram:c1".to_string(),
                metadata: None,
            })
            .await
            .expect("command should work");
        assert!(response.contains("Available commands"));
    }

    #[tokio::test]
    async fn list_channels_without_manager_is_explicit() {
        let agent = test_agent();
        let response = agent
            .process_message(InboundMessage {
                channel: "telegram".to_string(),
                sender_id: "u1".to_string(),
                chat_id: "c1".to_string(),
                content: "/list channels".to_string(),
                media: None,
                session_key: "telegram:c1".to_string(),
                metadata: None,
            })
            .await
            .expect("command should work");
        assert_eq!(response, "Channel manager not initialized");
    }
}
