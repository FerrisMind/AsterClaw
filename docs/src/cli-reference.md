# CLI справочник

Актуально по `src/main.rs`.

## Основные команды

```bash
asterclaw onboard
asterclaw agent [-m|--message <text>] [-s|--session <key>]
asterclaw gateway [--debug]
asterclaw status
asterclaw version
```

## Глобальные флаги

- `-d, --debug` (глобальный): `asterclaw --debug <command>`
- `gateway --debug` парсится, но отдельной логики кроме глобального `--debug` сейчас не добавляет

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

## Команды внутри чата агента

Агент поддерживает:

- `/help` и `/start`
- `/model`
- `/status`
- `/show model|channel`
- `/list models|channels`
- `/switch model to <name>` / `/switch channel to <name>`
