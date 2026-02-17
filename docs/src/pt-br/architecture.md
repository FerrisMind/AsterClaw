# Arquitetura

Módulos principais:

- `main.rs` — CLI e bootstrap de runtime
- `agent.rs` — loop do agente e orquestração de tools
- `tools/*` — implementações de ferramentas
- `channels.rs` — adaptadores de canal
- `providers/*` — adaptadores de provedores LLM
- `health.rs` — endpoints `/health` e `/ready`

Fluxo:

1. mensagem inbound
2. construção de contexto
3. possível chamada de ferramentas pela LLM
4. retorno do resultado para o contexto
5. resposta final e persistência da sessão
