# Developer Guide

## Project layout

- `src/main.rs` — CLI bootstrap
- `src/agent.rs` — agent loop
- `src/tools/` — tools
- `src/channels.rs` — channel adapters
- `src/providers/` — provider adapters
- `src/config.rs` — config schema
- `tests/` — e2e tests
- `scripts/` — profiling/NFR scripts

## Local workflow

```bash
cargo check
cargo fmt
cargo clippy -- -D warnings
cargo test
```

## Adding a tool

1. create a module in `src/tools/`
2. implement `Tool` trait
3. export and register it in `src/tools/mod.rs`
4. add tests for success/error/edge cases
5. update docs in `docs/src`, `docs/src/en`, `docs/src/pt-br`

## Keeping docs aligned with code

- CLI: `cargo run -- --help`, `cargo run -- <subcommand> --help`
- Config: `src/config.rs` and `config/config.example.json`
- Tools: `src/tools/mod.rs` and `src/tools/*.rs`
- Docs build check: `mdbook build docs`
