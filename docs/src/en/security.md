# Security

## Telegram

When `channels.telegram.enabled=true`:

- `token` is required
- non-empty `allow_from` is required

Use private chat + allow-list mode in production.

## Exec policy

Command classes:

- auto-allow
- require-confirm
- always-deny

Recommended baseline:

- keep `confirm_unknown = true`
- keep `auto_allow_prefixes` minimal
- avoid weakening `always_deny_prefixes`

## Web protection

- `web_fetch` blocks local/private targets (SSRF guard)
- keep `tools.web.fetch_*` bounds in place

## Secret management

- do not commit real API keys
- keep secrets in local config or CI secret store
