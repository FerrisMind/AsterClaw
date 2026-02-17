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
use crate::config::Config;
use crate::voice::GroqTranscriber;

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
    transcriber: Option<GroqTranscriber>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl TelegramChannel {
    fn new(cfg: &Config, bus: Arc<MessageBus>) -> Result<Self> {
        let telegram = &cfg.channels.telegram;
        if telegram.token.trim().is_empty() {
            return Err(anyhow!(
                "telegram token is required when channel is enabled"
            ));
        }
        let allow_list: Vec<String> = telegram
            .allow_from
            .iter()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect();
        if allow_list.is_empty() {
            return Err(anyhow!(
                "telegram allow_from is required for private mode (add your user id/username)"
            ));
        }
        Ok(Self {
            base: BaseChannel::new(allow_list),
            token: telegram.token.clone(),
            bus,
            client: Client::new(),
            transcriber: resolve_transcriber(cfg),
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
        let transcriber = self.transcriber.clone();
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

                                    let (content, media) = build_message_content_with_media(
                                        &client,
                                        &token,
                                        &msg,
                                        transcriber.as_ref(),
                                    )
                                    .await;
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
                                        media,
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
    voice: Option<TelegramVoice>,
    audio: Option<TelegramAudio>,
    video_note: Option<serde_json::Value>,
    document: Option<TelegramDocument>,
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

#[derive(Deserialize)]
struct TelegramVoice {
    file_id: String,
}

#[derive(Deserialize)]
struct TelegramAudio {
    file_id: String,
}

#[derive(Deserialize)]
struct TelegramDocument {
    file_id: String,
}

#[derive(Deserialize)]
struct TelegramGetFileResponse {
    ok: bool,
    result: Option<TelegramFile>,
}

#[derive(Deserialize)]
struct TelegramFile {
    file_path: Option<String>,
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

async fn build_message_content_with_media(
    client: &Client,
    token: &str,
    msg: &TelegramMessage,
    transcriber: Option<&GroqTranscriber>,
) -> (String, Option<Vec<String>>) {
    let mut content = String::new();
    let mut media_paths = Vec::new();

    if let Some(text) = &msg.text {
        content.push_str(text);
    }
    if let Some(caption) = &msg.caption {
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(caption);
    }

    if let Some(voice) = &msg.voice
        && let Some(path) = download_telegram_file(client, token, &voice.file_id, ".ogg").await
    {
        media_paths.push(path.clone());
        if !content.is_empty() {
            content.push('\n');
        }
        let voice_text = if let Some(tr) = transcriber {
            if tr.is_available() {
                match tr.transcribe(std::path::Path::new(&path)).await {
                    Ok(r) if !r.text.trim().is_empty() => {
                        format!("[voice transcription: {}]", r.text)
                    }
                    Ok(_) => "[voice]".to_string(),
                    Err(_) => "[voice (transcription failed)]".to_string(),
                }
            } else {
                "[voice]".to_string()
            }
        } else {
            "[voice]".to_string()
        };
        content.push_str(&voice_text);
    }

    if let Some(audio) = &msg.audio
        && let Some(path) = download_telegram_file(client, token, &audio.file_id, ".mp3").await
    {
        media_paths.push(path);
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str("[audio]");
    }

    if let Some(doc) = &msg.document
        && let Some(path) = download_telegram_file(client, token, &doc.file_id, "").await
    {
        media_paths.push(path);
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str("[file]");
    }

    if content.is_empty() {
        if msg.video_note.is_some() {
            content = "[video note unsupported in MVP]".to_string();
        } else if !media_paths.is_empty() {
            content = "[media]".to_string();
        } else {
            content = "[empty message]".to_string();
        }
    }

    let media = if media_paths.is_empty() {
        None
    } else {
        Some(media_paths)
    };
    (content, media)
}

async fn download_telegram_file(
    client: &Client,
    token: &str,
    file_id: &str,
    ext: &str,
) -> Option<String> {
    let get_file_url = format!(
        "https://api.telegram.org/bot{}/getFile?file_id={}",
        token, file_id
    );
    let resp = client.get(&get_file_url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let parsed = resp.json::<TelegramGetFileResponse>().await.ok()?;
    if !parsed.ok {
        return None;
    }
    let file_path = parsed.result.and_then(|r| r.file_path)?;
    let url = format!("https://api.telegram.org/file/bot{}/{}", token, file_path);
    let bytes = client.get(&url).send().await.ok()?.bytes().await.ok()?;
    let mut name = file_path.replace('/', "_");
    if !ext.is_empty() && !name.ends_with(ext) {
        name.push_str(ext);
    }
    let local_path = std::env::temp_dir().join(name);
    std::fs::write(&local_path, bytes).ok()?;
    Some(local_path.to_string_lossy().to_string())
}

fn resolve_transcriber(cfg: &Config) -> Option<GroqTranscriber> {
    let key = cfg
        .providers
        .groq
        .api_key
        .clone()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            std::env::var("GROQ_API_KEY")
                .ok()
                .filter(|v| !v.trim().is_empty())
        });
    key.map(GroqTranscriber::new)
}

fn markdown_to_telegram_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    let mut in_code_block = false;
    let mut code_block_buf = String::new();

    for line in input.lines() {
        // Handle fenced code blocks
        if line.trim_start().starts_with("```") {
            if in_code_block {
                // closing
                let escaped = html_escape(&code_block_buf);
                out.push_str("<pre>");
                out.push_str(&escaped);
                out.push_str("</pre>\n");
                code_block_buf.clear();
                in_code_block = false;
            } else {
                in_code_block = true;
                code_block_buf.clear();
            }
            continue;
        }
        if in_code_block {
            if !code_block_buf.is_empty() {
                code_block_buf.push('\n');
            }
            code_block_buf.push_str(line);
            continue;
        }

        // Headers → bold text
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed
            .strip_prefix("### ")
            .or_else(|| trimmed.strip_prefix("## "))
            .or_else(|| trimmed.strip_prefix("# "))
        {
            out.push_str("<b>");
            out.push_str(&convert_inline(&html_escape(rest)));
            out.push_str("</b>\n");
            continue;
        }

        // Blockquotes
        if let Some(rest) = trimmed.strip_prefix("> ") {
            out.push_str("▎ <i>");
            out.push_str(&convert_inline(&html_escape(rest)));
            out.push_str("</i>\n");
            continue;
        }

        // Bullet lists: - or * → •
        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            out.push_str("• ");
            out.push_str(&convert_inline(&html_escape(rest)));
            out.push('\n');
            continue;
        }

        // Regular line
        out.push_str(&convert_inline(&html_escape(line)));
        out.push('\n');
    }

    // Unclosed code block
    if in_code_block && !code_block_buf.is_empty() {
        out.push_str("<pre>");
        out.push_str(&html_escape(&code_block_buf));
        out.push_str("</pre>\n");
    }

    // Trim trailing whitespace
    out.trim_end().to_string()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Convert inline markdown patterns to Telegram HTML.
/// Operates on already-HTML-escaped text.
fn convert_inline(s: &str) -> String {
    // Links first: [text](url) → <a href="url">text</a>
    let s = convert_links(s);
    let s = convert_pattern(&s, "**", "<b>", "</b>");
    let s = convert_pattern(&s, "__", "<u>", "</u>");
    let s = convert_pattern(&s, "~~", "<s>", "</s>");
    let s = convert_pattern(&s, "*", "<b>", "</b>");
    let s = convert_pattern(&s, "_", "<i>", "</i>");
    convert_inline_code(&s)
}

/// Convert markdown links [text](url) to HTML <a> tags.
fn convert_links(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(bracket_start) = rest.find('[') {
        result.push_str(&rest[..bracket_start]);
        let after_bracket = &rest[bracket_start + 1..];
        if let Some(bracket_end) = after_bracket.find("](") {
            let text = &after_bracket[..bracket_end];
            let after_paren = &after_bracket[bracket_end + 2..];
            if let Some(paren_end) = after_paren.find(')') {
                let url = &after_paren[..paren_end];
                result.push_str(&format!("<a href=\"{}\">{}</a>", url, text));
                rest = &after_paren[paren_end + 1..];
                continue;
            }
        }
        result.push('[');
        rest = after_bracket;
    }
    result.push_str(rest);
    result
}

/// Replace paired `marker` with open/close HTML tags.
fn convert_pattern(input: &str, marker: &str, open: &str, close: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut rest = input;
    let mut opened = false;

    while let Some(pos) = rest.find(marker) {
        result.push_str(&rest[..pos]);
        if opened {
            result.push_str(close);
        } else {
            result.push_str(open);
        }
        opened = !opened;
        rest = &rest[pos + marker.len()..];
    }
    result.push_str(rest);
    // If unclosed, treat marker as literal
    if opened {
        return input.to_string();
    }
    result
}

/// Convert `inline code` to <code> tags.
fn convert_inline_code(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut rest = input;
    loop {
        match rest.find('`') {
            Some(start) => {
                result.push_str(&rest[..start]);
                let after = &rest[start + 1..];
                match after.find('`') {
                    Some(end) => {
                        result.push_str("<code>");
                        result.push_str(&after[..end]);
                        result.push_str("</code>");
                        rest = &after[end + 1..];
                    }
                    None => {
                        result.push_str(rest);
                        return result;
                    }
                }
            }
            None => {
                result.push_str(rest);
                return result;
            }
        }
    }
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
            let telegram = TelegramChannel::new(config, bus.clone())?;
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
    use crate::{bus::MessageBus, config::Config};
    use std::sync::Arc;

    #[test]
    fn base_channel_allowlist_matching() {
        assert!(is_allowed_sender(&[], "anyone"));
        assert!(is_allowed_sender(&["123456".to_string()], "123456|alice"));
        assert!(is_allowed_sender(&["@alice".to_string()], "123456|alice"));
        assert!(is_allowed_sender(&["123456|alice".to_string()], "123456"));
        assert!(!is_allowed_sender(&["123456".to_string()], "654321|bob"));
    }

    #[test]
    fn telegram_channel_requires_non_empty_allowlist() {
        let mut cfg = Config::default();
        cfg.channels.telegram.enabled = true;
        cfg.channels.telegram.token = "test-token".to_string();
        cfg.channels.telegram.allow_from.clear();

        let bus = Arc::new(MessageBus::new());
        let err = TelegramChannel::new(&cfg, bus)
            .err()
            .expect("expected error");
        assert!(err.to_string().contains("allow_from"));
    }

    #[test]
    fn md_to_tg_bold_italic() {
        assert_eq!(
            markdown_to_telegram_html("**hello** and _world_"),
            "<b>hello</b> and <i>world</i>"
        );
    }

    #[test]
    fn md_to_tg_inline_code() {
        assert_eq!(
            markdown_to_telegram_html("use `cargo build` here"),
            "use <code>cargo build</code> here"
        );
    }

    #[test]
    fn md_to_tg_code_block() {
        let input = "before\n```\nfn main() {}\n```\nafter";
        let html = markdown_to_telegram_html(input);
        assert!(html.contains("<pre>fn main() {}</pre>"));
        assert!(html.contains("before"));
        assert!(html.contains("after"));
    }

    #[test]
    fn md_to_tg_header_becomes_bold() {
        assert_eq!(markdown_to_telegram_html("# Title"), "<b>Title</b>");
        assert_eq!(markdown_to_telegram_html("## Subtitle"), "<b>Subtitle</b>");
    }

    #[test]
    fn md_to_tg_bullet_list() {
        assert_eq!(
            markdown_to_telegram_html("- first\n- second"),
            "• first\n• second"
        );
    }

    #[test]
    fn md_to_tg_blockquote() {
        let html = markdown_to_telegram_html("> quoted text");
        assert!(html.contains("▎"));
        assert!(html.contains("quoted text"));
    }

    #[test]
    fn md_to_tg_escapes_html() {
        assert_eq!(
            markdown_to_telegram_html("a < b & c > d"),
            "a &lt; b &amp; c &gt; d"
        );
    }

    #[test]
    fn md_to_tg_hyperlinks() {
        assert_eq!(
            markdown_to_telegram_html("See [Google](https://google.com) for more"),
            "See <a href=\"https://google.com\">Google</a> for more"
        );
    }
}
