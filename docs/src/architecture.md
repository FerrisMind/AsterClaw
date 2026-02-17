# Архитектура

## Основные компоненты

- `main.rs` — CLI, запуск gateway/agent, загрузка конфига
- `agent.rs` — цикл агента, tool-calls, история сессии
- `tools/*` — реализация инструментов (`exec`, `web_*`, `fs`, `cron`, и т.д.)
- `channels.rs` — каналы общения (включая Telegram)
- `providers/*` — провайдеры LLM
- `health.rs` — `/health` и `/ready`

## Поток обработки

1. Сообщение попадает в bus/channel.
2. `AgentLoop` строит контекст и вызывает модель.
3. Если модель запросила инструмент, выполняется `ToolRegistry`.
4. Результат инструмента возвращается в LLM-контекст.
5. Ответ отправляется пользователю и сохраняется в сессию.

## Хранение данных

- Конфиг: `~/.asterclaw/config.json`
- Workspace: `~/.asterclaw/workspace`
- Сессии: `workspace/sessions`
- Память: `workspace/memory`
- Cron jobs: `workspace/cron`
