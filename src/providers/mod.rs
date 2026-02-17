//! LLM provider implementations.

pub mod types;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

pub use types::*;

#[async_trait]
pub trait Provider: Send + Sync {
    async fn chat_with_options(
        &self,
        messages: &mut Vec<Message>,
        tools: Option<&[ToolDefinition]>,
        model: &str,
        options: HashMap<String, serde_json::Value>,
    ) -> Result<LlmResponse>;
}

#[derive(Debug, Clone, Copy)]
enum ProviderKind {
    OpenAi,
    OpenRouter,
    Groq,
    Zhipu,
    DeepSeek,
}

struct HttpProvider {
    api_key: String,
    base_url: String,
    extra_headers: HashMap<String, String>,
    _kind: ProviderKind,
    client: reqwest::Client,
}

impl HttpProvider {
    fn new(
        api_key: String,
        base_url: String,
        extra_headers: HashMap<String, String>,
        kind: ProviderKind,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(10))
            .pool_max_idle_per_host(4)
            .build()
            .unwrap_or_default();
        Self {
            api_key,
            base_url,
            extra_headers,
            _kind: kind,
            client,
        }
    }

    async fn make_request(
        &self,
        model: &str,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        options: &HashMap<String, serde_json::Value>,
    ) -> Result<LlmResponse> {
        let messages_json: Vec<serde_json::Value> = messages
            .iter()
            .map(normalize_message_for_provider)
            .collect();

        let mut body = serde_json::json!({
            "model": model,
            "messages": messages_json,
        });

        if let Some(tool_defs) = tools
            && !tool_defs.is_empty()
        {
            let tools_json: Vec<serde_json::Value> = tool_defs
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.function.name(),
                            "description": t.function.description(),
                            "parameters": t.function.parameters(),
                        }
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(tools_json);
            body["tool_choice"] = serde_json::json!("auto");
        }

        if let Some(temp) = options.get("temperature") {
            body["temperature"] = temp.clone();
        }
        if let Some(max_tokens) = options.get("max_tokens") {
            body["max_tokens"] = max_tokens.clone();
        }

        let mut req = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json");

        for (k, v) in &self.extra_headers {
            req = req.header(k, v);
        }

        let resp = req.json(&body).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "provider API request failed (status={}): {}",
                status,
                body
            ));
        }

        let result: serde_json::Value = resp.json().await?;
        parse_openai_compatible_response(&result)
    }
}

fn normalize_message_for_provider(message: &Message) -> serde_json::Value {
    let mut out = serde_json::json!({
        "role": message.role,
        "content": message.content,
    });

    if let Some(tool_call_id) = message.tool_call_id.as_ref()
        && !tool_call_id.trim().is_empty()
    {
        out["tool_call_id"] = serde_json::json!(tool_call_id);
    }

    if !message.tool_calls.is_empty() {
        let mut calls = Vec::with_capacity(message.tool_calls.len());
        for (idx, tc) in message.tool_calls.iter().enumerate() {
            let id = if tc.id.trim().is_empty() {
                format!("call_{}", idx + 1)
            } else {
                tc.id.clone()
            };
            let tool_type = if tc.tool_type.trim().is_empty() {
                "function".to_string()
            } else {
                tc.tool_type.clone()
            };
            let name = tc
                .function
                .as_ref()
                .map(|f| f.name.clone())
                .or_else(|| tc.name.clone())
                .unwrap_or_default();
            let arguments = tc
                .function
                .as_ref()
                .map(|f| f.arguments.clone())
                .or_else(|| {
                    tc.arguments
                        .as_ref()
                        .map(|m| serde_json::to_string(m).unwrap_or_else(|_| "{}".to_string()))
                })
                .unwrap_or_else(|| "{}".to_string());
            calls.push(serde_json::json!({
                "id": id,
                "type": tool_type,
                "function": {
                    "name": name,
                    "arguments": arguments,
                }
            }));
        }
        out["tool_calls"] = serde_json::Value::Array(calls);
    }

    out
}

#[async_trait]
impl Provider for HttpProvider {
    async fn chat_with_options(
        &self,
        messages: &mut Vec<Message>,
        tools: Option<&[ToolDefinition]>,
        model: &str,
        options: HashMap<String, serde_json::Value>,
    ) -> Result<LlmResponse> {
        self.make_request(model, messages, tools, &options).await
    }
}

fn parse_openai_compatible_response(result: &serde_json::Value) -> Result<LlmResponse> {
    let choices = result
        .get("choices")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("provider response missing 'choices'"))?;

    if choices.is_empty() {
        return Ok(LlmResponse {
            content: String::new(),
            tool_calls: vec![],
            finish_reason: Some("stop".to_string()),
            usage: None,
        });
    }

    let message = &choices[0]["message"];
    let content = message["content"].as_str().unwrap_or("").to_string();

    let mut tool_calls = Vec::new();
    if let Some(tc) = message["tool_calls"].as_array() {
        for (idx, t) in tc.iter().enumerate() {
            let id = t["id"]
                .as_str()
                .map(str::to_string)
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| format!("call_{}", idx + 1));
            let name = t["function"]["name"].as_str().unwrap_or("").to_string();
            let args_str = t["function"]["arguments"].as_str().unwrap_or("{}");
            let args: HashMap<String, serde_json::Value> =
                serde_json::from_str(args_str).unwrap_or_default();
            let tool_type = t["type"]
                .as_str()
                .map(str::to_string)
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "function".to_string());

            tool_calls.push(ToolCall {
                id,
                tool_type,
                function: Some(FunctionCall {
                    name: name.clone(),
                    arguments: args_str.to_string(),
                }),
                name: Some(name),
                arguments: Some(args),
            });
        }
    }

    let usage = result.get("usage").and_then(|u| {
        u.as_object().map(|_| UsageInfo {
            prompt_tokens: u["prompt_tokens"].as_i64().unwrap_or(0) as i32,
            completion_tokens: u["completion_tokens"].as_i64().unwrap_or(0) as i32,
            total_tokens: u["total_tokens"].as_i64().unwrap_or(0) as i32,
        })
    });

    Ok(LlmResponse {
        content,
        tool_calls,
        finish_reason: choices[0]["finish_reason"].as_str().map(|s| s.to_string()),
        usage,
    })
}

use crate::config::{Config, ProviderConfig};

pub fn create_provider(config: &Config) -> Result<Arc<dyn Provider>> {
    let provider_name = select_provider(config);

    if provider_name == "anthropic" || provider_name == "claude" {
        return Err(anyhow!(
            "provider '{}' is not in current MVP (use OpenAI-compatible providers)",
            provider_name
        ));
    }

    let (provider_cfg, base_default, _model_default, kind, extra_headers, env_names) =
        provider_meta(config, &provider_name)?;

    let api_key = read_api_key(provider_cfg, &env_names)?;
    let base_url = provider_cfg
        .api_base
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| base_default.to_string());

    let provider = HttpProvider::new(api_key, base_url, extra_headers, kind);

    Ok(Arc::new(provider))
}

fn select_provider(config: &Config) -> String {
    let explicit = config.agents.defaults.provider.trim().to_lowercase();
    if !explicit.is_empty() {
        return explicit;
    }

    let model = config.agents.defaults.model.trim().to_lowercase();
    if let Some((prefix, _)) = model.split_once('/') {
        return prefix.to_string();
    }

    if config.providers.openrouter.api_key.is_some() {
        "openrouter".to_string()
    } else if config.providers.openai.api_key.is_some() {
        "openai".to_string()
    } else if config.providers.groq.api_key.is_some() {
        "groq".to_string()
    } else if config.providers.zhipu.api_key.is_some() {
        "zhipu".to_string()
    } else if config.providers.deepseek.api_key.is_some() {
        "deepseek".to_string()
    } else {
        "openrouter".to_string()
    }
}

type ProviderMeta<'a> = (
    &'a ProviderConfig,
    &'static str,
    &'static str,
    ProviderKind,
    HashMap<String, String>,
    Vec<&'static str>,
);

fn provider_meta<'a>(config: &'a Config, provider_name: &str) -> Result<ProviderMeta<'a>> {
    match provider_name {
        "openai" | "gpt" => Ok((
            &config.providers.openai,
            "https://api.openai.com/v1",
            "gpt-4o",
            ProviderKind::OpenAi,
            HashMap::new(),
            vec!["OPENAI_API_KEY"],
        )),
        "openrouter" => {
            let mut headers = HashMap::new();
            headers.insert("HTTP-Referer".to_string(), "https://femtors.ai".to_string());
            headers.insert("X-Title".to_string(), "femtors".to_string());
            Ok((
                &config.providers.openrouter,
                "https://openrouter.ai/api/v1",
                "openai/gpt-4o",
                ProviderKind::OpenRouter,
                headers,
                vec!["OPENROUTER_API_KEY"],
            ))
        }
        "groq" => Ok((
            &config.providers.groq,
            "https://api.groq.com/openai/v1",
            "llama-3.1-70b-versatile",
            ProviderKind::Groq,
            HashMap::new(),
            vec!["GROQ_API_KEY"],
        )),
        "zhipu" | "glm" => Ok((
            &config.providers.zhipu,
            "https://open.bigmodel.cn/api/paas/v4",
            "glm-4.7",
            ProviderKind::Zhipu,
            HashMap::new(),
            vec!["ZHIPU_API_KEY"],
        )),
        "deepseek" => Ok((
            &config.providers.deepseek,
            "https://api.deepseek.com/v1",
            "deepseek-chat",
            ProviderKind::DeepSeek,
            HashMap::new(),
            vec!["DEEPSEEK_API_KEY"],
        )),
        other => Err(anyhow!("unsupported provider '{}'", other)),
    }
}

fn read_api_key(cfg: &ProviderConfig, env_names: &[&str]) -> Result<String> {
    if let Some(key) = cfg.api_key.as_ref()
        && !key.trim().is_empty()
    {
        return Ok(key.clone());
    }
    for env_name in env_names {
        if let Ok(val) = std::env::var(env_name)
            && !val.trim().is_empty()
        {
            return Ok(val);
        }
    }
    Err(anyhow!("provider API key is not configured"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::post;
    use axum::{Json, Router};
    use once_cell::sync::Lazy;
    use tokio::sync::Mutex;
    use tokio::sync::oneshot;

    static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    #[derive(Clone)]
    struct MockState {
        expected_auth: String,
    }

    async fn mock_chat(
        State(state): State<MockState>,
        headers: HeaderMap,
        Json(body): Json<serde_json::Value>,
    ) -> (StatusCode, Json<serde_json::Value>) {
        let auth = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if auth != state.expected_auth {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "unauthorized" })),
            );
        }

        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if model.contains("tool") {
            return (
                StatusCode::OK,
                Json(serde_json::json!({
                    "choices": [{
                        "message": {
                            "content": "ok",
                            "tool_calls": [{
                                "id": "call_1",
                                "function": {
                                    "name": "read_file",
                                    "arguments": "{\"path\":\"README.md\"}"
                                }
                            }]
                        },
                        "finish_reason": "tool_calls"
                    }]
                })),
            );
        }

        (
            StatusCode::OK,
            Json(serde_json::json!({
                "choices": [{
                    "message": { "content": "hello from mock" },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 1,
                    "completion_tokens": 2,
                    "total_tokens": 3
                }
            })),
        )
    }

    async fn start_mock_server(expected_auth: &str) -> (String, oneshot::Sender<()>) {
        let app = Router::new()
            .route("/chat/completions", post(mock_chat))
            .with_state(MockState {
                expected_auth: expected_auth.to_string(),
            });

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (tx, rx) = oneshot::channel::<()>();

        tokio::spawn(async move {
            let server = axum::serve(listener, app).with_graceful_shutdown(async move {
                let _ = rx.await;
            });
            let _ = server.await;
        });

        (format!("http://{}", addr), tx)
    }

    #[test]
    fn parse_tool_calls() {
        let payload = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "ok",
                    "tool_calls": [{
                        "id": "call_1",
                        "function": {
                            "name": "read_file",
                            "arguments": "{\"path\":\"README.md\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let parsed = parse_openai_compatible_response(&payload).expect("parse should succeed");
        assert_eq!(parsed.content, "ok");
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].name.as_deref(), Some("read_file"));
    }

    #[test]
    fn normalize_message_serializes_openai_tool_call_shape() {
        let msg = Message {
            role: "assistant".to_string(),
            content: "calling tool".to_string(),
            tool_calls: vec![ToolCall {
                id: "call_123".to_string(),
                tool_type: "function".to_string(),
                function: Some(FunctionCall {
                    name: "read_file".to_string(),
                    arguments: "{\"path\":\"README.md\"}".to_string(),
                }),
                name: None,
                arguments: None,
            }],
            tool_call_id: None,
        };

        let v = normalize_message_for_provider(&msg);
        assert_eq!(v["role"], "assistant");
        assert_eq!(v["tool_calls"][0]["id"], "call_123");
        assert_eq!(v["tool_calls"][0]["type"], "function");
        assert_eq!(v["tool_calls"][0]["function"]["name"], "read_file");
        assert_eq!(
            v["tool_calls"][0]["function"]["arguments"],
            "{\"path\":\"README.md\"}"
        );
    }

    #[test]
    fn normalize_message_serializes_tool_result_shape() {
        let msg = Message::tool("done", "call_abc");
        let v = normalize_message_for_provider(&msg);
        assert_eq!(v["role"], "tool");
        assert_eq!(v["tool_call_id"], "call_abc");
        assert_eq!(v["content"], "done");
    }

    #[test]
    fn parse_tool_calls_defaults_type_and_id() {
        let payload = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "",
                    "tool_calls": [{
                        "function": {
                            "name": "read_file",
                            "arguments": "{\"path\":\"README.md\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let parsed = parse_openai_compatible_response(&payload).expect("parse should succeed");
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].id, "call_1");
        assert_eq!(parsed.tool_calls[0].tool_type, "function");
    }

    #[tokio::test]
    async fn config_key_wins_over_env() {
        let _guard = ENV_LOCK.lock().await;
        // SAFETY: guarded by ENV_LOCK to avoid concurrent env mutations in tests.
        unsafe { std::env::set_var("OPENAI_API_KEY", "env-key") };

        let (base, shutdown) = start_mock_server("Bearer config-key").await;

        let mut cfg = Config::default();
        cfg.agents.defaults.provider = "openai".to_string();
        cfg.providers.openai.api_key = Some("config-key".to_string());
        cfg.providers.openai.api_base = Some(base);

        let provider = create_provider(&cfg).expect("provider");
        let mut msgs = vec![Message::user("ping")];
        let response = provider
            .chat_with_options(&mut msgs, None, "gpt-4o", HashMap::new())
            .await
            .expect("chat should succeed");
        assert_eq!(response.content, "hello from mock");
        let _ = shutdown.send(());

        // SAFETY: guarded by ENV_LOCK to avoid concurrent env mutations in tests.
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
    }

    #[tokio::test]
    async fn env_fallback_works() {
        let _guard = ENV_LOCK.lock().await;
        // SAFETY: guarded by ENV_LOCK to avoid concurrent env mutations in tests.
        unsafe { std::env::set_var("OPENAI_API_KEY", "env-key") };

        let (base, shutdown) = start_mock_server("Bearer env-key").await;

        let mut cfg = Config::default();
        cfg.agents.defaults.provider = "openai".to_string();
        cfg.providers.openai.api_base = Some(base);

        let provider = create_provider(&cfg).expect("provider");
        let mut msgs = vec![Message::user("ping")];
        let response = provider
            .chat_with_options(&mut msgs, None, "gpt-4o", HashMap::new())
            .await
            .expect("chat should succeed");
        assert_eq!(response.content, "hello from mock");
        let _ = shutdown.send(());

        // SAFETY: guarded by ENV_LOCK to avoid concurrent env mutations in tests.
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
    }

    #[tokio::test]
    async fn missing_key_returns_error() {
        let _guard = ENV_LOCK.lock().await;
        // SAFETY: guarded by ENV_LOCK to avoid concurrent env mutations in tests.
        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
            std::env::remove_var("OPENROUTER_API_KEY");
            std::env::remove_var("GROQ_API_KEY");
            std::env::remove_var("ZHIPU_API_KEY");
            std::env::remove_var("DEEPSEEK_API_KEY");
        }

        let mut cfg = Config::default();
        cfg.agents.defaults.provider = "openai".to_string();
        let err = create_provider(&cfg).err().expect("expected missing key");
        assert!(err.to_string().contains("API key"));
    }

    #[tokio::test]
    async fn deepseek_provider_path_works() {
        let (base, shutdown) = start_mock_server("Bearer deepseek-key").await;

        let mut cfg = Config::default();
        cfg.agents.defaults.provider = "deepseek".to_string();
        cfg.providers.deepseek.api_key = Some("deepseek-key".to_string());
        cfg.providers.deepseek.api_base = Some(base);

        let provider = create_provider(&cfg).expect("provider");
        let mut msgs = vec![Message::user("tool please")];
        let response = provider
            .chat_with_options(&mut msgs, None, "deepseek-tool-model", HashMap::new())
            .await
            .expect("chat should succeed");
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name.as_deref(), Some("read_file"));
        let _ = shutdown.send(());
    }
}
