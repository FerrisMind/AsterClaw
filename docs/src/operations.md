# Эксплуатация

## Запуск gateway

```bash
asterclaw gateway
```

## Health checks

- `GET /health` — liveness
- `GET /ready` — readiness

## Операционные команды

```bash
asterclaw status
asterclaw cron list
asterclaw auth status
asterclaw skills list
```

## Перезапуск после изменений

После изменения `~/.asterclaw/config.json` перезапустите процесс gateway.

## Диагностика

- Для подробных логов запускайте `asterclaw --debug gateway`
- `asterclaw gateway --debug` сейчас эквивалентен обычному запуску без глобального `--debug`
- Проверяйте `scripts/nfr_baseline.ps1`, `scripts/nfr-harness.ps1`, `scripts/simply-profile.ps1`
