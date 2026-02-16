<p align="left">
  <a href="README.md"><img src="https://img.shields.io/badge/English-5B7CFA" alt="English"></a>
  <a href="README.RU.md"><img src="https://img.shields.io/badge/Русский-232323" alt="Русский"></a>
  <a href="README.PT_BR.md"><img src="https://img.shields.io/badge/Português_BR-232323" alt="Português"></a>
</p>

<h1 align="center">picors</h1>

<p align="center">
  Rust 2024 MVP port of PicoClaw that ships a working `gateway` with Telegram, OpenAI-compatible providers, cron jobs, and a real `migrate` workflow.
</p>

## 📚 Table of Contents

- [What is picors?](#-what-is-picors)
- [MVP Scope](#-mvp-scope)
- [Quick Commands](#-quick-commands)
- [Configuration](#-configuration)
- [Provider Strategy](#-provider-strategy)
- [Tooling & Messaging](#-tooling--messaging)
- [Health, Cron, Heartbeat](#-health-cron-heartbeat)
- [Migration Command](#-migration-command)
- [Running & Testing](#-running--testing)
- [Contributing](#-contributing)
- [License](#-license)

## ✨ What is picors?

`picors` is a command-line gateway built in Rust 2024 that mirrors PicoClaw’s agent stack. It focuses on:

- agent loops + message bus with outbound channel dispatch
- a polling Telegram channel with allowlist/username filters
- OpenAI-compatible provider pipeline (OpenAI/OpenRouter/Groq/Zhipu/DeepSeek)
- dual configuration (`.picors` preferred, `.picoclaw` fallback) plus real migration tooling
- cron job persistence and health endpoints (`/health`, `/ready`)

The goal is a **minimal but practical MVP**: working `gateway`, Telegram channel, provider tools, and CLI support (`migrate`, `cron`, `status`, `agent`, etc.).

## 🚀 MVP Scope

| Area | Description |
| --- | --- |
| Gateway + agent loop | Consume inbound messages, run tools/providers, publish outbound events, notify channels |
| Telegram channel | Polling mode, allowlist/username filters, `/help`/`/list`/`/show`, markdown-safe outbound |
| Provider pipeline | OpenAI-compatible layer with config-driven provider selection (explicit provider → model prefix → OpenRouter) |
| Core tools | Filesystem tools, guarded `exec`, `web_search`, `web_fetch`, channel `message` context |
| Config + state | Dual read paths (`.picors` → `.picoclaw`), sanitized Windows-safe session names, atomic saves |
| Health & cron | `/health` + `/ready` endpoints, cron CRUD persisted under `workspace/cron/jobs.json` |
| Migration | `picors migrate` mirrors legacy `.picoclaw` layout (config + workspace) with dry-run, scope flags, backups |

## ⚡ Quick Commands

- `cargo check`
- `cargo clippy -- -D warnings`
- `cargo test`
- `picors gateway`
- `picors cron list`
- `picors migrate --dry-run`
- `picors status`

Additional commands (`agent`, `onboard`, `skills`, `auth`, `heartbeat`, `devices`) are documented in `PLAN.md`.

## 🛠️ Configuration

- **Primary config:** `~/.picors/config.json` (written by onboarding, CLI commands, and cron service).
- **Legacy fallback:** If `.picors` is absent, runtime reads `~/.picoclaw/config.json`, translating camelCase/snake_case keys before merging.
- **Workspace:** Sessions, cron jobs, and skills live inside the configured `workspace.path` (or `workspace` directory under the current workspace) with atomic persistence.
- **State:** Sessions are stored using Windows-safe filenames (sanitize `:` → `_`), with atomic writes to avoid corruption.

## 🎛️ Provider Strategy

1. **OpenAI** — default when `providers.openai` or model prefixes imply OpenAI APIs.
2. **OpenRouter** — fallback when `api_base` contains `openrouter.ai` and no explicit provider is set.
3. **Groq / Zhipu / DeepSeek** — supported via the shared OpenAI-compatible layer (provider selection from config/model prefix).
4. **Environment fallback** — `OPENAI_API_KEY`/`OPENAI_API_BASE` and their provider-specific equivalents are consulted when config lacks keys, but config values always override env.

Providers share a unified request/response parser, making features like tool calls and streaming consistent across the stack.

## 🧱 Tooling & Messaging

- **Filesystem tools** guard against directory traversal and avoid aggressive `canonicalize()` on child writes.
- **`exec` tool** enforces workspace policies and filters dangerous patterns before spawning commands.
- **`web_search`** integrates Brave/DDG endpoints via the tool interface when enabled (`web_search.enabled`).
- **`web_fetch`** downloads online resources and returns structured metadata for prompts.
- **`message` tool** lets models speak directly through Telegram or other channels without re-publishing outbound events.

## 🧪 Health, Cron, Heartbeat

- Health server exposes `/health` (liveness) and `/ready` (gateway readiness) endpoints within the `gateway` process.
- Cron jobs persist in `workspace/cron/jobs.json`; CLI commands allow `add/list/remove/enable/disable`, and the cron runner schedules JSON-backed jobs.
- Heartbeat is minimal in MVP — only triggers periodically when configured and paired with cron services.

## 🔁 Migration Command

`picors migrate` is treated as a first-class MVP feature:

1. `--dry-run` reports planned copies/conversions without modifying anything.
2. `--config-only` or `--workspace-only` limit the migration scope.
3. `--force` backs up existing `.picors` files (under `~/.picors/backups`) before overwriting.
4. Legacy provider keys, sessions, skills, and memory files are migrated to the new layout with summaries (copied/skipped/errors).

Migration also sanitizes session filenames so Windows paths stay valid.

## 🧰 Running & Testing

1. Install Rust 2024 toolchain (`rust-toolchain.toml`) via `rustup`.
2. Run `cargo check`, `cargo clippy -- -D warnings`, and `cargo test` before committing.
3. Start the gateway locally with `picors gateway`; logs include agent loops, provider requests, and Telegram polling.
4. Use `picors cron list` and `picors migrate --dry-run` during development to validate cron persistence and migration logic.

## 🤝 Contributing

- Follow the phased roadmap in `PLAN.md` to understand upcoming work.
- Keep CLI help text synchronized with implemented commands (no placeholder `TODO`s).
- Document new user-visible features in all three README files.
- Preserve the dual `.picors`/`.picoclaw` compatibility guarantees when touching config/state code.

For blockers or ongoing work, see `error.md`.

## 📄 License

MIT — see [LICENSE](LICENSE) for details.
