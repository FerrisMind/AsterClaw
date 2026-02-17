# Руководство разработчика

## Структура проекта

- `src/main.rs` — CLI и bootstrap
- `src/agent.rs` — основной цикл агента
- `src/tools/` — инструменты
- `src/channels.rs` — каналы
- `src/providers/` — LLM-провайдеры
- `src/config.rs` — схема конфигурации
- `tests/` — e2e тесты
- `scripts/` — NFR/профилирование

## Локальная разработка

```bash
cargo check
cargo fmt
cargo clippy -- -D warnings
cargo test
```

## Как добавить новый инструмент

1. Создать файл в `src/tools/`
2. Реализовать `Tool` trait:
   - `name()`
   - `description()`
   - `parameters()`
   - `execute(...)`
3. Подключить модуль и `pub use` в `src/tools/mod.rs`
4. Зарегистрировать инструмент в `register_builtin_tools()`
5. Добавить тесты в `src/tools/mod.rs` (или отдельный модуль)

## Критерии качества

- Никаких паник в runtime-пути
- Понятные ошибки (`ToolResult::error`)
- Лимиты на память/вывод для тяжелых операций
- Покрытие тестами сценариев: happy path + ошибки + граничные случаи

## Документация

- Обновляйте `docs/src/*.md` при изменении CLI/конфига/инструментов
- Для многоязычных изменений синхронизируйте `ru`, `en`, `pt-br`

## Проверка соответствия коду

- CLI: `cargo run -- --help`, `cargo run -- <subcommand> --help`
- Конфиг: `src/config.rs` + `config/config.example.json`
- Инструменты: `src/tools/mod.rs` и конкретные модули `src/tools/*.rs`
- Сборка документации: `mdbook build docs`
