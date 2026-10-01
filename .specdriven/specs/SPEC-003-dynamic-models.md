# SPEC-003: Lista de modelos dinâmica por provider

- Data: 2026-10-01
- Estado: aprovado
- Origem: usuário (intake — "lista de modelos dependendo do provider, dinâmica, não hardcode")
- Tracker ref: `.specdriven/tickets/T-006.md`

## Problema

`ProviderSpec.models` é uma constante embutida por provider. Quando o provider lança modelo novo, o app não oferece; quando o modelo embutido sai de linha, o dropdown oferece um inválido.

## Objetivo e sucesso

- Ao abrir Settings, providers **com credencial** fazem `list_models` na API real; o dropdown usa o resultado.
- Sem credencial, fetch falho ou endpoint recusando OAuth de subscription → fallback silencioso pros defaults embutidos (`ProviderSpec.models`), com hint discreto no card quando o fetch falhou.
- O modelo persistido em `providerModels` renderiza mesmo quando não consta da lista (append da opção).

## Escopo

### Dentro

- `providers::list_models(spec, credential) -> Result<Vec<String>, String>` por adapter:
  - `claude`: `GET {anthropic}/v1/models` (x-api-key ou bearer OAuth)
  - `codex`/`hermes`/`custom`: `GET {base_url}/models` (OpenAI-compatible)
  - `google`: `GET {base}/models` (generateLanguage, bearer)
- `list_models` é compartilhada com o probe de conexão (SPEC-005): uma chamada só serve os dois.
- `providers_list` (ou comando novo `provider_models`) executa o fetch async; UI usa retorno ou defaults.
- Cache em memória por sessão do Settings — fetch **ao abrir** a janela, não a cada render.

### Fora

- Persistir a lista em settings.json (derivável; settings fica como fonte só de overrides).
- Modelos de outras famílias na mesma tela; edição manual da lista.
- macOS (`windows/` apenas).

## Consumidores e paridade

- `windows` apenas. Paridade N/A.

## Contrato (somente API_REQUIRED)

- N/A (`NO_API`) — chamadas outbound a APIs de terceiros, nenhum contrato entre consumers configurados muda.

## Riscos e perguntas fechadas

- **OAuth recusando `/models`** (codex/google): tratado como fetch falho → fallback silencioso (decisão do intake).
- **Custom sem `/models`**: muitos endpoints OpenAI-compatible não implementam listagem — erro vira fallback.
- **Latência ao abrir Settings**: fetch async por provider em paralelo; UI mostra defaults até a resposta chegar (não bloqueia a janela).

## Arquivos prováveis

- `windows/src-tauri/src/providers/{mod,anthropic,openai,google}.rs`, `lib.rs` (comando), `src/core/bridge.ts`, `src/settings/main.ts`
