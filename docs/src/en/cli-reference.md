# CLI Reference

Based on `src/main.rs`.

## Core commands

```bash
asterclaw onboard
asterclaw agent [-m|--message <text>] [-s|--session <key>]
asterclaw gateway [--debug]
asterclaw status
asterclaw version
```

## Global flags

- `-d, --debug` (global): `asterclaw --debug <command>`
- `gateway --debug` is parsed, but currently adds no separate behavior beyond global `--debug`

## Cron

```bash
asterclaw cron list [--enabled-only]
asterclaw cron add --name <name> --message <text> [--every <sec> | --cron <expr>] [--channel <name>] [--chat-id <id>] [--enabled true|false]
asterclaw cron remove <id>
asterclaw cron enable <id>
asterclaw cron disable <id>
```

## Auth

```bash
asterclaw auth login [--provider <name>] [--token <token>] [--device-code]
asterclaw auth logout [--provider <name>]
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
