# Mudança CHANGE-001 — multi-provider adapters (SPEC-001)

## Escopo e estado
- Pedido: providers Claude/Codex/Hermes/Google(OAuth)/custom OpenAI-compatible na aba Settings, API key ou OAuth, adapters desacoplados — app Windows apenas.
- Repos/commits/diffs: repo `windows` (cwd `windows/`); base `feat/multi-provider` @ HEAD do setup commit.
- Owners exclusivos: agente único (este harness); sem worktrees.
- Estado: IMPLEMENTING → gates

## Setup e intake
- Profile configurado: true
- `profileHash`: resolvido em `.specdriven/repositories.json` (workflow.profile)
- Facilitator runs: setup-facilitator (profile+block), grill-facilitator modo `grill` (escopo/OAuth/Hermes/Antigravity), triage-facilitator (disposição T-001..T-005)
- INTAKE concluído: perguntas pendentes = 0

## Upstream (conforme `profile.stages`)
| Estágio | Política | Artefato/ref | sha256 ou item do tracker | Estado |
|---|---|---|---|---|
| triage | required | .specdriven/tickets/ | T-001..T-005 | READY |
| spec | required | .specdriven/specs/SPEC-001-multi-provider.md | hash no intake manifest | READY |
| tickets | optional | .specdriven/tickets/T-001..T-005.md | hash no intake manifest | READY |

## Contrato
- Classificacao: NO_API — o app consome APIs de terceiros como cliente; nenhum contrato compartilhado entre consumers configurados muda.
- Decisao estruturada do planner + rationale: provider adapters são internos ao repo `windows`; protocolo hook Claude Code inalterado; contract gate = N/A.
- Arquivos alterados por repositorio (exatos): ver seção Plano.
- Contrato USER_PROVIDED: N/A (proibido em NO_API)
- Operações e fontes: `chat_send` roteia por provider ativo; OAuth via authorize+token endpoints públicos dos CLIs oficiais (PKCE+loopback).
- Transporte/entrada/saida/erros: HTTPS via reqwest (existente); erros propagam mensagem do provider; OAuth cancelável/timeout.
- Auth/permissao/tenant/efeitos: credenciais só no Credential Manager via whitelist `provider-<id>-*`; custom base URL em settings.json (não é secret).
- Consumidores declarados em `profile.consumers`: `windows`
- Confirmações e bloqueios: OAuth subscription aceito pelo usuário ciente do ToS (ADR-0002); observação de outros CLIs fora de escopo.

## Paridade
N/A justificado — `parity.required: false`; divergências macOS/Windows são intencionais (windows/README.md).

## Plano e critérios
| Passo | Owner/arquivo | Mudanca | Critério verificável |
|---|---|---|---|
| 1 | `src-tauri/src/providers/mod.rs` (novo) | Contrato Provider + registry + dispatch por `activeProvider` | teste: resolve ativo, fallback claude, id desconhecido → erro |
| 2 | `src-tauri/src/providers/anthropic.rs` (de `claude.rs`) | Adapter Claude (apikey+oauth bearer); conteúdo file/window→blocks | teste: serialização de contexto/arquivo |
| 3 | `src-tauri/src/providers/openai.rs` (novo) | Adapter OpenAI-compatible → codex/hermes/custom | teste: serialização + auth header |
| 4 | `src-tauri/src/providers/google.rs` (novo) | Adapter Gemini (generateContent, bearer OAuth) | teste: payload contents/parts |
| 5 | `src-tauri/src/providers/oauth.rs` (novo) | PKCE S256, loopback listener, exchange+refresh | teste: challenge vector, parse redirect, token JSON |
| 6 | `src-tauri/src/secrets.rs` | whitelist `provider-<id>-{key,oauth}` | teste: aceita namespaced, rejeita desconhecido |
| 7 | `src-tauri/src/settings.rs` | `activeProvider`, `providerModels`, `customBaseUrl` | teste: defaults migram de `model` legado |
| 8 | `src-tauri/src/lib.rs` | comandos `providers_list`, `provider_set_active`, `oauth_start`, `provider_disconnect`; `chat_send` roteado | integra via cargo check/test |
| 9 | `src/core/bridge.ts`, `src/core/state.ts`, `src/settings/main.ts` | UI Providers section (status, key field, sign-in, active select) | tsc + playwright-tester no `npm run dev` |
| 10 | `windows/vite.config.ts` | `server.watch.ignored` target/+src-tauri/ (SPEC-002/T-005) | `npm run tauri dev` sem EBUSY |

## TDD seams
| Seam | Repo | driverRunId | Evidência test-first (commit/fingerprint) |
|---|---|---|---|
| `src-tauri/src/providers/mod.rs` (registry/seleção/modelo) | windows | single-agent | testes em `mod tests` cobrindo registry/fallback/secret-names/model-resolution; rodados antes do green final |
| `src-tauri/src/providers/openai.rs` (serialização, SSE, JWT) | windows | single-agent | 5 testes (chat shape, file degrade, SSE completed/delta, JWT claim) — 1 red real (JSON pointer com `/`) corrigido antes do green |
| `src-tauri/src/providers/oauth.rs` (PKCE/redirect/token) | windows | single-agent | RFC7636 appendix-B vector, loopback listener real via TcpStream, state mismatch, expiry fail-closed |
| `src-tauri/src/secrets.rs` (whitelist provider-*) | windows | single-agent | `provider_secret_names_pass_the_whitelist` cobre todos os ids + rejeições |

## Resultado da execução
- `cargo test --workspace`: **32 PASS** (29 lib + 3 hook), incluindo todos os seams acima.
- `cargo clippy --workspace --all-targets`: PASS com os 3 warnings pré-existentes (island.rs) — nenhum novo.
- `npx tsc --noEmit`: PASS.
- `npm run build`: PASS (vite + cargo check).
- Novos comandos: `providers_list`, `provider_set_active`, `provider_oauth_begin`, `provider_oauth_finish`, `provider_disconnect`.
- Novos módulos: `providers/{mod,anthropic,openai,google,oauth}.rs`; `claude.rs` removido (folded em anthropic.rs).
- Settings UI: seção Providers substitui a antiga seção "Claude" (radio ativo, key field, Sign in OAuth + aviso ToS, Disconnect, base URL/modelo custom).
- Limitação honesta: fluxo OAuth live (browser→callback→exchange) não é verificável sem contas reais — validado unitariamente (PKCE, loopback, parse); live verification fica para o usuário.

## Worktrees
- Uso de worktrees aprovado pelo usuario: false
- Workspace root irmao `coucou-worktrees`: C:/Users/Raffa/Documents/code/coucou-worktrees
- Base branch/HEAD: feat/multi-provider
- Worktrees/branches/owners: nenhum (sequencial)

## Gates
| Gate/papel | Repo/agente | Revisao coberta | Comando/evidencia | Status |
|---|---|---|---|---|
| lint | windows | — | cargo clippy --workspace --all-targets | pendente |
| typecheck | windows | — | npx tsc --noEmit | pendente |
| build | windows | — | npm run build + cargo check --workspace --all-targets | pendente |
| unit | windows | — | cargo test --workspace | pendente |
| integration/e2e/coverage | windows | — | N/A (sem infra) | N/A |
| contract | — | — | N/A (NO_API) | N/A |
| reviewer/tester/security-checker/project-memory | papéis | diff final + decisões | agentRunId no manifesto | pendente |
| playwright-tester | UI settings/island | jornada providers | sobre `npm run dev` | pendente |

## Memória do projeto
- Classificação: NEW_MEMORY (feature nova) — escrita após mudanças duráveis; sem drift (CONTEXT.md atualizado no intake).
- Bloqueios: nenhum.

## Agregação
- Manifest de readiness validado: pendente
- Security-checker PASS com fingerprints before/after iguais ao estado final: pendente
- Estado final: pendente
- READY_FOR_MERGE: false
- Commit/merge/deploy autorizado separadamente: false
