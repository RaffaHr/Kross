---
id: T-007
title: Hooks por provider ativo — seção no card, expande ao conectar
status: todo
blockedBy: []
specRef: .specdriven/specs/SPEC-004-provider-hooks.md
trackerRef: .specdriven/tickets/T-007-provider-hooks.md
---

# T-007: Hooks por provider ativo

## Entrega (tracer bullet)

Cada provider cuja CLI suporta hooks (Claude `settings.json`, Codex `config.toml`, Gemini `settings.json`) ganha uma seção de hooks no próprio card em Settings → Providers. Ao conectar com sucesso o card expande e mostra a configuração. Providers sem CLI com hooks (hermes, custom) não mostram a seção.

## Critérios de aceite

- [ ] Seção de hooks dentro do card do provider, visível/expandida ao conectar com sucesso
- [ ] Instalação escreve no config file daquela CLI com a mesma disciplina do Claude: backup datado + merge + diff + confirmação
- [ ] Hook instalado encaminha eventos pro `coucou-hook` com o id do provider
- [ ] CLI ausente ou sem suporte a hooks → seção não aparece (honesto, não disabled sem explicação)
- [ ] Claude mantém comportamento atual (regressão zero)

## Seams TDD

- `hooks` por provider: módulo por CLI com merge/backup testável (espelha o seam atual de `hooks.rs`).

## Notas

- Formatos de hook por CLI precisam de pesquisa no planning (codex hooks vs `notify`, gemini settings.json hooks).
- Eventos que a CLI não emite (ex.: permission request) são documentados honestamente na seção, não fingidos.
