<p align="left">
  <a href="README.md"><img src="https://img.shields.io/badge/English-232323" alt="English"></a>
  <a href="README.RU.md"><img src="https://img.shields.io/badge/Русский-232323" alt="Русский"></a>
  <a href="README.PT_BR.md"><img src="https://img.shields.io/badge/Português_BR-3ABF7A" alt="Português"></a>
</p>

<h1 align="center">AsterClaw</h1>

<p align="center">
  Porta Rust 2024 do PicoClaw com gateway funcional, canal Telegram, provedores OpenAI-compativeis e comando `migrate`.
</p>

## 🎬 Demo

https://github.com/user-attachments/assets/3fd498ce-77de-4f2d-b100-e807ef06f2e0

## 📚 Índice

- [O que é AsterClaw?](#-o-que-é-asterclaw)
- [Escopo do MVP](#-escopo-do-mvp)
- [Comandos rápidos](#-comandos-rápidos)
- [Configuração](#-configuração)
- [Estratégia de provedores](#-estratégia-de-provedores)
- [Ferramentas e mensagens](#-ferramentas-e-mensagens)
- [Health, Cron, Heartbeat](#-health-cron-heartbeat)
- [Comando de migração](#-comando-de-migração)
- [Execução e testes](#-execução-e-testes)
- [Contribuindo](#-contribuindo)
- [Licença](#-licença)

## ✨ O que é AsterClaw?

`AsterClaw` é um gateway CLI em Rust 2024 que reproduz o stack do PicoClaw com loop de agentes, barramento de mensagens e dispatch para canais. O MVP prioriza:

- gateway + envio outbound
- canal Telegram em polling com filtros (allowlist/username)
- camada compatível com OpenAI (OpenAI/OpenRouter/Groq/Zhipu/DeepSeek)
- compatibilidade dupla (`.asterclaw` → `.picoclaw`) e comando de migração real
- armazenamento de cron e endpoints `/health`, `/ready`

Objetivo: um MVP mínimo, porém utilizável, com `gateway`, Telegram, provedores e CLI completo (`migrate`, `cron`, `status`, `agent`, etc.).

Contrato canônico do MVP: `mvp.md`.

## 🚀 Escopo do MVP

| Área | Descrição |
| --- | --- |
| Gateway + loop de agentes | Consome inbound, dispara ferramentas/provedores, publica outbound, notifica canais |
| Canal Telegram | Polling, filtros (allowlist/username), comandos `/help`/`/list`/`/show`, saída markdown segura |
| Provedores | Camada compatível com OpenAI controlada por config (forçar provedor → prefixo → fallback OpenRouter) |
| Ferramentas | Operações de arquivos, `exec` protegido, `web_search`, `web_fetch`, contexto `message` |
| Config + estado | Leitura dual (`.asterclaw` depois `.picoclaw`), nomes de sessão seguros para Windows, gravações atômicas |
| Health & cron | `/health` + `/ready`, CRUD de cron persistido em JSON |
| Migração | `asterclaw migrate` transfere config/workspace antigos com dry-run, flags e backups |

## ⚡ Comandos rápidos

- `cargo check`
- `cargo clippy -- -D warnings`
- `cargo test`
- `asterclaw gateway`
- `asterclaw cron list`
- `asterclaw migrate --dry-run`
- `asterclaw status`
- `pwsh scripts/nfr-harness.ps1`

Outros comandos (`agent`, `onboard`, `skills`, `auth`, `heartbeat`, `devices`) estão no `PLAN.md`.

## 🛠️ Configuração

- **Config principal:** `~/.asterclaw/config.json` (escrito por onboarding, CLI e cron).
- **Fallback legado:** se `.asterclaw` falta, lê-se `~/.picoclaw/config.json`, convertendo camelCase/snake_case conforme necessário.
- **Workspace:** sessões, cron e skills vivem em `workspace.path` (ou pasta `workspace`), com gravação atômica.
- **Estado:** nomes de sessão têm `:` substituído por `_` para recortar problemas no Windows, e são salvos de forma segura.

## 🎛️ Estratégia de provedores

1. **OpenAI** — padrão quando `providers.openai` está presente ou o prefixo da modelo indica OpenAI.
2. **OpenRouter** — fallback quando `api_base` aponta para `openrouter.ai`.
3. **Groq / Zhipu / DeepSeek** — tratados pelo mesmo adaptador compatível com OpenAI (config + prefixo).
4. **Variáveis de ambiente** — `OPENAI_API_KEY` / `OPENAI_API_BASE` e similares são consultados somente se o config estiver vazio.

Todos os provedores compartilham parser unificado, tornando tool calls e streaming consistentes.

## 🧱 Ferramentas e mensagens

- Ferramentas de arquivo evitam travessias perigosas e não executam `canonicalize()` ao criar arquivos novos.
- `exec` aplica políticas de workspace e filtra padrões perigosos antes de executar processos.
- `web_search` usa Brave ou DuckDuckGo quando `web_search.enabled` está ativo.
- `web_fetch` baixa URLs e retorna metadados estruturados para o LLM.
- Ferramenta `message` envia texto diretamente para canais (como Telegram) sem repostar eventos outbound.

## 🧪 Health, Cron, Heartbeat

- Servidor `/health` e `/ready` roda dentro do gateway.
- Cron jobs são salvos em `workspace/cron/jobs.json`; CLI oferece `add/list/remove/enable/disable`.
- Heartbeat é mínimo: só dispara quando explícito no config (cron/heartbeat conectados).

## 🔁 Comando de migração

`asterclaw migrate` é uma funcionalidade importante do MVP:

1. `--dry-run` exibe o plano sem fazer alterações.
2. `--config-only` / `--workspace-only` restringem o escopo.
3. `--force` cria backup em `~/.asterclaw/backups` e sobrescreve arquivos.
4. Chaves legadas, sessões e arquivos de workspace são portados para o layout novo e o resultado (copiado/ignorado/erro) é exibido.

Também normaliza nomes de sessão para compatibilidade com Windows.

## 🧰 Execução e testes

1. Instale toolchain Rust 2024 (`rust-toolchain.toml`) via `rustup`.
2. Execute `cargo check`, `cargo clippy -- -D warnings`, `cargo test`.
3. Inicie o gateway com `asterclaw gateway`; os logs mostram loops de agentes, chamadas de provedores e polling Telegram.
4. Valide cron e migração com `asterclaw cron list` e `asterclaw migrate --dry-run`.

## 🤝 Contribuindo

- Siga o roteiro em `PLAN.md`.
- Sincronize o texto de ajuda do CLI com o comportamento real.
- Documente novos fluxos nas três versões do README.
- Preserve a compatibilidade dual `.asterclaw`/`.picoclaw` ao tocar em config/estado.

Bloqueadores e questões em andamento estão em `error.md`.

## 📄 Licença

MIT — consulte [LICENSE](LICENSE).
