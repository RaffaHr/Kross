---
id: T-001
title: Provider abstraction + plumbing de secrets/settings
status: done
blockedBy: []
specRef: .specdriven/specs/SPEC-001-multi-provider.md
trackerRef: .specdriven/tickets/T-001-provider-abstraction.md
---

# T-001: Provider abstraction + plumbing de secrets/settings

## Entrega (tracer bullet)

`src-tauri/src/providers/` existe com o contrato comum (identidade do provider, métodos de auth suportados, envio de mensagem multi-turn com contexto de arquivo/janela), um registry, e `provider ativo` persistido em `settings.json`. `claude.rs` é reorganizado como o primeiro adapter sem mudança de comportamento — o chat atual continua funcionando pelo caminho novo.

## Critérios de aceite

- [x] `providers/mod.rs` define o contrato; nenhum adapter importa outro adapter
- [x] `secrets::KNOWN_KEYS` aceita chaves namespaced por provider (`provider-<id>-*`)
- [x] `settings.json` persiste `activeProvider`; ausente → `claude` (migração silenciosa)
- [x] `anthropic-api-key` legado continua valendo como credencial do adapter Claude
- [x] `cargo test --workspace` + `npx tsc --noEmit` verdes

## Seams TDD

- `providers/registry.rs` (resolve ativo, fallback), `providers/mod.rs` (normalização de contexto/arquivo por provider)

## Notas

- TDD mode `seams`: planner confirma os seams no plano; tdd-driver escreve red antes.
