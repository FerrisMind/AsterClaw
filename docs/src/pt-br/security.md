# Segurança

## Telegram

Com `channels.telegram.enabled=true`:

- `token` é obrigatório
- `allow_from` não pode ser vazio

Use modo privado com allow-list em produção.

## Política de exec

Classes:

- auto-allow
- require-confirm
- always-deny

Recomendado:

- manter `confirm_unknown = true`
- reduzir `auto_allow_prefixes`
- não afrouxar `always_deny_prefixes` sem necessidade

## Proteção web

- `web_fetch` bloqueia alvos locais/privados (SSRF guard)
- mantenha limites `tools.web.fetch_*`

## Segredos

- não commitar API keys reais
- usar config local e secrets no CI/CD
