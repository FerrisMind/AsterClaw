# Guia de Desenvolvimento

## Estrutura do projeto

- `src/main.rs` — bootstrap de CLI/runtime
- `src/agent.rs` — loop do agente
- `src/tools/` — ferramentas
- `src/channels.rs` — canais
- `src/providers/` — provedores LLM
- `src/config.rs` — schema de configuração
- `tests/` — testes e2e
- `scripts/` — scripts de NFR/profiling

## Fluxo local

```bash
cargo check
cargo fmt
cargo clippy -- -D warnings
cargo test
```

## Como adicionar uma tool

1. criar módulo em `src/tools/`
2. implementar `Tool` trait
3. exportar e registrar em `src/tools/mod.rs`
4. adicionar testes (sucesso/erro/edge)
5. atualizar docs em `docs/src`, `docs/src/en`, `docs/src/pt-br`

## Como manter docs alinhada ao código

- CLI: `cargo run -- --help`, `cargo run -- <subcomando> --help`
- Config: `src/config.rs` e `config/config.example.json`
- Tools: `src/tools/mod.rs` e `src/tools/*.rs`
- Build de docs: `mdbook build docs`
