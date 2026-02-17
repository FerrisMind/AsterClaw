# Tools

Registered in `ToolRegistry::register_builtin_tools()` (`src/tools/mod.rs`).

## Filesystem

- `read_file`
- `write_file`
- `edit_file`
- `append_file`
- `list_dir`

## Shell

- `exec`

Includes policy checks and output limits.
Exec command timeout is 30 seconds.

## Web

- `web_search`
- `web_fetch`

Includes SSRF guard and configurable response bounds.
`web_search` clamps `count` to `1..10`.

## Messaging and orchestration

- `message`
- `spawn`
- `subagent`

## Scheduling and memory

- `cron`
- `memory` (`read`, `write`, `append`, `read_daily`, `append_daily`)

## Tool-output context limit

- `tools.tool_output_max_chars` limits what tool responses are sent back into LLM context
- `0` disables truncation

## Device

- `i2c`
- `spi`

On non-Linux hosts, device operations may fail at runtime by design.
