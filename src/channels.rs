//! Communication channel integrations.

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use parking_lot::Mutex;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::task::JoinHandle;

use crate::bus::{InboundMessage, MessageBus, OutboundMessage};
use crate::config::{Config, TelegramConfig};

#[async_trait]
pub trait Channel: Send + Sync {
    async fn start(&self) -> Result<()>;
    async fn stop(&self) -> Result<()>;
    async fn send(&self, msg: &OutboundMessage) -> Result<()>;
}

#[derive(Debug)]
struct BaseChannel {
    allow_list: Vec<String>,
    running: AtomicBool,
}

impl BaseChannel {
    fn new(allow_list: Vec<String>) -> Self {
        Self {
            allow_list,
            running: AtomicBool::new(false),
        }
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::SeqCst);
    }

}

fn split_compound_sender(sender: &str) -> (&str, &str) {
    if let Some(idx) = sender.find('|') {
        (&sender[..idx], &sender[idx + 1..])
    } else {
        (sender, "")
    }
}

fn is_allowed_sender(allow_list: &[String], sender_id: &str) -> bool {
    if allow_list.is_empty() {
        return true;
    }
    let (id_part, user_part) = split_compound_sender(sender_id);
    for allowed in allow_list {
        let trimmed = allowed.trim_start_matches('@');
        let (allowed_id, allowed_user) = split_compound_sender(trimmed);

        if sender_id == allowed
            || sender_id == trimmed
            || id_part == allowed
            || id_part == trimmed
            || id_part == allowed_id
            || (!allowed_user.is_empty() && sender_id == allowed_user)
            || (!user_part.is_empty()
                && (user_part == allowed || user_part == trimmed || user_part == allowed_user))
        {
            return true;
        }
    }
    false
}

struct TelegramChannel {
    base: BaseChannel,
    token: String,
    bus: Arc<MessageBus>,
    client: Client,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl TelegramChannel {
    fn new(cfg: &TelegramConfig, bus: Arc<MessageBus>) -> Result<Self> {
        if cfg.token.trim().is_empty() {
            return Err(anyhow!(
                "telegram token is required when channel is enabled"
            ));
        }
        Ok(Self {
            base: BaseChannel::new(cfg.allow_from.clone()),
            token: cfg.token.clone(),
            bus,
            client: Client::new(),
            task: Mutex::new(None),
        })
    }

    fn api_url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{}", self.token, method)
    }

    async fn send_message_impl(
        client: &Client,
        url: &str,
        chat_id: &str,
        text: &str,
        parse_mode: Option<&str>,
    ) -> Result<()> {
        let mut payload = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
        });
        if let Some(mode) = parse_mode {
            payload["parse_mode"] = serde_json::json!(mode);
        }

        let resp = client.post(url).json(&payload).send().await?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("telegram sendMessage failed: {}", body));
        }
        Ok(())
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    async fn start(&self) -> Result<()> {
        if self.base.is_running() {
            return Ok(());
        }

        self.base.set_running(true);
        let token = self.token.clone();
        let bus = self.bus.clone();
        let client = self.client.clone();
        let allow_list = self.base.allow_list.clone();
        let running = Arc::new(AtomicBool::new(true));
        let running_ref = running.clone();

        let handle = tokio::spawn(async move {
            let mut offset: i64 = 0;
            loop {
                if !running_ref.load(Ordering::SeqCst) {
                    break;
                }

                let url = format!(
                    "https://api.telegram.org/bot{}/getUpdates?timeout=10&offset={}",
                    token, offset
                );
                match client.get(&url).send().await {
                    Ok(resp) => {
                        if let Ok(body) = resp.json::<TelegramGetUpdatesResponse>().await
                            && body.ok
                        {
                            for update in body.result {
                                offset = update.update_id + 1;
                                if let Some(msg) = update.message {
                                    let sender_id = build_sender_id(&msg);
                                    if !is_allowed_sender(&allow_list, &sender_id) {
                                        continue;
                                    }

                                    let content = build_message_content(&msg);
                                    if content.is_empty() {
                                        continue;
                                    }

                                    let chat_id = msg.chat.id.to_string();
                                    let inbound = InboundMessage {
                                        channel: "telegram".to_string(),
                                        sender_id: msg
                                            .from
                                            .as_ref()
                                            .map(|u| u.id.to_string())
                                            .unwrap_or_else(|| "unknown".to_string()),
                                        chat_id: chat_id.clone(),
                                        content,
                                        session_key: format!("telegram:{}", chat_id),
                                        media: None,
                                        metadata: None,
                                    };
                                    if let Err(err) = bus.publish_inbound(inbound).await {
                                        tracing::error!(
                                            "failed to publish inbound telegram msg: {}",
                                            err
                                        );
                                    }
                                }
                            }
                        }
                    }
                    Err(err) => tracing::warn!("telegram polling request failed: {}", err),
                }

                tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;
            }
        });

        *self.task.lock() = Some(handle);
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        self.base.set_running(false);
        if let Some(handle) = self.task.lock().take() {
            handle.abort();
        }
        Ok(())
    }

    async fn send(&self, msg: &OutboundMessage) -> Result<()> {
        let html = markdown_to_telegram_html(&msg.content);
        let send_url = self.api_url("sendMessage");

        if let Err(err) =
            Self::send_message_impl(&self.client, &send_url, &msg.chat_id, &html, Some("HTML"))
                .await
        {
            tracing::warn!("telegram html send failed, fallback to plain text: {}", err);
            Self::send_message_impl(&self.client, &send_url, &msg.chat_id, &msg.content, None)
                .await?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct TelegramGetUpdatesResponse {
    ok: bool,
    result: Vec<TelegramUpdate>,
}

#[derive(Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    message: Option<TelegramMessage>,
}

#[derive(Deserialize)]
struct TelegramMessage {
    chat: TelegramChat,
    from: Option<TelegramUser>,
    text: Option<String>,
    caption: Option<String>,
    voice: Option<serde_json::Value>,
    audio: Option<serde_json::Value>,
    video_note: Option<serde_json::Value>,
    document: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct TelegramChat {
    id: i64,
}

#[derive(Deserialize)]
struct TelegramUser {
    id: i64,
    username: Option<String>,
}

fn build_sender_id(msg: &TelegramMessage) -> String {
    if let Some(from) = &msg.from {
        if let Some(username) = &from.username {
            return format!("{}|{}", from.id, username);
        }
        return from.id.to_string();
    }
    "unknown".to_string()
}

fn build_message_content(msg: &TelegramMessage) -> String {
    let mut content = String::new();
    if let Some(text) = &msg.text {
        content.push_str(text);
    }
    if let Some(caption) = &msg.caption {
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(caption);
    }
    if content.is_empty() {
        if msg.voice.is_some() {
            content = "[voice unsupported in MVP]".to_string();
        } else if msg.audio.is_some() {
            content = "[audio unsupported in MVP]".to_string();
        } else if msg.video_note.is_some() {
            content = "[video note unsupported in MVP]".to_string();
        } else if msg.document.is_some() {
            content = "[document unsupported in MVP]".to_string();
        }
    }
    if content.is_empty() {
        "[empty message]".to_string()
    } else {
        content
    }
}

fn markdown_to_telegram_html(input: &str) -> String {
    // Conservative conversion: escape everything first, then restore simple inline code markers.
    let escaped = input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    escaped.replace("`", "")
}

pub struct ChannelManager {
    channels: HashMap<String, Arc<dyn Channel>>,
    bus: Arc<MessageBus>,
    dispatch_task: Mutex<Option<JoinHandle<()>>>,
}

impl ChannelManager {
    pub fn new(config: &Config, bus: &Arc<MessageBus>) -> Result<Self> {
        let mut channels: HashMap<String, Arc<dyn Channel>> = HashMap::new();

        if config.channels.telegram.enabled {
            let telegram = TelegramChannel::new(&config.channels.telegram, bus.clone())?;
            channels.insert("telegram".to_string(), Arc::new(telegram));
        }

        Ok(Self {
            channels,
            bus: bus.clone(),
            dispatch_task: Mutex::new(None),
        })
    }

    pub fn get_enabled_channels(&self) -> Vec<String> {
        let mut names: Vec<String> = self.channels.keys().cloned().collect();
        names.sort();
        names
    }

    pub async fn start_all(&self) -> Result<()> {
        for channel in self.channels.values() {
            channel.start().await?;
        }

        let mut out_rx = self.bus.take_outbound_receiver()?;
        let channels = self.channels.clone();
        let task = tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                if let Some(channel) = channels.get(&msg.channel) {
                    if let Err(err) = channel.send(&msg).await {
                        tracing::error!(
                            "channel send failed: channel={} error={}",
                            msg.channel,
                            err
                        );
                    }
                } else {
                    tracing::warn!("unknown outbound channel: {}", msg.channel);
                }
            }
        });

        *self.dispatch_task.lock() = Some(task);
        Ok(())
    }

    pub async fn stop_all(&self) -> Result<()> {
        if let Some(task) = self.dispatch_task.lock().take() {
            task.abort();
        }
        for channel in self.channels.values() {
            channel.stop().await?;
        }
        Ok(())
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_channel_allowlist_matching() {
        assert!(is_allowed_sender(&[], "anyone"));
        assert!(is_allowed_sender(&["123456".to_string()], "123456|alice"));
        assert!(is_allowed_sender(&["@alice".to_string()], "123456|alice"));
        assert!(is_allowed_sender(
            &["123456|alice".to_string()],
            "123456"
        ));
        assert!(!is_allowed_sender(
            &["123456".to_string()],
            "654321|bob"
        ));
    }
}
