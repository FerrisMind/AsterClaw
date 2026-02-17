# Referência de CLI

Baseado em `src/main.rs`.

## Comandos principais

```bash
asterclaw onboard
asterclaw agent [-m|--message <texto>] [-s|--session <chave>]
asterclaw gateway [--debug]
asterclaw status
asterclaw version
```

## Flags globais

- `-d, --debug` (global): `asterclaw --debug <command>`
- `gateway --debug` é aceito no parse, mas hoje não adiciona comportamento separado além do `--debug` global

## Cron

```bash
asterclaw cron list [--enabled-only]
asterclaw cron add --name <nome> --message <texto> [--every <seg> | --cron <expr>] [--channel <nome>] [--chat-id <id>] [--enabled true|false]
asterclaw cron remove <id>
asterclaw cron enable <id>
asterclaw cron disable <id>
```

## Auth

```bash
asterclaw auth login [--provider <nome>] [--token <token>] [--device-code]
asterclaw auth logout [--provider <nome>]
asterclaw auth status
```

## Skills

```bash
asterclaw skills list
asterclaw skills install <owner/repo/path>
asterclaw skills remove <name>
asterclaw skills search
asterclaw skills show <name>
```

## Migrate

```bash
asterclaw migrate [--dry-run] [--config-only] [--workspace-only] [--force]
```
