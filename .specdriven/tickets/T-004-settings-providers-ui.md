---
id: T-004
title: Settings — seção Providers (lista, connect, provider ativo)
status: open
blockedBy: [T-001]
specRef: .specdriven/specs/SPEC-001-multi-provider.md
trackerRef: .specdriven/tickets/T-004-settings-providers-ui.md
---

# T-004: Settings — seção Providers (lista, connect, provider ativo)

## Entrega (tracer bullet)

A janela Settings lista os 5 providers com estado (conectado via key/OAuth ou não), ações por provider (salvar key, sign in OAuth, disconnect, base URL no custom) e o seletor de provider ativo. Nenhum segredo cruza o IPC — o front só pergunta "tem credencial?".

## Critérios de aceite

- [ ] Cada provider mostra método(s) de auth disponíveis e estado real lido do Credential Manager
- [ ] Disconnect remove a credencial (api key e/ou tokens OAuth) e reflete na UI
- [ ] Provider ativo persiste em `settings.json` e vale sem restart
- [ ] Aviso de ToS junto aos botões OAuth
- [ ] Visual consistente com o Settings existente (`src/settings/`, `settings.css`)

## Seams TDD

- Nenhum (UI) — cobertura vem do playwright-tester sobre `npm run dev` na verificação.

## Notas

- Depende de T-002/T-003 para ficar funcional de ponta a ponta, mas pode ser construída contra o contrato de T-001.
