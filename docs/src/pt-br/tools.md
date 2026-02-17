# Ferramentas

Registradas em `ToolRegistry::register_builtin_tools()` (`src/tools/mod.rs`).

## Filesystem

- `read_file`
- `write_file`
- `edit_file`
- `append_file`
- `list_dir`

## Shell

- `exec` (com políticas e limites de saída)
- timeout de execução: 30 segundos

## Web

- `web_search`
- `web_fetch` (com proteção SSRF e limites configuráveis)
- `web_search` limita `count` para `1..10`

## Mensageria/orquestração

- `message`
- `spawn`
- `subagent`

## Agendamento e memória

- `cron`
- `memory` (`read`, `write`, `append`, `read_daily`, `append_daily`)

## Limite de contexto de saída das tools

- `tools.tool_output_max_chars` limita o texto retornado ao contexto do LLM
- `0` desativa truncamento

## Device

- `i2c`
- `spi`

Em hosts não Linux, operações de device podem falhar em runtime.
