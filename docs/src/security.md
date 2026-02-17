# Безопасность

## Telegram

- При `channels.telegram.enabled = true` обязательны:
  - `token`
  - непустой `allow_from`
- Рекомендуется использовать только private chat + allow-list.

## Exec policy

Команды классифицируются на:

- auto-allow
- require-confirm
- always-deny

Рекомендации:

- `confirm_unknown = true`
- минимальный `auto_allow_prefixes`
- не ослабляйте `always_deny_prefixes` без необходимости

## Web security

- `web_fetch` блокирует локальные/приватные цели (SSRF guard)
- используйте разумные лимиты `tools.web.fetch_*`

## Секреты

- не храните реальные ключи в репозитории
- используйте локальный конфиг и secrets в CI/CD
