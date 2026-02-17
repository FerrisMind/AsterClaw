<p align="left">
  <a href="README.md"><img src="https://img.shields.io/badge/English-5B7CFA" alt="English"></a>
  <a href="README.RU.md"><img src="https://img.shields.io/badge/Русский-232323" alt="Русский"></a>
  <a href="README.PT_BR.md"><img src="https://img.shields.io/badge/Português_BR-232323" alt="Português"></a>
</p>

<h1 align="center">FemtoRS</h1>

<p align="center">
  Rust rewrite of PicoClaw — 34% less memory, zero GC pauses, full feature parity.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Rust-2024-DEA584?style=flat&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/Tests-52%20passing-brightgreen" alt="Tests">
  <img src="https://img.shields.io/badge/Clippy-0%20warnings-brightgreen" alt="Clippy">
  <img src="https://img.shields.io/badge/license-MIT-green" alt="License">
</p>

## 📚 Table of Contents

- [What is FemtoRS?](#-what-is-femtors)
- [Performance](#-performance)
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

## ✨ What is FemtoRS?

`FemtoRS` is a Rust rewrite of [PicoClaw](https://github.com/sipeed/picoclaw) — the ultra-lightweight Go AI assistant. FemtoRS achieves full gateway feature parity while delivering lower memory usage and zero garbage collection pauses:

- **73% smaller binary** (6.5 MB vs 23.9 MB)
- **34% less memory** (11.5 MB vs 17.3 MB RSS)
- **Zero GC pauses** — predictable latency under load
- **Full feature parity** — agent loops, Telegram, 14 tools, cron, heartbeat, migration
- **Dual config** (`.femtors` preferred, `.picoclaw` fallback) with real migration tooling

Canonical MVP contract: `mvp.md`.

## 📊 Performance

Measured with `scripts/nfr_baseline.ps1` — gateway startup to `/health` readiness.

**Test system:** Windows 11 Pro (26200), AMD Ryzen 7 3700X 8-Core, 64 GB RAM, NVMe SSD.

|                     | PicoClaw (Go) | **FemtoRS (Rust)** | Δ          |
| ------------------- | ------------- | ------------------ | ---------- |
| **Binary size**     | 23.9 MB       | **6.5 MB**         | **−73%**   |
| **RSS (steady)**    | 17.3 MB       | **11.5 MB**        | **−34%**   |
| **Startup**         | ~113 ms       | **~108 ms**        | −4%        |
| **GC pauses**       | Yes (Go GC)   | **None**           | —          |
| **Tests**           | —             | **52 passing**     | —          |
| **Clippy warnings** | —             | **0**              | —          |

> **Note:** Startup times are I/O-bound (config read + TCP bind) where both languages perform similarly.
> The real Rust advantage is under sustained load: no GC stop-the-world pauses, lower tail latency, and smaller memory footprint on constrained devices.
> PicoClaw claims <10 MB RAM but [notes](https://github.com/sipeed/picoclaw#readme) that recent PRs push it to 10–20 MB. FemtoRS at 11.5 MB is closer to that original target than Go's current 17.3 MB.

Run the benchmark yourself:

```powershell
# Rust only
.\scripts\nfr_baseline.ps1

# Compare with Go baseline
.\scripts\nfr_baseline.ps1 -GoBaseline path\to\picoclaw.exe
```

## 🚀 MVP Scope

| Area | Description |
| --- | --- |
| Gateway + agent loop | Consume inbound messages, run tools/providers, publish outbound events, notify channels |
| Telegram channel | Polling mode, allowlist/username filters, `/help`/`/list`/`/show`, markdown-safe outbound |
| Provider pipeline | OpenAI-compatible layer with config-driven provider selection (explicit provider → model prefix → OpenRouter) |
| Core tools (14) | Filesystem, guarded `exec`, `web_search`, `web_fetch`, `message`, `spawn`, `subagent`, `cron`, `i2c`, `spi` |
| Config + state | Dual read paths (`.femtors` → `.picoclaw`), sanitized Windows-safe session names, atomic saves |
| Health & cron | `/health` + `/ready` endpoints, cron CRUD persisted under `workspace/cron/jobs.json` |
| Migration | `femtors migrate` mirrors legacy `.picoclaw` layout (config + workspace) with dry-run, scope flags, backups |
| NFR benchmark | `scripts/nfr_baseline.ps1` — RSS and startup comparison with Go baseline |

## ⚡ Quick Commands

- `cargo check`
- `cargo clippy -- -D warnings`
- `cargo test`
- `femtors gateway`
- `femtors cron list`
- `femtors migrate --dry-run`
- `femtors status`
- `.\scripts\nfr_baseline.ps1`

Additional commands (`agent`, `onboard`, `skills`, `auth`, `heartbeat`, `devices`) are documented in `PLAN.md`.

## 🛠️ Configuration

- **Primary config:** `~/.femtors/config.json` (written by onboarding, CLI commands, and cron service).
- **Legacy fallback:** If `.femtors` is absent, runtime reads `~/.picoclaw/config.json`, translating camelCase/snake_case keys before merging.
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

`femtors migrate` is treated as a first-class MVP feature:

1. `--dry-run` reports planned copies/conversions without modifying anything.
2. `--config-only` or `--workspace-only` limit the migration scope.
3. `--force` backs up existing `.femtors` files (under `~/.femtors/backups`) before overwriting.
4. Legacy provider keys, sessions, skills, and memory files are migrated to the new layout with summaries (copied/skipped/errors).

Migration also sanitizes session filenames so Windows paths stay valid.

## 🧰 Running & Testing

1. Install Rust 2024 toolchain (`rust-toolchain.toml`) via `rustup`.
2. Run `cargo check`, `cargo clippy -- -D warnings`, and `cargo test` before committing.
3. Start the gateway locally with `femtors gateway`; logs include agent loops, provider requests, and Telegram polling.
4. Use `femtors cron list` and `femtors migrate --dry-run` during development to validate cron persistence and migration logic.

## 🤝 Contributing

- Follow the phased roadmap in `PLAN.md` to understand upcoming work.
- Keep CLI help text synchronized with implemented commands (no placeholder `TODO`s).
- Document new user-visible features in all three README files.
- Preserve the dual `.femtors`/`.picoclaw` compatibility guarantees when touching config/state code.

For blockers or ongoing work, see `error.md`.

## 📄 License

MIT — see [LICENSE](LICENSE) for details.
