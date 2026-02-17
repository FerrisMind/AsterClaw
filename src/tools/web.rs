//! Web search and fetch tools.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::LazyLock;

use async_trait::async_trait;
use regex::Regex;
use serde_json::Value;
use url::Url;

use crate::config::WebToolsConfig;

use super::{Tool, ToolResult, arg_i64, arg_string};

const DDG_HTML_MAX_BYTES: usize = 512 * 1024;

// ── WebSearchTool ───────────────────────────────────────────────────────

/// Web search provider strategy.
#[derive(Debug, Clone)]
enum SearchProvider {
    /// Brave Search API (requires API key).
    Brave { api_key: String, max_results: usize },
    /// DuckDuckGo HTML scraping (no key required).
    DuckDuckGo { max_results: usize },
}

pub struct WebSearchTool {
    provider: SearchProvider,
    client: reqwest::Client,
}

impl WebSearchTool {
    /// Build from config — priority: Brave > DuckDuckGo > disabled.
    pub fn from_config(web: &WebToolsConfig, client: reqwest::Client) -> Self {
        let brave_key = web
            .brave
            .api_key
            .clone()
            .or_else(|| std::env::var("BRAVE_API_KEY").ok())
            .filter(|k| !k.trim().is_empty());

        let provider = if let Some(key) = brave_key.filter(|_| web.brave.enabled) {
            SearchProvider::Brave {
                api_key: key,
                max_results: (web.brave.max_results as usize).clamp(1, 10),
            }
        } else if web.duckduckgo.enabled {
            SearchProvider::DuckDuckGo {
                max_results: (web.duckduckgo.max_results as usize).clamp(1, 10),
            }
        } else {
            // Default fallback: DDG always works without a key
            SearchProvider::DuckDuckGo { max_results: 5 }
        };

        Self { provider, client }
    }

    /// Brave Search API call.
    async fn search_brave(
        &self,
        query: &str,
        api_key: &str,
        count: usize,
    ) -> Result<String, String> {
        let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        let url = format!(
            "https://api.search.brave.com/res/v1/web/search?q={}&count={}",
            encoded, count
        );
        let resp = self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .header("X-Subscription-Token", api_key)
            .send()
            .await
            .map_err(|e| format!("Brave request failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("Brave API error: {}", resp.status()));
        }

        let body: Value = resp
            .json()
            .await
            .map_err(|e| format!("Brave JSON parse failed: {e}"))?;

        let results = body.pointer("/web/results").and_then(|v| v.as_array());

        let mut lines = vec![format!("Results for: {}", query)];
        if let Some(items) = results {
            for (i, item) in items.iter().take(count).enumerate() {
                let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("");
                let desc = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                lines.push(format!("{}. {}\n   {}", i + 1, title, url));
                if !desc.is_empty() {
                    lines.push(format!("   {}", desc));
                }
            }
        }
        if lines.len() == 1 {
            lines.push("No results".to_string());
        }
        Ok(lines.join("\n"))
    }

    /// DuckDuckGo HTML scraping (no API key needed).
    async fn search_ddg(&self, query: &str, count: usize) -> Result<String, String> {
        let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        let url = format!("https://html.duckduckgo.com/html/?q={}", encoded);
        let resp = self.client
            .get(&url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .send()
            .await
            .map_err(|e| format!("DDG request failed: {e}"))?;

        let (html, ddg_truncated) = read_response_limited(resp, DDG_HTML_MAX_BYTES).await?;

        // Two-step approach: find ALL <a> tags, then filter for result__a.
        // This avoids depending on attribute ordering (class before/after href).
        static RE_ANCHOR: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"<a\s([^>]*?)href="([^"]+)"([^>]*)>([\s\S]*?)</a>"#).expect("valid regex")
        });
        static RE_SNIPPET: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"<a\s+class="result__snippet[^"]*"[^>]*>([\s\S]*?)</a>"#)
                .expect("valid regex")
        });
        static RE_STRIP: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"<[^>]+>").expect("valid regex"));

        // Collect only anchors whose opening tag contains result__a
        let mut result_links: Vec<(&str, &str)> = Vec::new(); // (href, inner_html)
        for caps in RE_ANCHOR.captures_iter(&html) {
            let before_href = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let after_href = caps.get(3).map(|m| m.as_str()).unwrap_or("");
            if before_href.contains("result__a") || after_href.contains("result__a") {
                let href = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                let inner = caps.get(4).map(|m| m.as_str()).unwrap_or("");
                result_links.push((href, inner));
            }
        }
        let snippet_caps: Vec<_> = RE_SNIPPET.captures_iter(&html).collect();

        let mut lines = vec![format!("Results for: {} (via DuckDuckGo)", query)];
        let mut seen = 0usize;

        for (i, (raw_url, raw_title)) in result_links.iter().enumerate() {
            if seen >= count {
                break;
            }
            let title = RE_STRIP.replace_all(raw_title, "").trim().to_string();
            if title.is_empty() {
                continue;
            }

            // Decode HTML entities before parsing URL params
            let decoded_url = raw_url
                .replace("&amp;", "&")
                .replace("&#x27;", "'")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"");

            // Decode DDG redirect URLs (uddg= parameter)
            let mut final_url = if decoded_url.contains("uddg=") {
                url::form_urlencoded::parse(decoded_url.as_bytes())
                    .find(|(k, _)| k == "uddg")
                    .map(|(_, v)| v.to_string())
                    .unwrap_or_else(|| decoded_url.clone())
            } else {
                decoded_url
            };

            // Handle protocol-relative URLs (//example.com → https://example.com)
            if final_url.starts_with("//") {
                final_url = format!("https:{}", final_url);
            }

            lines.push(format!("{}. {}\n   {}", seen + 1, title, final_url));

            // Attach snippet if available
            if let Some(snippet_cap) = snippet_caps.get(i) {
                let snippet = RE_STRIP
                    .replace_all(snippet_cap.get(1).map(|m| m.as_str()).unwrap_or(""), "")
                    .trim()
                    .to_string();
                if !snippet.is_empty() {
                    lines.push(format!("   {}", snippet));
                }
            }
            seen += 1;
        }

        if seen == 0 {
            lines.push("No results".to_string());
        }
        if ddg_truncated {
            lines.push("(results parsed from truncated DDG page body)".to_string());
        }
        Ok(lines.join("\n"))
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }
    fn description(&self) -> &str {
        "Search the web for current information. Returns titles, URLs, and snippets from search results."
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query" },
                "count": { "type": "integer", "description": "Number of results (1-10)", "minimum": 1, "maximum": 10 }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>, _: &str, _: &str) -> ToolResult {
        let query = match arg_string(&args, "query") {
            Some(v) if !v.is_empty() => v,
            _ => return ToolResult::error("query is required"),
        };

        let result = match &self.provider {
            SearchProvider::Brave {
                api_key,
                max_results,
            } => {
                let count = arg_i64(&args, "count")
                    .map(|v| v.clamp(1, 10) as usize)
                    .unwrap_or(*max_results);
                self.search_brave(&query, api_key, count).await
            }
            SearchProvider::DuckDuckGo { max_results } => {
                let count = arg_i64(&args, "count")
                    .map(|v| v.clamp(1, 10) as usize)
                    .unwrap_or(*max_results);
                self.search_ddg(&query, count).await
            }
        };

        match result {
            Ok(out) => ToolResult::new(&out),
            Err(e) => ToolResult::error(&format!("search failed: {}", e)),
        }
    }
}

// ── WebFetchTool ────────────────────────────────────────────────────────

pub struct WebFetchTool {
    default_max_chars: usize,
    hard_max_chars: usize,
    hard_max_bytes: usize,
    client: reqwest::Client,
}

impl WebFetchTool {
    #[allow(dead_code)] // test helper constructor
    pub fn new(max_chars: usize, client: reqwest::Client) -> Self {
        Self::with_limits(max_chars, 200_000, 1_000_000, client)
    }

    pub fn with_limits(
        default_max_chars: usize,
        hard_max_chars: usize,
        hard_max_bytes: usize,
        client: reqwest::Client,
    ) -> Self {
        Self {
            default_max_chars: default_max_chars.max(100),
            hard_max_chars: hard_max_chars.max(100),
            hard_max_bytes: hard_max_bytes.max(4096),
            client,
        }
    }
}

async fn read_response_limited(
    mut resp: reqwest::Response,
    max_bytes: usize,
) -> Result<(String, bool), String> {
    let mut bytes = Vec::with_capacity(max_bytes.min(16 * 1024));
    let mut truncated = false;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("failed to read response: {}", e))?
    {
        let remaining = max_bytes.saturating_sub(bytes.len());
        if remaining == 0 {
            truncated = true;
            break;
        }
        let take = remaining.min(chunk.len());
        bytes.extend_from_slice(&chunk[..take]);
        if take < chunk.len() {
            truncated = true;
            break;
        }
    }
    Ok((String::from_utf8_lossy(&bytes).to_string(), truncated))
}

fn html_to_text(input: &str) -> String {
    static RE_SCRIPT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<script[\s\S]*?</script>").expect("valid regex"));
    static RE_STYLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<style[\s\S]*?</style>").expect("valid regex"));
    static RE_TAGS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<[^>]+>").expect("valid regex"));
    static RE_WS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("valid regex"));
    let re_script = &*RE_SCRIPT;
    let re_style = &*RE_STYLE;
    let re_tags = &*RE_TAGS;
    let re_ws = &*RE_WS;

    let s = re_script.replace_all(input, "");
    let s = re_style.replace_all(&s, "");
    let s = re_tags.replace_all(&s, " ");
    re_ws.replace_all(&s, " ").trim().to_string()
}

fn is_private_or_local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let oct = v4.octets();
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                // Carrier-grade NAT: 100.64.0.0/10
                || (oct[0] == 100 && (64..=127).contains(&oct[1]))
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
        }
    }
}

async fn is_blocked_web_target(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return true;
    };

    let host_lc = host.to_ascii_lowercase();
    if host_lc == "localhost"
        || host_lc.ends_with(".localhost")
        || host_lc == "metadata.google.internal"
    {
        return true;
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_private_or_local_ip(ip);
    }

    let port = url.port_or_known_default().unwrap_or(80);
    if let Ok(addrs) = tokio::net::lookup_host((host, port)).await {
        for addr in addrs {
            if is_private_or_local_ip(addr.ip()) {
                return true;
            }
        }
    }

    false
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }
    fn description(&self) -> &str {
        "Fetch URL and extract readable text"
    }
    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" },
                "max_chars": { "type": "integer", "minimum": 100 }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, args: HashMap<String, Value>, _: &str, _: &str) -> ToolResult {
        let input_url = match arg_string(&args, "url") {
            Some(v) if !v.is_empty() => v,
            _ => return ToolResult::error("url is required"),
        };
        let parsed = match Url::parse(&input_url) {
            Ok(v) => v,
            Err(e) => return ToolResult::error(&format!("invalid URL: {}", e)),
        };
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return ToolResult::error("only http/https URLs are allowed");
        }
        if is_blocked_web_target(&parsed).await {
            return ToolResult::error("target URL is blocked by SSRF guard");
        }
        let limit = arg_i64(&args, "max_chars")
            .map(|v| v.max(100) as usize)
            .unwrap_or(self.default_max_chars)
            .min(self.hard_max_chars);

        let resp = match self
            .client
            .get(parsed)
            .header("User-Agent", "Mozilla/5.0")
            .send()
            .await
        {
            Ok(v) => v,
            Err(e) => return ToolResult::error(&format!("request failed: {}", e)),
        };

        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let raw_byte_limit = (limit.saturating_mul(4))
            .saturating_add(8192)
            .min(self.hard_max_bytes)
            .max(4096);
        let (raw, raw_truncated) = match read_response_limited(resp, raw_byte_limit).await {
            Ok(v) => v,
            Err(e) => return ToolResult::error(&e),
        };

        let raw_prefix = raw
            .chars()
            .take(1024)
            .collect::<String>()
            .to_ascii_lowercase();
        let mut text = if content_type.contains("application/json") {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
                serde_json::to_string_pretty(&json).unwrap_or(raw)
            } else {
                raw
            }
        } else if content_type.contains("text/html")
            || raw.starts_with("<!DOCTYPE")
            || raw_prefix.starts_with("<html")
        {
            html_to_text(&raw)
        } else {
            raw
        };

        let mut truncated = raw_truncated;
        if text.chars().count() > limit {
            text = text.chars().take(limit).collect();
            truncated = true;
        }

        let text_len = text.chars().count();
        let for_llm = format!(
            "Fetched URL (status={}, truncated={}, chars={})\n\n{}",
            status, truncated, text_len, text
        );

        tracing::debug!(
            "web_fetch: status={}, content_type={}, limit_chars={}, raw_byte_limit={}, raw_truncated={}, final_chars={}",
            status,
            content_type,
            limit,
            raw_byte_limit,
            raw_truncated,
            text_len
        );

        ToolResult::new(&for_llm)
    }
}
