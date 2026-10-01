---
id: T-005
title: vite.config — ignorar target/ e src-tauri/ no watch (EBUSY no tauri dev)
status: open
blockedBy: []
specRef: .specdriven/specs/SPEC-002-vite-watch-ignore.md
trackerRef: .specdriven/tickets/T-005-vite-watch-ignore.md
---

# T-005: vite.config — ignorar target/ e src-tauri/ no watch (EBUSY no tauri dev)

## Entrega (tracer bullet)

`server.watch.ignored` em `windows/vite.config.ts` cobre `**/target/**` e `**/src-tauri/**`.

## Critérios de aceite

- [ ] `npm run tauri dev` passa do primeiro build sem `EBUSY`
- [ ] Editar `src/**` ainda dispara hot-reload do front end

## Seams TDD

- Nenhum.

## Notas

- Independente da feature multi-provider; pode entrar primeiro por desbloquear o dev loop.
