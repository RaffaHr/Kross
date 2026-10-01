# SPEC-002: `npm run tauri dev` quebra com EBUSY — Vite observando `target/`

- Data: 2026-10-01
- Estado: aprovado
- Origem: observado na sessão (stderr do `npm run tauri dev` do usuário)
- Tracker ref: `.specdriven/tickets/T-005.md`

## Problema

`npm run tauri dev` morre no primeiro build: o Vite tenta `fs.watch` em `windows/target/debug/deps/coucou_lib.dll` enquanto o Cargo ainda está escrevendo o arquivo → `EBUSY: resource busy or locked, watch`. `vite.config.ts` não declara `server.watch.ignored`, então o watcher cobre o `target/` do workspace Cargo.

## Objetivo e sucesso

- `npm run tauri dev` completa o primeiro build sem crash de watcher.
- O watcher do Vite ignora `target/` (e de preferência `src-tauri/` — rebuild Rust é responsabilidade do watcher do próprio Tauri).

## Escopo

### Dentro

- `windows/vite.config.ts`: `server.watch.ignored` cobrindo `**/target/**` e `**/src-tauri/**`.

### Fora

- Qualquer mudança de comportamento de build/hot-reload além do ignore.

## Consumidores e paridade

- `windows` apenas. Paridade N/A.

## Contrato (somente API_REQUIRED)

- N/A (`NO_API`).

## Riscos e perguntas fechadas

- Nenhum. Correção mecânica, alinhada à recomendação oficial do Tauri para Vite.

## Referências

- `windows/vite.config.ts`; stack trace EBUSY do terminal do usuário (2026-10-01)
