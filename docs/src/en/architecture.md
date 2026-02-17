# Architecture

Core modules:

- `main.rs` — CLI and runtime bootstrap
- `agent.rs` — agent loop and tool-call orchestration
- `tools/*` — tool implementations
- `channels.rs` — channel adapters
- `providers/*` — LLM provider adapters
- `health.rs` — `/health` and `/ready`

Request flow:

1. inbound message is received
2. context is built
3. model may call tools
4. tool results are returned to model context
5. final response is emitted and session is persisted
