# ADR-0001: Perfil SpecDriven — gates locais só no app Windows, paridade desligada, upstream rigoroso

- Data: 2026-10-01
- Estado: aceito
- Origem: setup (entrevista do setup-facilitator)

## Contexto

O repositório `coucou` é um monorepo com dois produtos: `NotchBuddy/` (macOS, Swift/XcodeGen) e `windows/` (Tauri 2, front-end TypeScript sem framework + backend Rust + relé `coucou-hook`). A máquina de desenvolvimento atual é Windows: `xcodegen`/`xcodebuild`/`swift` não existem localmente e o app macOS só é construído no CI (`build.yml`, `macos-latest`). No app Windows existem 11 testes Rust, `tsc --noEmit` e `npm run build` verdes, clippy com 3 warnings e `cargo fmt --check` com diffs preexistentes; não há cobertura (sem tarpaulin/vitest), nem E2E, nem linter JS. O README do Windows documenta divergências intencionais em relação ao Mac.

## Decisão

- `repositories[]` declara apenas `windows` como repositório canônico com gates locais. `NotchBuddy/` fica fora dos gates locais; sua prova de build é o CI macOS.
- `profile.consumers = ["windows"]`; `parity.required = false` — as duas implementações divergem deliberadamente.
- `stages`: `triage: required`, `spec: required`, `tickets: optional`, `wayfind: off`. Tracker: GitHub Issues via `gh` em `Louis-CFM/coucou`.
- `tdd.mode = seams`.
- Gates `windows`: lint = `cargo clippy --workspace --all-targets`; typecheck = `npx tsc --noEmit`; build = `npm run build` + `cargo check --workspace --all-targets`; unit = `cargo test --workspace`; integration/e2e/coverage = N/A por ausência real de infraestrutura; contract sem comandos (`API_REQUIRED` ⇒ `MISSING_CAPABILITY` até existir runner).
- `surfaces`: `web: true` (a island roda em browser via `npm run dev`), `mobile: false`, `e2e: false`.
- Worktrees paralelos em `C:/Users/Raffa/Documents/code/coucou-worktrees`, sempre com aprovação explícita.
- AgentMemory já configurado em `.specdriven/agentmemory.json` (`projectId: coucou`, `127.0.0.1:3111`, launcher MCP SpecDriven com 54 tools).

## Alternativas consideradas

- **NotchBuddy como repositório canônico com gates**: rejeitado — `xcodebuild` ausente tornaria `doctor`/`run-gates` permanentemente `MISSING_CAPABILITY` nesta máquina.
- **NotchBuddy com gates todos N/A**: rejeitado — o usuário preferiu explicitamente "só windows" para não dar aparência de verificação onde não há.
- **Paridade obrigatória macOS↔Windows**: rejeitada — divergências são intencionais e documentadas; exigir decisão de paridade em toda mudança seria cerimônia sem valor.
- **coverage/lint estritos (`-D warnings`, `fmt --check`, tarpaulin)**: adiado — endurecer gates exige antes corrigir warnings/diffs ou instalar ferramentas; fica como decisão pendente, registrada no bloco `specdriven` do CLAUDE.md.

## Consequências

- Mudanças em `NotchBuddy/` não têm gate local — o plano deve declarar isso e depender de CI + revisão.
- Toda mudança passa por disposição de triage no GitHub Issues e exige `SPEC-*.md` antes do planner.
- Qualquer mudança classificada `API_REQUIRED` bloqueia até existir um contract runner configurado — não contornar reclassificando como `NO_API`.
- Reexecução do setup é merge: o bloco `specdriven:begin/end` do CLAUDE.md pode ser reescrito; o resto do arquivo é intocado.
