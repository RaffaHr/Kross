# SPEC-001: Multi-provider AI no app Windows (adapters desacoplados)

- Data: 2026-10-01
- Estado: aprovado
- Origem: conversa (pedido do usuário após setup SpecDriven)
- Tracker ref: `.specdriven/tickets/T-001.md` … `T-004.md`

## Problema

O app Windows só fala com Claude: `src-tauri/src/claude.rs` é hardcoded para a Messages API da Anthropic (`x-api-key`, modelo fixo default `claude-opus-5`), e `secrets.rs` tem uma única chave `anthropic-api-key`. O usuário quer escolher entre vários providers de IA — Claude, Codex, Hermes (agent CLI), Google (via OAuth estilo gemini-cli, referido como "Antigravity") e um custom OpenAI-compatible (base URL + API key) — autenticando por API key **ou** OAuth por subscription.

## Objetivo e sucesso

- A aba Settings ganha uma seção **Providers**: lista os providers suportados, cada um mostrando estado (conectado/não conectado) e permitindo configurar credenciais.
- Cada provider suporta o(s) método(s) de auth que a spec define; tokens/keys ficam no Windows Credential Manager (whitelist estendida em `secrets.rs`), nunca em `settings.json` nem no front end.
- A island usa o **provider ativo** para o chat (incluindo contexto de arquivo/janela). Trocar de provider não exige restart.
- Estrutura `src-tauri/src/providers/` com um adapter por provider + um registry; nenhum adapter importa código de outro.
- Critérios verificáveis: `cargo test`/`tsc`/clippy verdes; conectar cada provider via UI; enviar uma mensagem de chat com provider ativo ≠ Claude e receber resposta; desconectar remove a credencial do Credential Manager.

## Escopo

### Dentro

- Camada `providers/` no backend Rust: trait/contrato comum (nome, métodos de auth suportados, envio de mensagem com histórico multi-turn e contexto de arquivo/janela), registry, e seleção de provider ativo persistida em `settings.json`.
- Adapters: **claude** (API key + OAuth Claude subscription), **codex** (API key OpenAI + OAuth ChatGPT estilo codex-cli), **google** (OAuth estilo gemini-cli; "Antigravity" na UI pode ser o rótulo), **openai-compatible** (base URL + key; serve de preset para **hermes** — Hermes Agent CLI é OpenAI-compatible por configuração de endpoint — e para o custom livre).
- Fluxo OAuth local: browser abre a URL de authorize com PKCE; um listener loopback `127.0.0.1` captura o `code`; troca e refresh de token no backend; tokens no Credential Manager.
- UI de Settings: por provider, campos de API key e/ou botão "Sign in"; campo base URL no custom; seletor de provider ativo; modelo por provider quando aplicável.
- Migração suave: `anthropic-api-key` existente continua funcionando como credential do provider Claude.

### Fora (não-escopo explícito)

- Observação de sessões de outros CLIs (Codex CLI, Gemini CLI, Hermes Agent). A ingestão de sessões continua sendo os hooks do Claude Code — isso é plumbing de input, não provider de IA. Fase futura se o usuário pedir.
- Streaming de respostas na island (a API atual já é request/response único; manter).
- Mudanças no app macOS (`NotchBuddy/`) — fora dos gates locais por decisão de setup.
- Rebrand para "Kross" (nome do fork) — não pedido; o produto segue "Coucou".
- Verificação E2E automatizada — sem harness; a validação de UI fica com o playwright-tester sobre `npm run dev`.

## Consumidores e paridade

- Consumers afetados: `windows`. Paridade: OFF por profile (divergências macOS↔Windows são intencionais).

## Contrato (somente API_REQUIRED)

- N/A — mudança `NO_API`: o app consome APIs de terceiros como cliente; não há contrato compartilhado entre consumidores configurados que mude.

## Riscos e perguntas fechadas

- **ToS de OAuth subscription**: o usuário optou explicitamente por OAuth para todos os providers cientes de que Anthropic/OpenAI/Google podem proibir uso fora do produto oficial e banir a conta. Registrado em `docs/adr/ADR-0002-provider-oauth-tos.md`. Mitigação: a UI declara o aviso ao lado de cada botão OAuth.
- **"Antigravity"**: é a IDE do Google sem API pública; o fechado no intake foi OAuth Google estilo gemini-cli (Code Assist/Gemini). Rótulo na UI pode ser "Google (Gemini)" — decidir copy na implementação.
- **"Hermes"**: Hermes Agent CLI — acesso via endpoint OpenAI-compatible; entra como preset do adapter genérico (base URL predefinida + key). Se o Hermes do usuário for outro, o custom provider cobre.
- **Risco técnico OAuth**: client IDs/endpoints públicos dos CLIs oficiais podem mudar; adapters devem concentrar essas constantes por provider.
- **Segredos**: OAuth refresh tokens são secrets — mesma disciplina de `secrets.rs` (whitelist, Credential Manager).

## Referências

- `windows/src-tauri/src/claude.rs` (cliente atual a virar adapter), `secrets.rs`, `settings.rs`, `hooks.rs`, `src/settings/main.ts`
- `docs/adr/ADR-0002-provider-oauth-tos.md`, `CONTEXT.md` (glossário provider/adapter/OAuth)
- Intake desta sessão: respostas do usuário (escopo "tudo configurável", OAuth aceito, Hermes=agent CLI, Antigravity=OAuth Google)
