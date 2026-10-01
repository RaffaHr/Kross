# SPEC-004: Hooks por provider ativo

- Data: 2026-10-01
- Estado: aprovado
- Origem: usuário (intake — "hooks devem depender do provider ativo, ficam na seção do provider, expandem ao conectar; cada provider tem configuração diferente")
- Tracker ref: `.specdriven/tickets/T-007.md`

## Problema

Hooks existem só para Claude Code (`~/.claude/settings.json`, seção própria na Settings). Com providers novos o usuário quer que a observação de sessões siga a CLI do provider ativo — Codex CLI e Gemini CLI têm configurações de hook próprias e diferentes.

## Objetivo e sucesso

- Cada provider com CLI que suporta hooks ganha uma **seção de hooks dentro do seu card** em Settings → Providers.
- A seção **expande ao conectar com sucesso** (evento `provider-oauth-complete` ok, ou salvar API key válida).
- Instalação escreve no config file daquela CLI seguindo a disciplina atual do Claude: backup datado, merge não-destrutivo, diff antes de confirmar, nunca sobrescreve.
- O hook instalado invoca `coucou-hook` com o id do provider; eventos entram no mesmo relay/pipe e a island pode distinguir a origem.
- CLI ausente no sistema ou sem suporte a hooks → seção não renderiza (não "disabled" — ausente, honesto). `hermes`/`custom` nunca têm seção.
- Comportamento do Claude inalterado.

## Escopo

### Dentro

- `hooks.rs` generalizado: um módulo por CLI (`hooks/claude.rs`, `hooks/codex.rs`, `hooks/gemini.rs` ou mapa de specs) expondo `status()`, `install()`, `uninstall()` para o arquivo de config daquela CLI.
- `coucou-hook` aceita o provider (flag `--provider <id>` ou primeiro argv) e marca os eventos com a origem.
- `provider_hooks_status(id)` comando + payload no `ProviderInfo` (`hooksSupported`, `hooksInstalled`).
- UI: subseção no card com estado (installed/not installed), botões Install/Uninstall e o diff/confirmação existente reaproveitado; expansão automática no `provider-oauth-complete`/`secret_set` ok.
- Pesquisa no planning dos formatos reais (codex `config.toml` hooks vs `notify`; gemini `settings.json` hooks) — o spec assume que existem hooks de ciclo de vida mínimos; eventos ausentes são documentados na seção, não fingidos.

### Fora

- macOS (seguimento do mesmo padrão fica pra quando o workflow cobrir NotchBuddy).
- Hooks em `hermes`/`custom` (não há CLI alvo).
- Normalização completa de sessões não-Claude na island — o relay marca a origem; views ricas por provider são follow-up.

## Consumidores e paridade

- `windows` apenas. Paridade N/A.

## Contrato (somente API_REQUIRED)

- N/A (`NO_API`) — arquivos locais e pipe, nenhuma API.

## Riscos e perguntas fechadas

- **Formato instável de hooks nas CLIs** (codex/gemini hooks são recentes): merge por chave namespaced (`coucou`) no config da CLI — nunca tocar em chaves do usuário.
- **Escopo "só onde a CLI suporta"** (decisão do intake): detecção de CLI instalada via arquivo de config/home dir ou `where`; seção escondida quando nada detectável.
- **Nunca bloquear a CLI** (invariante): o `coucou-hook` continua fail-fast ≤300 ms — o mesmo executável serve todos os providers.

## Arquivos prováveis

- `windows/src-tauri/src/hooks.rs` (→ módulos por CLI), `windows/hook/` (flag de provider), `lib.rs`, `src/settings/main.ts`, `src/core/bridge.ts`
