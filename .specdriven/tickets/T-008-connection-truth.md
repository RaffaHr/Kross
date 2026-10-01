---
id: T-008
title: Verdade de conexão — callback neutro, erro de exchange visível, connected = probe real
status: done
blockedBy: []
specRef: .specdriven/specs/SPEC-005-connection-truth.md
trackerRef: .specdriven/tickets/T-008-connection-truth.md
---

# T-008: Verdade de conexão

## Entrega (tracer bullet)

A página de callback para de dizer "Signed in" antes da troca de token ("Code received — finishing in Coucou"); falha de exchange aparece em destaque no card do provider; `connected` deixa de ser presença de secret e vira um probe real na API do provider (reusa a chamada de modelos do T-006).

## Critérios de aceite

- [ ] Callback page nunca afirma sucesso antes da exchange terminar
- [ ] Falha de exchange é visível no card (erro do evento `provider-oauth-complete` em destaque)
- [ ] `connected` reflete probe real: credencial existe E a API responde
- [ ] Probe falho mostra o erro real (401, network, etc.) — não "not connected" genérico
- [ ] Bug do Codex OAuth corrigido: exchange completa e o chat funciona

## Seams TDD

- `provider_probe(spec, credential)` — adapter ping real; mod.rs testa mapeamento erro→status.
- Corpo da exchange por provider (codex JSON vs form) coberto por teste de shape.

## Notas

- Causa raiz suspeita do bug Codex: token endpoint pode exigir JSON em vez de form-urlencoded — confirmar contra codex-cli no planning.
