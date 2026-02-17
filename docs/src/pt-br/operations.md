# Operações

## Iniciar gateway

```bash
asterclaw gateway
```

## Health endpoints

- `GET /health`
- `GET /ready`

## Comandos operacionais

```bash
asterclaw status
asterclaw cron list
asterclaw auth status
asterclaw skills list
```

Após editar `~/.asterclaw/config.json`, reinicie o gateway.

Para logs detalhados, use `asterclaw --debug gateway`.
`asterclaw gateway --debug` hoje é equivalente ao run normal (sem debug global).
