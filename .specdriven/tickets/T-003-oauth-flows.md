---
id: T-003
title: OAuth por subscription — Claude, Codex/ChatGPT, Google (Antigravity)
status: open
blockedBy: [T-001]
specRef: .specdriven/specs/SPEC-001-multi-provider.md
trackerRef: .specdriven/tickets/T-003-oauth-flows.md
---

# T-003: OAuth por subscription — Claude, Codex/ChatGPT, Google (Antigravity)

## Entrega (tracer bullet)

"Sign in" abre o browser na URL de authorize (PKCE); listener loopback `127.0.0.1` recebe o code; backend troca por tokens e guarda access+refresh no Credential Manager. Provider OAuth faz chat com o token; refresh automático antes de expirar. Pelo menos um fluxo OAuth funciona ponta a ponta (Google sugerido como primeiro por ser o menos restritivo).

## Critérios de aceite

- [ ] Flow PKCE completo por provider OAuth (claude, codex, google)
- [ ] Access/refresh tokens só no Credential Manager; cancelar o browser não trava a UI
- [ ] Token expirado renova sem re-login; refresh inválido → estado "desconectado" + UI permite novo sign-in
- [ ] Adapter escolhe credencial OAuth quando presente, API key como alternativa
- [ ] Aviso de ToS visível ao lado de cada botão OAuth (ADR-0002)

## Seams TDD

- PKCE (verifier/challenge S256 contra vetores conhecidos), parsing do redirect (code/state/erro), seleção de credencial (oauth > apikey)

## Notas

- Client IDs/endpoints públicos dos CLIs oficiais são constantes por adapter — concentrar e comentar a fonte.
- Loopback porta efêmera; timeout do listener (~2 min) e cleanup garantido.
