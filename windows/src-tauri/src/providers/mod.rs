// Provider registry and the seam every AI call goes through.
//
// The island asks for one chat turn; this module picks the active provider,
// resolves its credential (OAuth tokens or API key, all living in the Windows
// Credential Manager) and lets the right adapter speak its own wire format.
// Adapters share no code — adding a provider means adding a module here.

pub mod anthropic;
pub mod google;
pub mod oauth;
pub mod openai;

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::secrets;
use crate::settings::Settings;

// ── Normalized chat history ───────────────────────────────────────────────────

/// One side of the conversation. `parts` keep the original order so an
/// assistant turn can hold provider-native blocks (e.g. Claude tool_use) next
/// to the text the island actually shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub role: Role,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Text(String),
    /// Attached file content, base64-encoded; each adapter encodes it the way
    /// its API wants (document/image block, inline text, or skipped).
    File { media_type: String, data_b64: String },
    /// A provider-native block an adapter left in the history verbatim — only
    /// the adapter that produced it understands it; others must skip it.
    Native(Value),
}

/// Multi-turn history shared by every adapter. A turn is committed only after
/// the provider answered, so a failed call never leaves a dangling user turn.
#[derive(Default)]
pub struct Chat {
    turns: Mutex<Vec<Turn>>,
}

impl Chat {
    pub fn reset(&self) {
        self.turns.lock().unwrap().clear();
    }

    pub fn is_empty(&self) -> bool {
        self.turns.lock().unwrap().is_empty()
    }

    /// Locks aren't held across `.await`: adapters send a snapshot.
    pub fn snapshot(&self) -> Vec<Turn> {
        self.turns.lock().unwrap().clone()
    }

    /// Commit a completed exchange — skipped entirely when the call failed.
    pub fn commit(&self, user: Turn, assistant: Turn) {
        let mut turns = self.turns.lock().unwrap();
        turns.push(user);
        turns.push(assistant);
    }
}

// ── Public chat surface ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatContext {
    File { name: String, path: String },
    Window { app_name: String, title: String, url: Option<String> },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatReply {
    pub text: String,
}

pub const SYSTEM_PROMPT: &str = "You are Mochi, a personal AI assistant living at the top of the user's screen. \
You can help with absolutely anything — research, coding, finding places, recommendations, tasks, questions. \
Respond in the user's language. Be thorough and complete — use as much detail as the task requires. \
No markdown formatting (no **, no ##, no bullet dashes). Use plain text with line breaks.";

/// Text and code files are inlined; anything larger is skipped, as on macOS.
pub const MAX_INLINE_TEXT: u64 = 200_000;

/// Reads `path` and classifies it for the adapters: known binary formats stay
/// base64 `File` parts, small text files are inlined, the rest is skipped.
pub fn file_part(path: &str) -> Option<Part> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let media_type = match ext.as_str() {
        "pdf" => Some("application/pdf"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    };

    if let Some(media) = media_type {
        let bytes = std::fs::read(path).ok()?;
        return Some(Part::File {
            media_type: media.to_string(),
            data_b64: base64(&bytes),
        });
    }

    let len = std::fs::metadata(path).ok()?.len();
    if len > MAX_INLINE_TEXT {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    Some(Part::Text(format!("File contents:\n{text}")))
}

/// Window context — a plain text part every adapter understands.
pub fn window_part(app_name: &str, title: &str, url: Option<&str>) -> Part {
    let mut text = format!("Context — App: {app_name}, Window: {title}");
    if let Some(url) = url {
        text.push_str(&format!(", URL: {url}"));
    }
    Part::Text(text)
}

/// One chat turn routed through the active provider. Context rides the first
/// message only, exactly like ClaudeService.chat() on macOS.
pub async fn send(
    chat: &Chat,
    settings: &Settings,
    query: String,
    context: Option<ChatContext>,
) -> Result<ChatReply, String> {
    let spec = spec_of(settings.active_provider.as_str());
    let model = model_for(spec, settings);
    let credential = resolve_credential(spec, settings).await?;

    let mut parts: Vec<Part> = Vec::new();
    if chat.is_empty() {
        match &context {
            Some(ChatContext::File { name, path }) => {
                if let Some(part) = file_part(path) {
                    parts.push(part);
                }
                parts.push(Part::Text(format!("File: {name}")));
            }
            Some(ChatContext::Window { app_name, title, url }) => {
                parts.push(window_part(app_name, title, url.as_deref()));
            }
            None => {}
        }
    }
    parts.push(Part::Text(query));

    let mut history = chat.snapshot();
    let user = Turn { role: Role::User, parts };
    history.push(user.clone());

    let assistant = match dispatch(spec, &credential, &model, &history).await {
        Ok(turn) => turn,
        Err(err) => return Err(err),
    };
    chat.commit(user, assistant.clone());

    let text = assistant
        .parts
        .iter()
        .filter_map(|p| match p {
            Part::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if text.is_empty() {
        return Err("No response text.".into());
    }
    Ok(ChatReply { text })
}

async fn dispatch(
    spec: &ProviderSpec,
    credential: &Credential,
    model: &str,
    history: &[Turn],
) -> Result<Turn, String> {
    match spec.id {
        "claude" => anthropic::send(credential, model, history).await,
        "codex" => openai::send_codex(credential, model, history).await,
        "google" => google::send(credential, model, history).await,
        "hermes" | "custom" => openai::send_compatible(spec, credential, model, history).await,
        _ => Err(format!("No adapter for provider {}", spec.id)),
    }
}

// ── Credentials ───────────────────────────────────────────────────────────────

/// What an adapter may send. Never leaves the Rust side.
#[derive(Debug, Clone)]
pub enum Credential {
    ApiKey(String),
    OAuth(OAuthTokens),
}

/// OAuth tokens as stored in the Credential Manager (JSON blob).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthTokens {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Unix seconds when `access_token` stops working; None = unknown.
    #[serde(default)]
    pub expires_at: Option<i64>,
    /// Codex's `chatgpt_account_id` claim — pulled out of the id_token at
    /// exchange time so the bulky JWT never has to persist (Windows Credential
    /// Manager blobs cap at ~2560 bytes and a Codex bundle overflows that).
    #[serde(default)]
    pub account_id: Option<String>,
    /// Retained for flows that need it; Codex drops it after extracting
    /// `account_id`.
    #[serde(default)]
    pub id_token: Option<String>,
}

/// Secret name for a provider's API key: `provider-claude-key`, …
pub fn key_secret(id: &str) -> String {
    format!("provider-{id}-key")
}

/// Secret name for a provider's token bundle: `provider-claude-oauth`, …
pub fn oauth_secret(id: &str) -> String {
    format!("provider-{id}-oauth")
}

/// OAuth wins over an API key for the same provider — an explicit sign-in is
/// the fresher intent. Claude keeps `anthropic-api-key` as a last fallback so
/// an existing install keeps working without touching settings.
pub async fn resolve_credential(spec: &'static ProviderSpec, settings: &Settings) -> Result<Credential, String> {
    if spec.oauth.is_some() {
        if let Some(raw) = secrets::get(&oauth_secret(spec.id)) {
            let tokens: OAuthTokens = serde_json::from_str(&raw)
                .map_err(|_| format!("Stored {} sign-in is corrupt — sign in again.", spec.name))?;
            let tokens = oauth::ensure_fresh(spec, tokens, settings).await?;
            return Ok(Credential::OAuth(tokens));
        }
    }
    if let Some(key) = secrets::get(&key_secret(spec.id)) {
        return Ok(Credential::ApiKey(key));
    }
    if spec.id == "claude" {
        if let Some(key) = secrets::get("anthropic-api-key") {
            return Ok(Credential::ApiKey(key));
        }
    }
    Err(format!(
        "{} isn't connected. Open Settings → Providers and add a key{}.",
        spec.name,
        if spec.oauth.is_some() { " or sign in" } else { "" }
    ))
}

/// Drops both credential kinds for `id` (Sign out / Disconnect).
pub fn disconnect(id: &str) {
    let _ = secrets::clear(&key_secret(id));
    let _ = secrets::clear(&oauth_secret(id));
}

/// Whether the provider has *some* usable credential — what the UI dots show.
/// Doesn't validate, just checks presence, same as `secret_present`.
pub fn connected(spec: &ProviderSpec) -> bool {
    secrets::present(&oauth_secret(spec.id))
        || secrets::present(&key_secret(spec.id))
        || (spec.id == "claude" && secrets::present("anthropic-api-key"))
}

// ── Live probe: "connected" means the API actually answers ───────────────────

/// What a `provider_probe` call reports. `state` is one of:
/// `none` (no credential), `connected` (API answered), `failed` (the API or the
/// credential store rejected it — show the error), `unverified` (credential
/// exists but the listing endpoint can't tell — e.g. custom without /models).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub state: &'static str,
    pub error: Option<String>,
    /// Live model list when the probe succeeded; bundled defaults otherwise.
    pub models: Vec<String>,
}

async fn dispatch_models(spec: &ProviderSpec, credential: &Credential) -> Result<Vec<String>, String> {
    match spec.id {
        "claude" => anthropic::list_models(credential).await,
        "google" => google::list_models(credential).await,
        _ => openai::list_models(spec, credential).await,
    }
}

/// Presence + a real call: the probe doubles as the dynamic models fetch so a
/// single request serves both the status dot and the dropdown.
pub async fn probe(spec: &'static ProviderSpec, settings: &Settings) -> ProbeResult {
    let defaults: Vec<String> = spec.models.iter().map(|m| m.to_string()).collect();
    let credential = match resolve_credential(spec, settings).await {
        Ok(c) => c,
        Err(err) => {
            return ProbeResult { state: "none", error: Some(err), models: defaults };
        }
    };
    match dispatch_models(spec, &credential).await {
        Ok(models) if !models.is_empty() => {
            ProbeResult { state: "connected", error: None, models }
        }
        Ok(_) => ProbeResult { state: "connected", error: None, models: defaults },
        Err(err) => {
            // Auth rejections are real negatives; anything else just means the
            // listing endpoint can't confirm — the credential may still chat.
            let rejected = err.starts_with("HTTP 401") || err.starts_with("HTTP 403");
            ProbeResult {
                state: if rejected { "failed" } else { "unverified" },
                error: Some(err),
                models: defaults,
            }
        }
    }
}

// ── Registry ─────────────────────────────────────────────────────────────────

pub struct OAuthSpec {
    pub authorize_url: &'static str,
    pub token_url: &'static str,
    /// Public OAuth client id. Empty means the registration is user-supplied:
    /// resolved from `client_id_env` or the provider's Settings fields.
    pub client_id: &'static str,
    /// Public client secret — only present for installed-app clients that
    /// ship one; empty for PKCE-only or user-supplied registrations.
    pub client_secret: &'static str,
    /// Env vars supplying client id/secret when `client_id` is empty; "" = none.
    /// Settings fields are the other source (see `Settings::oauth_client`).
    pub client_id_env: &'static str,
    pub client_secret_env: &'static str,
    pub scopes: &'static [&'static str],
    /// Loopback redirect path; `None` = manual code paste (no listener).
    pub callback_path: Option<&'static str>,
    /// Fixed port the provider's client_id is registered with; 0 = any free.
    pub callback_port: u16,
    /// Extra authorize params like `code=true` for Claude.
    pub extra_params: &'static [(&'static str, &'static str)],
}

pub struct ProviderSpec {
    pub id: &'static str,
    pub name: &'static str,
    /// Whether "Sign in with …" exists for this provider at all.
    pub oauth: Option<OAuthSpec>,
    pub key_placeholder: &'static str,
    pub default_model: &'static str,
    pub models: &'static [&'static str],
    /// Fixed base URL for OpenAI-compatible presets; "" = read from settings.
    pub base_url: &'static str,
}

// OAuth constants below are the public desktop/CLI client registrations each
// provider ships in its own official CLI — using them makes the consent screen
// say e.g. "Codex" instead of a third-party app. This is the subscription
// path the user accepted in ADR-0002; it may violate provider ToS.

/// Claude Code's OAuth client (manual code flow via console.anthropic.com).
const CLAUDE_OAUTH: OAuthSpec = OAuthSpec {
    authorize_url: "https://claude.ai/oauth/authorize",
    token_url: "https://console.anthropic.com/v1/oauth/token",
    client_id: "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
    client_secret: "",
    client_id_env: "",
    client_secret_env: "",
    scopes: &["org:create_api_key", "user:profile", "user:inference"],
    callback_path: None,
    callback_port: 0,
    extra_params: &[("code", "true")],
};

/// Codex CLI's client — redirect is pinned to 127.0.0.1:1455/auth/callback
/// (the exact URI codex-rs registers, IP literal not `localhost`). The
/// `originator` param and the connectors scopes are also codex-rs's: without
/// them the issued token lacks what the ChatGPT backend expects.
const CODEX_OAUTH: OAuthSpec = OAuthSpec {
    authorize_url: "https://auth.openai.com/oauth/authorize",
    token_url: "https://auth.openai.com/oauth/token",
    client_id: "app_EMoamEEZ73f0CkXaXp7hrann",
    client_secret: "",
    client_id_env: "",
    client_secret_env: "",
    scopes: &[
        "openid",
        "profile",
        "email",
        "offline_access",
        "api.connectors.read",
        "api.connectors.invoke",
    ],
    callback_path: Some("/auth/callback"),
    callback_port: 1455,
    extra_params: &[
        ("codex_cli_simplified_flow", "true"),
        ("id_token_add_organizations", "true"),
        ("originator", "codex_cli_rs"),
    ],
};

/// Google's installed-app OAuth registration is a public id/secret pair —
/// but GitHub push protection flags the literals anyway, so the values are
/// user-supplied: pasted into Settings → Providers (stored as plain settings,
/// they are not user credentials) or exported via env vars.
const GOOGLE_OAUTH: OAuthSpec = OAuthSpec {
    authorize_url: "https://accounts.google.com/o/oauth2/v2/auth",
    token_url: "https://oauth2.googleapis.com/token",
    client_id: "",
    client_secret: "",
    client_id_env: "COUCOU_GOOGLE_CLIENT_ID",
    client_secret_env: "COUCOU_GOOGLE_CLIENT_SECRET",
    scopes: &[
        "https://www.googleapis.com/auth/cloud-platform",
        "https://www.googleapis.com/auth/userinfo.email",
        "https://www.googleapis.com/auth/userinfo.profile",
    ],
    callback_path: Some("/oauth/callback"),
    callback_port: 0,
    extra_params: &[("access_type", "offline"), ("prompt", "consent")],
};

pub const PROVIDERS: &[ProviderSpec] = &[
    ProviderSpec {
        id: "claude",
        name: "Claude",
        oauth: Some(CLAUDE_OAUTH),
        key_placeholder: "sk-ant-…",
        default_model: "claude-opus-5",
        models: &["claude-opus-5", "claude-sonnet-5", "claude-haiku-4-5"],
        base_url: "",
    },
    ProviderSpec {
        id: "codex",
        name: "Codex",
        oauth: Some(CODEX_OAUTH),
        key_placeholder: "sk-…",
        default_model: "gpt-5-codex",
        models: &["gpt-5-codex", "gpt-5", "codex-mini-latest"],
        base_url: "https://api.openai.com/v1",
    },
    ProviderSpec {
        id: "google",
        name: "Google (Gemini)",
        oauth: Some(GOOGLE_OAUTH),
        key_placeholder: "AIza…",
        default_model: "gemini-2.5-pro",
        models: &["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.5-flash-lite"],
        base_url: "https://generativelanguage.googleapis.com/v1beta",
    },
    ProviderSpec {
        id: "hermes",
        name: "Hermes",
        oauth: None,
        key_placeholder: "API key",
        default_model: "hermes-4-70b",
        models: &["hermes-4-70b", "hermes-4-405b"],
        base_url: "https://inference-api.nousresearch.com/v1",
    },
    ProviderSpec {
        id: "custom",
        name: "Custom (OpenAI-compatible)",
        oauth: None,
        key_placeholder: "API key",
        default_model: "",
        models: &[],
        base_url: "",
    },
];

pub fn spec_of(id: &str) -> &'static ProviderSpec {
    PROVIDERS.iter().find(|p| p.id == id).unwrap_or(&PROVIDERS[0])
}

/// The model for `spec`: `providerModels[id]` first; Claude falls back to the
/// legacy `settings.model` so an old settings.json keeps its choice; then the
/// provider default.
pub fn model_for(spec: &ProviderSpec, settings: &Settings) -> String {
    if let Some(model) = settings.provider_models.get(spec.id) {
        if !model.is_empty() {
            return model.clone();
        }
    }
    if spec.id == "claude" && !settings.model.is_empty() {
        return settings.model.clone();
    }
    spec.default_model.to_string()
}

/// Small standalone base64 encoder — not worth pulling another crate's API
/// shape in for two call sites. Also used for Stripe's basic auth.
pub(crate) fn base64_for(bytes: &[u8]) -> String {
    base64(bytes)
}

pub(crate) fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn registry_covers_every_declared_provider() {
        let ids: Vec<_> = PROVIDERS.iter().map(|p| p.id).collect();
        assert_eq!(ids, ["claude", "codex", "google", "hermes", "custom"]);
        // Sign-in exists exactly where the spec says so.
        assert!(spec_of("claude").oauth.is_some());
        assert!(spec_of("codex").oauth.is_some());
        assert!(spec_of("google").oauth.is_some());
        assert!(spec_of("hermes").oauth.is_none());
        assert!(spec_of("custom").oauth.is_none());
    }

    #[test]
    fn unknown_provider_falls_back_to_claude() {
        assert_eq!(spec_of("bogus").id, "claude");
        assert_eq!(spec_of("").id, "claude");
    }

    #[test]
    fn secret_names_are_namespaced() {
        assert_eq!(key_secret("claude"), "provider-claude-key");
        assert_eq!(oauth_secret("google"), "provider-google-oauth");
    }

    #[test]
    fn provider_secret_names_pass_the_whitelist() {
        for spec in PROVIDERS {
            assert!(secrets::allowed(&key_secret(spec.id)), "{} key", spec.id);
            assert!(secrets::allowed(&oauth_secret(spec.id)), "{} oauth", spec.id);
        }
        assert!(!secrets::allowed("provider-evil-key"));
        assert!(!secrets::allowed("provider-claude-other"));
        assert!(secrets::allowed("anthropic-api-key"));
    }

    #[test]
    fn model_resolution_prefers_map_then_legacy_then_default() {
        let mut settings = Settings { model: "claude-sonnet-5".into(), ..Default::default() };
        // legacy field still drives claude when no override exists
        assert_eq!(model_for(spec_of("claude"), &settings), "claude-sonnet-5");
        // map wins
        settings.provider_models.insert("claude".into(), "claude-haiku-4-5".into());
        assert_eq!(model_for(spec_of("claude"), &settings), "claude-haiku-4-5");
        // other providers fall back to their default
        assert_eq!(model_for(spec_of("codex"), &settings), "gpt-5-codex");
    }

    #[test]
    fn chat_commits_only_completed_turns() {
        let chat = Chat::default();
        assert!(chat.is_empty());
        chat.commit(
            Turn { role: Role::User, parts: vec![Part::Text("hi".into())] },
            Turn { role: Role::Assistant, parts: vec![Part::Text("hello".into())] },
        );
        let snap = chat.snapshot();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[1].role, Role::Assistant);
        chat.reset();
        assert!(chat.is_empty());
    }
}
