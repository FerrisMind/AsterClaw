//! Web search and fetch tools.

use std::collections::HashMap;
use std::sync::LazyLock;

use async_trait::async_trait;
use regex::Regex;
use serde_json::Value;
use url::Url;

use crate::config::WebToolsConfig;

use super::{Tool, ToolResult, arg_i64, arg_string};

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
    pub fn from_config(web: &WebToolsConfig) -> Self {
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

        Self {
            provider,
            client: reqwest::Client::new(),
        }
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

        let html = resp
            .text()
            .await
            .map_err(|e| format!("DDG response read failed: {e}"))?;

        // Two-step approach: find ALL <a> tags, then filter for result__a.
        // This avoids depending on attribute ordering (class before/after href).
        static RE_ANCHOR: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"<a\s([^>]*?)href="([^"]+)"([^>]*)>([\s\S]*?)</a>"#)
                .expect("valid regex")
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
            Ok(out) => ToolResult::new(&out).with_for_llm(&out).with_for_user(&out),
            Err(e) => ToolResult::error(&format!("search failed: {}", e)),
        }
    }
}

// ── WebFetchTool ────────────────────────────────────────────────────────

pub struct WebFetchTool {
    max_chars: usize,
    client: reqwest::Client,
}

impl WebFetchTool {
    pub fn new(max_chars: usize) -> Self {
        Self {
            max_chars: max_chars.max(100),
            client: reqwest::Client::new(),
        }
    }
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
        let limit = arg_i64(&args, "max_chars")
            .map(|v| v.max(100) as usize)
            .unwrap_or(self.max_chars);

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

        let raw = match resp.text().await {
            Ok(v) => v,
            Err(e) => return ToolResult::error(&format!("failed to read response: {}", e)),
        };

        let mut text = if content_type.contains("application/json") {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
                serde_json::to_string_pretty(&json).unwrap_or(raw)
            } else {
                raw
            }
        } else if content_type.contains("text/html")
            || raw.starts_with("<!DOCTYPE")
            || raw.to_ascii_lowercase().starts_with("<html")
        {
            html_to_text(&raw)
        } else {
            raw
        };

        let truncated = text.chars().count() > limit;
        if truncated {
            text = text.chars().take(limit).collect();
        }

        let payload = serde_json::json!({
            "url": input_url,
            "status": status,
            "content_type": content_type,
            "truncated": truncated,
            "length": text.chars().count(),
            "text": text,
        });
        let for_user =
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string());
        let for_llm = format!(
            "Fetched URL (status={}, truncated={}, chars={})\n\n{}",
            status, truncated, payload["length"], text
        );

        ToolResult::new(&for_user)
            .with_for_user(&for_user)
            .with_for_llm(&for_llm)
    }
}
