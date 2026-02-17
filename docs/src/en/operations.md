# Operations

## Start gateway

```bash
asterclaw gateway
```

## Health endpoints

- `GET /health`
- `GET /ready`

## Common operational commands

```bash
asterclaw status
asterclaw cron list
asterclaw auth status
asterclaw skills list
```

Restart gateway after editing `~/.asterclaw/config.json`.

For verbose logs, use `asterclaw --debug gateway`.
`asterclaw gateway --debug` is currently equivalent to normal gateway run (without global debug).
