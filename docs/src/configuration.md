# Конфигурация

Актуально по `src/config.rs`.

## Где лежит конфиг

- Основной: `~/.asterclaw/config.json`
- Legacy fallback: `~/.picoclaw/config.json` (если новый отсутствует)
- Если задан `ASTERCLAW_HOME`, путь резолвится как `ASTERCLAW_HOME/config.json`

Базовый шаблон:

- `config/config.example.json`

## Полная структура

`Config` содержит:

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

Важно:

- при `enabled=true` поле `token` обязательно
- `allow_from` для Telegram private-режима обязательно (иначе канал не стартует)

## `providers`

Секции в схеме `Config`:

- `openai`
- `openrouter`
- `groq`
- `zhipu`
- `gemini`
- `deepseek`
- `anthropic`

Фактически реализованные провайдеры в рантайме (`src/providers/mod.rs`):

- `openai`
- `openrouter`
- `groq`
- `zhipu` (`glm`)
- `deepseek`

Важно:

- `anthropic`/`claude` в текущем MVP явно отклоняется
- `gemini` секция есть в конфиге, но как провайдер в рантайме пока не реализован

Поля секции провайдера:

- `api_key`
- `api_base`
- `proxy`
- `auth_method`
- `connect_mode`

API-ключ можно задать через конфиг или через env-переменные:

- `OPENAI_API_KEY`
- `OPENROUTER_API_KEY`
- `GROQ_API_KEY`
- `ZHIPU_API_KEY`
- `DEEPSEEK_API_KEY`

## `gateway`

- `host` (по умолчанию `127.0.0.1`)
- `port` (по умолчанию `18790`)

## `runtime`

- `worker_threads`
- `max_blocking_threads`

Рекомендации для низкой памяти:

- `worker_threads: 1`
- `max_blocking_threads: 8..16`

## `tools.web`

- `brave.enabled`
- `brave.api_key`
- `brave.max_results`
- `duckduckgo.enabled`
- `duckduckgo.max_results`
- `fetch_default_max_chars`
- `fetch_hard_max_chars`
- `fetch_hard_max_bytes`

Для Brave API ключ берется из `tools.web.brave.api_key` или `BRAVE_API_KEY`.

## `tools.exec`

- `confirm_unknown`
- `auto_allow_prefixes`
- `require_confirm_prefixes`
- `always_deny_prefixes`
- `stdout_max_bytes`
- `stderr_max_bytes`

## `tools.tool_output_max_chars`

Лимит tool-output в LLM-контексте:

- `0` — без обрезки
- `>0` — ограничение размера

## `heartbeat`

- `enabled`
- `interval`

## `devices`

- `enabled`
- `monitor_usb`
