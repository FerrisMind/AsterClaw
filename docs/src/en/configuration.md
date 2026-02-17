# Configuration

Current source of truth: `src/config.rs`.

## Config locations

- primary: `~/.asterclaw/config.json`
- legacy fallback: `~/.picoclaw/config.json`
- if `ASTERCLAW_HOME` is set, path resolves to `ASTERCLAW_HOME/config.json`

Template:

- `config/config.example.json`

## Top-level sections

- `agents`
- `channels`
- `providers`
- `gateway`
- `runtime`
- `tools`
- `heartbeat`
- `devices`

## `agents.defaults`

- `workspace`
- `restrict_to_workspace`
- `provider`
- `model`
- `max_tokens`
- `temperature`
- `max_tool_iterations`

## `channels.telegram`

- `enabled`
- `token`
- `proxy`
- `allow_from`

When Telegram is enabled, `token` and non-empty `allow_from` are required.

## `providers`

Sections present in `Config` schema:

- `openai`, `openrouter`, `groq`, `zhipu`, `gemini`, `deepseek`, `anthropic`

Providers actually implemented in runtime (`src/providers/mod.rs`):

- `openai`, `openrouter`, `groq`, `zhipu` (`glm`), `deepseek`

Important:

- `anthropic`/`claude` is explicitly rejected in current MVP
- `gemini` exists in config schema, but provider runtime support is not implemented yet

Fields per provider section:

- `api_key`, `api_base`, `proxy`, `auth_method`, `connect_mode`

API keys can be provided via config or env:

- `OPENAI_API_KEY`
- `OPENROUTER_API_KEY`
- `GROQ_API_KEY`
- `ZHIPU_API_KEY`
- `DEEPSEEK_API_KEY`

## `gateway`

- `host`
- `port`

## `runtime`

- `worker_threads`
- `max_blocking_threads`

Low-memory baseline:

- `worker_threads: 1`
- `max_blocking_threads: 8..16`

## `tools.web`

- `brave.*`
- `duckduckgo.*`
- `fetch_default_max_chars`
- `fetch_hard_max_chars`
- `fetch_hard_max_bytes`

For Brave, API key is taken from `tools.web.brave.api_key` or `BRAVE_API_KEY`.

## `tools.exec`

- `confirm_unknown`
- `auto_allow_prefixes`
- `require_confirm_prefixes`
- `always_deny_prefixes`
- `stdout_max_bytes`
- `stderr_max_bytes`

## `tools.tool_output_max_chars`

- `0` means no truncation
- `>0` applies truncation in tool loop context
