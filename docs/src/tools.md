# Инструменты

Набор берется из `ToolRegistry::register_builtin_tools()` в `src/tools/mod.rs`.

## Файловые

- `read_file`
- `write_file`
- `edit_file`
- `append_file`
- `list_dir`

## Shell

- `exec`

Особенности:

- проверка policy (`auto_allow` / `require_confirm` / `always_deny`)
- ограничение stdout/stderr через конфиг
- timeout выполнения команды: 30 секунд

## Web

- `web_search`
- `web_fetch`

Особенности:

- SSRF guard для приватных/локальных адресов
- лимиты на размер ответа через `tools.web.*`
- `web_search` ограничивает `count` в диапазоне `1..10`

## Оркестрация/канал

- `message`
- `spawn`
- `subagent`

## Планировщик и память

- `cron`
- `memory`

`memory` поддерживает действия:

- `read`
- `write`
- `append`
- `read_daily`
- `append_daily`

## Лимит контекста tool-ответов

- `tools.tool_output_max_chars` ограничивает размер текста, который возвращается обратно в LLM-контекст
- `0` отключает обрезку

## Device

- `i2c`
- `spi`

На не-Linux системах возможны ожидаемые runtime-ошибки для device-команд.
