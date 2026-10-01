# CONTEXT — Coucou

Modelo de domínio canônico. Termos aqui têm exatamente um significado; sinônimos casuais são corrigidos para o termo canônico.

## Glossário

| Termo | Significado | Não significa | Relações |
|---|---|---|---|
| Mochi | O personagem animado desenhado em código (Canvas) que habita a UI | Um asset de imagem, Rive ou Lottie | Vive na island; reage a eventos (peek, hearts, dizzy, box) |
| island | A superfície de UI que se expande/retrai (no notch no macOS; na borda superior no Windows) | Uma janela comum com taskbar | Contém Mochi, pills, views |
| NotchBuddy | O alvo/target macOS empacotado como `fr.louisraille.NotchBuddy` | O app do App Store (esse é CoucouAppStore) | Produto macOS em `NotchBuddy/` |
| CoucouAppStore | Segundo alvo macOS (`fr.louisraille.Coucou`) para distribuição App Store | O build Developer ID | Divide `Sources/` com NotchBuddy via XcodeGen |
| coucou (windows) | O app desktop Windows em `windows/` (Tauri 2: front TS + backend Rust) | O app macOS | Repositório canônico `windows` nos gates SpecDriven |
| hook / coucou-hook | Executável relé que a CLI do provider invoca a cada evento (`nb-hook` no Mac, `coucou-hook.exe` no Windows) — hoje Claude Code, a extensão por CLI é o T-007 | Um daemon de longa duração | Sai em ≤300 ms se a app não responde; nunca bloqueia a CLI chamadora |
| permission request | Pedido de aprovação de ferramenta vindo do Claude Code, mostrado na island com Deny/Allow | Auto-aprovação | Nunca aprovado sem clique explícito |
| pill | Indicador compacto de uma integração ao lado de Mochi | Uma view completa | Expandir abre a view da integração |
| session | Uma sessão de CLI de coding observada pelo app (hoje só Claude Code; Codex/Gemini dependem de T-007) | Uma conversa de chat | Sessões terminadas mostram resumo na island |
| integration | Fonte externa configurada pelo usuário (Cal.com etc.) com chave no Keychain/Credential Manager | Telemetria | Polled pelo backend; nunca sem configuração |
| settings.json | `~/.claude/settings.json`, onde os hooks do Claude Code vivem | Um arquivo que a app pode sobrescrever | Só merge com backup datado + diff + confirmação |
| provider | Um backend de IA que a island usa para chat e funções futuras (Claude, Codex, Hermes, Google, custom) | O sistema de hooks que observa sessões do Claude Code — esse é ingestão, não provider | Cada um tem um adapter em `src-tauri/src/providers/` |
| adapter | Módulo por provider que implementa auth (API key e/ou OAuth) e chat, sem acoplamento entre providers | Um serviço compartilhado com código de providers misturado | Isolado por arquivo; registry os reúne |
| OAuth subscription | Login via navegador (PKCE + loopback) usando a assinatura do usuário em vez de API key | Uma licença para uso oficial — vários provedores proíbem em apps terceiros (ADR-0002) | Tokens no Credential Manager, nunca em disco |
| custom provider | Endpoint OpenAI-compatible configurado pelo usuário (base URL + API key) | Qualquer formato de API | Reusa o adapter OpenAI-compatible |
| model list | Lista de modelos que um provider oferece, buscada na API do provider ao abrir Settings | Uma constante por provider no código | Fallback silencioso pros defaults embutidos quando o fetch falha (OAuth de subscription pode recusar listagem) |
| provider hook | Config de hook na CLI do provider ativo (Claude `~/.claude/settings.json`, Codex `config.toml`, Gemini `~/.gemini/settings.json`) que encaminha eventos pro `coucou-hook` | Um hook genérico igual pra toda CLI | Só existe onde a CLI suporta hooks; seção aparece/expande no card do provider ao conectar |
| connection probe | Chamada real à API do provider (listagem de modelos) para dizer se a credencial funciona | Presença do secret no Credential Manager | `connected` na UI só acende se o probe passa; falha mostra o erro real |

## Invariantes de negócio

- Nunca bloquear nem atrasar uma sessão do Claude Code — o hook sai imediatamente sem resposta da app (CLAUDE.md; ADR-0001).
- Secrets só no Keychain (macOS) / Windows Credential Manager; nunca em disco ou git (CLAUDE.md; ADR-0001).
- Sem telemetria; rede apenas para serviços configurados pelo usuário (CLAUDE.md).
- 0 % de CPU quando a island está oculta (CLAUDE.md).
- Nenhuma aprovação de permissão do Claude Code nem envio de email sem clique explícito (CLAUDE.md).
- O personagem é desenhado em código; sem Rive/Lottie/imagens (CLAUDE.md).
- O `.xcodeproj` é gerado por XcodeGen a partir de `project.yml`; nunca editado à mão (CLAUDE.md).
- Gates locais SpecDriven cobrem somente `windows/`; mudanças em `NotchBuddy/` provam-se no CI macOS (ADR-0001).

## Capacidades e fronteiras

- Faz: observar sessões do Claude Code, aprovar permissões, chat, drop de arquivo, integrações em pills, relé hook não-bloqueante.
- Não faz: telemetria, rede não configurada, auto-aprovação, bloqueio do terminal.
- Fronteiras externas: Claude Code (hooks + settings.json), Anthropic API (chat), Cal.com e demais integrações, macOS Keychain / Windows Credential Manager, GitHub (releases + tracker).
- A versão Windows diverge intencionalmente do macOS (sem notch, sem email, Cal.com em lista) — divergências válidas estão em `windows/README.md`.
