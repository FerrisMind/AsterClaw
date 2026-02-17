# Быстрый старт

## Требования

- Rust toolchain (`stable`)
- `cargo`
- Git
- (опционально) `mdbook` для локального просмотра документации

## Сборка

```bash
cargo build --release
```

## Первичная настройка

```bash
asterclaw onboard
```

Команда создаст конфиг и workspace:

- `~/.asterclaw/config.json`
- `~/.asterclaw/workspace`

## Запуск

```bash
asterclaw gateway
```

Проверка состояния:

```bash
asterclaw status
```

## Локальный запуск документации

```bash
mdbook serve docs
```
