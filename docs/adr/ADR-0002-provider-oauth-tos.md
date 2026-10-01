# ADR-0002: OAuth por subscription em providers de IA apesar do risco de ToS

- Data: 2026-10-01
- Estado: aceito
- Origem: usuário (intake SpecDriven — escolha explícita após aviso)

## Contexto

O usuário pediu login "por API key ou OAuth com subscription" para Claude, Codex, Hermes, Antigravity (Google) e custom. Usar tokens OAuth de assinatura (Claude Pro/Max, ChatGPT/Codex, Gemini/Code Assist) fora dos produtos oficiais viola os Termos de Serviço de vários provedores — a Anthropic já baniu esse uso em apps terceiros; OpenAI e Google têm restrições análogas. O risco prático é suspenso de conta. O aviso foi apresentado no intake e o usuário escolheu "OAuth para todos mesmo assim".

## Decisão

- Implementar OAuth (PKCE + loopback) nos adapters `claude`, `codex` e `google`, ao lado de API key.
- Tokens OAuth (access/refresh) ficam no Windows Credential Manager como qualquer secret.
- A UI mostra um aviso de ToS junto a cada botão "Sign in": uso fora do produto oficial pode violar os termos do provedor e suspender a conta.
- `hermes` e `custom` ficam em API key (Hermes Agent é OpenAI-compatible; custom não tem OAuth genérico).

## Alternativas consideradas

- **API key apenas**: rejeitada pelo usuário — ele quer usar a subscription que já paga.
- **OAuth só onde explicitamente licenciado**: rejeitada — nenhum dos três licencia explicitamente para terceiros, então a opção seria equivalente a "sem OAuth".

## Consequências

- O risco de banimento é assumido pelo usuário, por escolha informada — não é um bug se acontecer.
- Constantes de OAuth (client IDs, endpoints, escopos) são copiadas dos CLIs oficiais e podem quebrar silenciosamente — adapters concentram essas constantes e a UI deve tratar falha de troca de token como "desconectado", não crash.
- **Exceção Google**: o par client id/secret do gemini-cli é público mas tripa o push protection do GitHub — não pode ser embutido no binário. O usuário informa o par em Settings → Providers (campos `googleClientId`/`googleClientSecret`, armazenados como settings comuns — não são credenciais de usuário) ou via env `COUCOU_GOOGLE_CLIENT_ID`/`COUCOU_GOOGLE_CLIENT_SECRET`. Sem o par configurado, o "Sign in" do Google fica desabilitado.
- Não registrar tokens em logs; `log.rs` nunca recebe valores de credencial.
