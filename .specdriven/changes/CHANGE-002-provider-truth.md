# Mudança CHANGE-002 — modelos dinâmicos, hooks por provider, verdade de conexão (SPEC-003/004/005)

## Escopo e estado
- Pedido: lista de modelos dinâmica; hooks por provider ativo na seção do card com expansão ao conectar; validação real de conexão + bug Codex OAuth.
- Repo `windows` apenas; base `feat/multi-provider` @ 4853f68.
- Owners exclusivos: agente único; sem worktrees.
- Estado: PLANNING → IMPLEMENTING → gates

## Intake (decisões do usuário)
- Modelos: fetch na API do provider **ao abrir Settings**; fallback silencioso pros defaults embutidos quando falhar (OAuth pode recusar `/models`).
- Hooks: **só onde a CLI suporta** — Claude (`~/.claude/settings.json`), Codex (`~/.codex/hooks.json` + `features.hooks`), Gemini (`~/.gemini/settings.json`); hermes/custom sem seção; seção escondida quando a CLI não é detectada; card expande a seção ao conectar com sucesso.
- Conexão: `connected` significa **probe real na API** (reusa `list_models`), não presença de secret.
- Callback OAuth: mensagem neutra até a exchange terminar; erro real visível no card via evento.

## Pesquisa (planning)
- **Codex exchange**: codex-rs usa `TokenEncoding::Form` (form-urlencoded) — nosso formato já está correto; causa provável do bug é **limite de ~2560 bytes do Credential Manager** (bundle Codex com id_token JWT de org claims). Mitigação: extrair `chatgpt_account_id` e não persistir `id_token`; propagar erro de `secrets::set` em vez de só eprintln.
- **Codex hooks**: `~/.codex/hooks.json` OU `[hooks]` em config.toml; eventos SessionStart/SessionEnd/UserPromptSubmit/PreToolUse/PostToolUse/PermissionRequest/Notification?/Stop/Subagent*; precisa `features.hooks = true` em config.toml; **hooks novos ficam "needs trust" até o usuário rodar `/hooks` no codex** — limitação honesta, documentada na seção.
- **Gemini hooks**: `~/.gemini/settings.json` `hooks.{Event}[{matcher,sequential,hooks:[{name,type:"command",command}]}]`; eventos SessionStart/SessionEnd/BeforeAgent/AfterAgent/BeforeTool/AfterTool/Notification/PreCompress; sem PermissionRequest equivalente → observação only.
- **Models endpoints**: anthropic `GET /v1/models` (x-api-key|bearer) → `data[].id`; openai-compatible `GET {base}/models` bearer → `data[].id`; gemini `GET {base}/models` → `models[].name` (prefixo `models/` removido).

## Contrato
- Classificação: NO_API — outbound a APIs de terceiros e arquivos locais; nenhum contrato entre consumers configurados.
- Efeitos: escreve em `~/.codex/{config.toml,hooks.json}` e `~/.gemini/settings.json` além de `~/.claude/settings.json` — mesma disciplina (backup+merge+diff+confirmação).
- `coucou-hook.exe` ganha argv `--provider <id>`; payload recebe campo `provider`. Só claude/codex têm evento que espera decisão (PermissionRequest); demais fire-and-forget ≤300ms/2s budgets inalterados.

## Plano e critérios
| Passo | Owner/arquivo | Mudança | Critério |
|---|---|---|---|
| 1 | `providers/oauth.rs` | callback neutro; `into_tokens` propaga erro de store; exchange codex tenta form→JSON fallback defensivo | teste: resposta neutra; erro propagado |
| 2 | `providers/mod.rs` + `openai.rs` | `OAuthTokens.account_id`; codex extrai de id_token e não o persiste | teste: bundle <2560B típico; account id lido do campo |
| 3 | `providers/*.rs` | `list_models` por adapter + `probe` em mod.rs | teste: parse data[].id / models[].name; erro→fallback |
| 4 | `lib.rs` + bridge/state/main.ts | `provider_probe` async; card mostra checking/ok/erro real; dropdown usa probe models | tsc + teste de status mapping |
| 5 | `hook/src/main.rs` | argv `--provider <id>`; payload `provider`; waits só claude/codex PermissionRequest | teste hook exe: parse argv, injeção |
| 6 | `hooks.rs` → `hooks/` | shared (backup/fingerprint/diff) + claude/codex/gemini specs; codex mescla hooks.json + config.toml(toml_edit); detecção de CLI | testes de merge por provider |
| 7 | `lib.rs` + UI | `provider_hooks_{status,preview,write}`; seção no card; expande ao conectar; seção standalone "Claude Code" migra pro card | cargo check + tsc |

## TDD seams
| Seam | Repo | Evidência test-first |
|---|---|---|
| `providers::probe` + `list_models` (shape por adapter, fallback) | windows | testes red antes do impl |
| `oauth` callback neutro + store-error propagation | windows | teste de texto/resultado |
| `hook` argv `--provider` + `waits_for_answer` por provider | windows | teste no exe |
| `hooks::{codex,gemini}` merge/unmerge + codex config.toml | windows | testes de merge espelhando claude |

## Worktrees
- Não.

## Resultado (implementação)

- **T-006 modelos dinâmicos** — `anthropic::list_models` (`GET /v1/models` → `data[].id`), `openai::list_models` (`GET {base}/models`, respeita `customBaseUrl`), `google::list_models` (`GET {base}/models` → `models[].name`, prefixo `models/` removido; x-goog-api-key ou bearer). `providers::dispatch_models` roteia por provider.
- **T-008 verdade de conexão** — `providers::probe` + comando `provider_probe`: `none` (sem credencial) / `connected` (API respondeu) / `failed` (401/403) / `unverified` (outro erro de listagem). Card mostra `checking…` → status real; falha exibe o erro da API. Callback page neutra ("Code received — finishing in Coucou"); erro do evento `provider-oauth-complete` em destaque no card.
- **Bug Codex OAuth** — `OAuthTokens.account_id`: `chatgpt_account_id` extraído do `id_token` e o JWT **não é persistido** (bundle ficava >2560B e o Credential Manager descartava). Erro de `secrets::set` propagado ao invés de só logar.
- **T-007 hooks por provider** — `hooks.rs` → `hooks/` (mod + claude/codex/gemini specs): `CliSpec` descreve arquivos, eventos, shape de entrada e nota por CLI. Claude inalterado (`settings.json`, comando `"exe" <event>`). Codex mescla `~/.codex/hooks.json` + `[features] hooks=true` em `config.toml` (toml_edit), nota sobre trust review `/hooks`. Gemini usa entries `{matcher:"*",hooks:[{name:"coucou",...}]}`, timeout em ms. `coucou-hook` aceita `coucou-hook <provider> <Event>` e injeta `provider` no payload; argv antigo continua válido (default claude). Painel de hooks dentro do card do provider; comandos `hooks_*` ganharam arg `provider`.
- **Testes**: 33 lib + 3 hook green (novos: gemini entry shape, codex config.toml merge, account_id JWT, expired-token fail-closed, models parsing). `tsc` limpo; clippy com só os 2 warnings pré-existentes de `island.rs`.
