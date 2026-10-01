# SPEC-005: Verdade de conexão — callback neutro, exchange honesta, connected = probe real

- Data: 2026-10-01
- Estado: aprovado
- Origem: bug reportado pelo usuário (Codex OAuth diz "connected" na página mas Settings/chat dizem "not connected"; vale pra todo provider)
- Tracker ref: `.specdriven/tickets/T-008.md`

## Problema

Três mentiras de UI sobre conexão:

1. `wait_for_code` responde **"Signed in — you can close this tab"** assim que recebe `code`+`state`, antes da troca de token acontecer (`providers/oauth.rs:181`). Se a exchange falha, o browser já declarou vitória.
2. O erro real de `exchange` só sai via evento `provider-oauth-complete`; a página e o card não mostram nada → `connected` permanece false sem explicação.
3. `providers::connected()` só testa **presença** de secret no Credential Manager — credencial presente mas inválida/expirada indica "connected" falso-positivo; credencial que falhou na exchange indica ausência sem dizer por quê.

Bug observado (Codex): a página disse "Signed in", Settings ficou "not connected", chat respondeu "provider codex isn't connected" — ou seja, a exchange falhou e o erro foi engolido. Suspeita: o token endpoint do Codex (`auth.openai.com/oauth/token`) espera JSON, e `token_post` manda form-urlencoded — confirmar contra codex-cli no planning.

## Objetivo e sucesso

- Callback loopback responde mensagem **neutra** ("Code received — finish in Coucou"); o resultado real sai no card via `provider-oauth-complete`.
- Falha de exchange aparece **em destaque no card do provider** (mensagem do evento), não só em stderr.
- `ProviderInfo.connected` passa a refletir um **probe real**: credencial presente + chamada mínima à API (reusa `list_models` do SPEC-003 — mesma request, dois usos).
- Probe falho mostra o erro real no card (ex.: `401`, `network`, `endpoint missing /models`) — não "not connected" genérico.
- Codex OAuth completa ponta a ponta: `provider-codex-oauth` gravado, probe passa, chat responde.
- O mesmo vale para todo provider (OAuth ou API key) — comportamento uniforme.

## Escopo

### Dentro

- `providers/oauth.rs`: texto neutro no `respond` de sucesso de captura (o "Signed in" de verdade acontece no card via evento).
- `providers::connected` → probe: `ProviderInfo` ganha `probeStatus`/`probeError` derivados de uma chamada real por provider com credencial (async, em paralelo, cache de sessão de Settings).
- `token_post` por provider quando o endpoint exige formato específico (codex JSON).
- Chat: quando `resolve_credential` encontra bundle mas o adapter falha, a mensagem de erro carrega a causa real do adapter — não só "not connected".
- UI: estado `checking` no status dot enquanto o probe corre; erro clicável/visível.

### Fora

- Retry automático com backoff do probe; validação contínua em background (probe é ao abrir Settings / on-demand no chat, não polling).
- macOS.

## Consumidores e paridade

- `windows` apenas. Paridade N/A.

## Contrato (somente API_REQUIRED)

- N/A (`NO_API`).

## Riscos e perguntas fechadas

- **Probe por provider com credencial a cada abertura de Settings**: aceito no intake ("test real na API"); latência mitigada por execução paralela e estado `checking`.
- **Sem credencial** → `connected=false` sem probe (zero rede).
- **OAuth expirado** → probe usa `resolve_credential`/`ensure_fresh` — refresh válido conta como conectado.
- **Custom provider sem endpoint `/models`** → probe falha honesto; card oferece hint "endpoint pode não listar modelos — tente mandar mensagem".

## Arquivos prováveis

- `windows/src-tauri/src/providers/{mod,oauth,anthropic,openai,google}.rs`, `lib.rs`, `src/settings/main.ts`, `src/core/bridge.ts`
