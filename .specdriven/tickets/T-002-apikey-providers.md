---
id: T-002
title: Adapters por API key — Claude, Codex/OpenAI, Hermes preset, custom
status: done
blockedBy: [T-001]
specRef: .specdriven/specs/SPEC-001-multi-provider.md
trackerRef: .specdriven/tickets/T-002-apikey-providers.md
---

# T-002: Adapters por API key — Claude, Codex/OpenAI, Hermes preset, custom

## Entrega (tracer bullet)

Chat da island funciona com um provider não-Claude por API key: adapter `openai-compatible` (base URL + key, formato chat completions) instanciado como `codex` (endpoint OpenAI fixo), `hermes` (preset de base URL documentada) e `custom` (URL livre em settings.json). Adapter `claude` cobre API key Anthropic.

## Critérios de aceite

- [x] Mensagem enviada com provider `codex` ou `custom` ativo retorna resposta (contexto de arquivo/janela mapeado ao formato do provider)
- [x] Erros de auth propagam a mensagem do provider (como `call()` faz hoje)
- [x] `custom` exige base URL; sem URL → erro claro, sem request
- [x] Gates verdes (`cargo clippy`, `tsc`, `npm run build`+`cargo check`, `cargo test`)

## Seams TDD

- Serialização de mensagens/contexto por adapter (Anthropic blocks vs OpenAI parts — casos: texto, imagem png, pdf, arquivo >200KB)

## Notas

- Sem OAuth neste ticket — T-003.
