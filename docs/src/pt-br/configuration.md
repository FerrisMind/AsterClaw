# Configuração

Fonte atual: `src/config.rs`.

## Local do arquivo

- principal: `~/.asterclaw/config.json`
- fallback legado: `~/.picoclaw/config.json`
- se `ASTERCLAW_HOME` estiver definido, o caminho vira `ASTERCLAW_HOME/config.json`

Template:

- `config/config.example.json`

## Seções de topo

- `agents`
- `channels`
- `providers`
- `gateway`
- `runtime`
- `tools`
- `heartbeat`
- `devices`

## `agents.defaults`

- `workspace`
- `restrict_to_workspace`
- `provider`
- `model`
- `max_tokens`
- `temperature`
- `max_tool_iterations`

## `channels.telegram`

- `enabled`
- `token`
- `proxy`
- `allow_from`

Quando Telegram está ativo, `token` e `allow_from` não vazio são obrigatórios.

## `providers`

Seções presentes no schema `Config`:

- `openai`, `openrouter`, `groq`, `zhipu`, `gemini`, `deepseek`, `anthropic`

Provedores realmente implementados no runtime (`src/providers/mod.rs`):

- `openai`, `openrouter`, `groq`, `zhipu` (`glm`), `deepseek`

Importante:

- `anthropic`/`claude` é rejeitado explicitamente no MVP atual
- `gemini` existe no schema do config, mas ainda não tem implementação no runtime

Campos por seção:

- `api_key`, `api_base`, `proxy`, `auth_method`, `connect_mode`

As chaves também podem vir por variáveis de ambiente:

- `OPENAI_API_KEY`
- `OPENROUTER_API_KEY`
- `GROQ_API_KEY`
- `ZHIPU_API_KEY`
- `DEEPSEEK_API_KEY`

## `runtime`

- `worker_threads`
- `max_blocking_threads`

Base para pouca memória:

- `worker_threads: 1`
- `max_blocking_threads: 8..16`

## `tools.web`

- `brave.*`
- `duckduckgo.*`
- `fetch_default_max_chars`
- `fetch_hard_max_chars`
- `fetch_hard_max_bytes`

Para Brave, a chave pode vir de `tools.web.brave.api_key` ou `BRAVE_API_KEY`.

## `tools.exec`

- `confirm_unknown`
- `auto_allow_prefixes`
- `require_confirm_prefixes`
- `always_deny_prefixes`
- `stdout_max_bytes`
- `stderr_max_bytes`

## `tools.tool_output_max_chars`

- `0` desativa truncamento
- `>0` aplica limite
