---
id: T-006
title: Lista de modelos dinâmica por provider (fetch ao abrir Settings)
status: done
blockedBy: []
specRef: .specdriven/specs/SPEC-003-dynamic-models.md
trackerRef: .specdriven/tickets/T-006-dynamic-models.md
---

# T-006: Lista de modelos dinâmica por provider

## Entrega (tracer bullet)

Ao abrir Settings, cada provider com credencial faz fetch da lista de modelos na API do provider; o dropdown usa o resultado. Sem credencial ou com falha no fetch → defaults embutidos, sem erro na UI (hint discreto).

## Critérios de aceite

- [ ] Dropdown de modelos reflete a API real do provider quando conectado
- [ ] Sem credencial → defaults embutidos, zero chamada de rede
- [ ] Fetch falho (OAuth recusado, offline) → defaults + hint discreto, nunca erro bloqueante
- [ ] Modelo persistido em `providerModels` continua válido mesmo fora da lista

## Seams TDD

- `providers::list_models(spec, credential)` — cada adapter devolve `Vec<String>` ou erro; mod.rs testa fallback e shape por adapter.

## Notas

- Compartilha a chamada com o probe de conexão (T-008) — uma requisição só.
