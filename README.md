<p align="left">
  <a href="README.md"><img src="https://img.shields.io/badge/English-5B7CFA" alt="English"></a>
  <a href="README.RU.md"><img src="https://img.shields.io/badge/Русский-232323" alt="Русский"></a>
  <a href="README.PT_BR.md"><img src="https://img.shields.io/badge/Português_BR-232323" alt="Português"></a>
</p>

<h1 align="center">AsterClaw</h1>

<p align="center">
  Ultra-lightweight personal AI agent written in Rust.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Rust-2024-DEA584?style=flat&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/Tests-79%20passing-brightgreen" alt="Tests">
  <img src="https://img.shields.io/badge/Clippy-0%20warnings-brightgreen" alt="Clippy">
  <img src="https://img.shields.io/badge/license-MIT-green" alt="License">
</p>

## 🎬 Demo

https://github.com/FerrisMind/AsterClaw/raw/main/res/demo.mp4

## ✨ What is AsterClaw?

AsterClaw is an ultra-lightweight personal AI agent that runs as a local gateway, connects to LLM providers, and executes tools on your behalf. Built in Rust for maximum performance and minimal footprint.

## 📊 Performance Comparison

| Metric            | OpenClaw     | PicoClaw     | **AsterClaw**    |
| :---------------- | :----------- | :----------- | :--------------- |
| **Language**       | TypeScript   | Go           | **Rust**         |
| **Binary size**    | >100 MB      | 23.9 MB      | **~5 MB**        |
| **RAM usage**      | >1 GB        | <10 MB*      | **~11 MB**       |
| **Boot time**      | >500s        | <1s          | **<1s**          |
| **GC pauses**      | Yes (V8)     | Yes (Go GC)  | **None**         |
| **Tests**          | —            | —            | **79 passing**   |
| **Clippy warnings**| —            | —            | **0**            |

> \* PicoClaw claims <10 MB RAM but recent versions measure 17–20 MB.

### Why Rust?

- **No garbage collector** — predictable latency under load, no stop-the-world pauses
- **Tiny binary** — single ~5 MB executable, no runtime dependencies
- **Memory efficient** — ~11 MB RSS, ideal for embedded and constrained devices
- **Full feature parity** — 14 tools, Telegram, cron, heartbeat, migration

## 🚀 Features

| Area | Description |
| --- | --- |
| Gateway + agent loop | Consume inbound messages, run tools/providers, publish outbound events |
| Telegram channel | Polling mode, allowlist/username filters, `/help`/`/list`/`/show`, markdown-safe outbound |
| Provider pipeline | OpenAI-compatible layer with config-driven provider selection |
| Core tools (14) | `read_file`, `write_file`, `edit_file`, `append_file`, `list_dir`, `exec`, `web_search`, `web_fetch`, `message`, `spawn`, `subagent`, `cron`, `i2c`, `spi` |
| Config + state | `~/.asterclaw/config.json`, sanitized Windows-safe session names, atomic saves |
| Health & cron | `/health` + `/ready` endpoints, cron CRUD persisted in workspace |
| Migration | `asterclaw migrate` with dry-run, scope flags, and backups |

## ⚡ Quick Start

```bash
# Build
cargo build --release

# Initialize config
asterclaw onboard

# Start the gateway
asterclaw gateway

# Check status
asterclaw status
```

## 🛠️ Configuration

- **Config:** `~/.asterclaw/config.json` — created by `asterclaw onboard`.
- **Workspace:** Sessions, cron jobs, and skills live inside the configured workspace directory with atomic persistence.
- **State:** Sessions use Windows-safe filenames with atomic writes to avoid corruption.

## 🎛️ Provider Strategy

1. **OpenAI** — default when `providers.openai` or model prefixes imply OpenAI APIs.
2. **OpenRouter** — fallback when no explicit provider is set.
3. **Groq / Zhipu / DeepSeek / Anthropic** — supported via the shared OpenAI-compatible layer.
4. **Environment fallback** — `OPENAI_API_KEY` and provider-specific equivalents are consulted when config lacks keys.

Providers share a unified request/response parser, making tool calls and streaming consistent across the stack.

## 🧱 Tools

- **Filesystem tools** guard against directory traversal.
- **`exec`** enforces workspace policies and filters dangerous patterns before spawning commands.
- **`web_search`** integrates Brave/DuckDuckGo for web research.
- **`web_fetch`** downloads online resources and returns structured content.
- **`message`** lets the model speak directly through channels.
- **`cron`** schedules recurring messages and tasks.
- **`i2c` / `spi`** — device I/O for embedded/IoT use cases.

## 🧪 Health & Cron

- Health server exposes `/health` (liveness) and `/ready` (readiness) endpoints.
- Cron jobs persist in workspace; CLI commands allow `add/list/remove/enable/disable`.
- Heartbeat triggers periodically when configured.

## 🔁 Migration

`asterclaw migrate` migrates data from legacy configurations:

1. `--dry-run` — preview without changes.
2. `--config-only` or `--workspace-only` — limit scope.
3. `--force` — backup existing files before overwriting.

## 🧰 Development

```bash
cargo check
cargo clippy -- -D warnings
cargo test
```

## 📄 License

MIT — see [LICENSE](LICENSE) for details.
