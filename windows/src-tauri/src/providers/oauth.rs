// OAuth plumbing shared by the sign-in providers — PKCE authorization-code
// flow with either a loopback listener (Codex, Google) or manual code paste
// (Claude, whose public client redirects to a hosted "copy this code" page).
//
// The client constants in mod.rs are the providers' own public CLI clients —
// see ADR-0002 for the accepted subscription/ToS trade-off.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

use super::{OAuthSpec, OAuthTokens, ProviderSpec};
use crate::settings::Settings;

/// How long "Sign in with …" waits for the browser before giving up.
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

// ── PKCE ─────────────────────────────────────────────────────────────────────

pub struct Pending {
    pub verifier: String,
    pub state: String,
    /// Exact redirect_uri the exchange must send back — differs per flow.
    pub redirect_uri: String,
}

fn random_urlsafe(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    getrandom::getrandom(&mut bytes).expect("OS random source unavailable");
    base64_url(&bytes)
}

fn base64_url(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn sha256_url(input: &str) -> String {
    use sha2::Digest;
    base64_url(&sha2::Sha256::digest(input.as_bytes()))
}

/// S256 challenge for a freshly generated verifier — RFC 7636.
fn pkce_pair() -> (String, String) {
    let verifier = random_urlsafe(32);
    let challenge = sha256_url(&verifier);
    (verifier, challenge)
}

// ── Flow start ───────────────────────────────────────────────────────────────

pub struct Begin {
    /// URL the UI opens in the user's browser.
    pub url: String,
    /// True when the user must paste a code back (Claude's hosted page).
    pub expects_paste: bool,
}

/// Everything a started sign-in leaves behind: the pending exchange state and,
/// for loopback providers, the receiver the callback lands on.
pub struct Started {
    pub begin: Begin,
    pub pending: Pending,
    pub receiver: Option<mpsc::Receiver<Result<String, String>>>,
}

/// Starts a sign-in for `spec`. For loopback providers this also spawns the
/// listener thread; the result arrives on `Started::receiver`.
pub fn begin(spec: &'static ProviderSpec, settings: &Settings) -> Result<Started, String> {
    let oauth = spec.oauth.as_ref().ok_or_else(|| format!("{} has no sign-in.", spec.name))?;
    // Fail before touching ports when the provider's client registration is
    // user-supplied and missing (e.g. Google without Settings/env values).
    let (client_id, _) = client_credentials(spec, settings)?;
    let (verifier, challenge) = pkce_pair();
    let state = random_urlsafe(16);

    match oauth.callback_path {
        Some(path) => {
            let (listener, port) = bind_listener(oauth.callback_port)?;
            // 127.0.0.1, not "localhost": Codex registers the IP literal, and
            // Google accepts loopback IPs interchangeably for installed apps.
            let redirect_uri = format!("http://127.0.0.1:{port}{path}");
            let url = authorize_url(oauth, &redirect_uri, &challenge, &state, &client_id);
            let expected_state = state.clone();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(wait_for_code(listener, path, &expected_state));
            });
            let pending = Pending { verifier, state, redirect_uri };
            Ok(Started { begin: Begin { url, expects_paste: false }, pending, receiver: Some(rx) })
        }
        None => {
            // Claude: the public client redirects to a hosted page that shows
            // the user a `code#state` string to paste back into the app.
            let redirect_uri = "https://console.anthropic.com/oauth/code/callback".to_string();
            let url = authorize_url(oauth, &redirect_uri, &challenge, &state, &client_id);
            let pending = Pending { verifier, state, redirect_uri };
            Ok(Started { begin: Begin { url, expects_paste: true }, pending, receiver: None })
        }
    }
}

fn bind_listener(preferred: u16) -> Result<(TcpListener, u16), String> {
    for port in [preferred, 0] {
        // Skip the fallback when a fixed port is required — a Codex callback
        // anywhere but :1455 would be rejected by the authorization server.
        if port != preferred && preferred != 0 {
            continue;
        }
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => {
                let actual = listener.local_addr().map_err(|e| e.to_string())?.port();
                return Ok((listener, actual));
            }
            Err(_) if port == 0 => return Err("Could not open the sign-in listener.".into()),
            Err(e) => {
                if preferred != 0 {
                    return Err(format!(
                        "Port {preferred} is busy — close the other app and retry. ({e})"
                    ));
                }
            }
        }
    }
    Err("Could not open the sign-in listener.".into())
}

/// Blocks the caller until the browser callback lands or the timeout passes.
/// Runs on a dedicated thread — nothing in here is async.
fn wait_for_code(listener: TcpListener, path: &str, expected_state: &str) -> Result<String, String> {
    listener
        .set_nonblocking(false)
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + CALLBACK_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("Sign-in timed out — the browser never came back.".into());
        }
        let (stream, _) = match listener.accept() {
            Ok(pair) => pair,
            Err(_) if Instant::now() < deadline => continue,
            Err(e) => return Err(format!("Sign-in listener failed: {e}")),
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            continue;
        }
        let Some(target) = request_line.split_whitespace().nth(1) else { continue };
        let (path_part, query) = target.split_once('?').unwrap_or((target, ""));
        if path_part != path {
            respond(&mut reader, 404, "Not found — you can close this tab.");
            continue;
        }
        let params: std::collections::HashMap<String, String> = query
            .split('&')
            .filter_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                Some((percent_decode(k), percent_decode(v)))
            })
            .collect();

        if let Some(err) = params.get("error") {
            respond(&mut reader, 200, "Sign-in failed — you can close this tab.");
            let why = params
                .get("error_description")
                .cloned()
                .unwrap_or_else(|| err.clone());
            return Err(format!("Sign-in was denied: {why}"));
        }
        let code = params.get("code").cloned();
        let state = params.get("state").cloned();
        match (code, state) {
            (Some(code), Some(state)) if state == expected_state => {
                respond(&mut reader, 200, "Code received — Coucou is finishing the sign-in. Check Settings → Providers for the result; you can close this tab.");
                return Ok(code);
            }
            (Some(_), _) => {
                respond(&mut reader, 200, "State mismatch — try signing in again.");
                return Err("Sign-in state mismatch — please try again.".into());
            }
            _ => {
                respond(&mut reader, 400, "Missing code — try signing in again.");
                continue;
            }
        }
    }
}

fn respond(reader: &mut BufReader<std::net::TcpStream>, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let page = format!(
        "<!doctype html><title>Coucou</title><body style=\"font-family:system-ui;display:grid;place-items:center;min-height:80vh\"><p>{body}</p></body>"
    );
    let _ = reader.get_mut().write_all(
        format!(
            "HTTP/1.1 {status} {reason}\r\ncontent-type: text/html\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{page}",
            page.len()
        )
        .as_bytes(),
    );
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(n) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                out.push(n);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ── URLs and token exchange ─────────────────────────────────────────────────

pub fn authorize_url(
    oauth: &OAuthSpec,
    redirect_uri: &str,
    challenge: &str,
    state: &str,
    client_id: &str,
) -> String {
    let mut url = reqwest::Url::parse(oauth.authorize_url).expect("constant URL");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", client_id);
        q.append_pair("response_type", "code");
        q.append_pair("redirect_uri", redirect_uri);
        q.append_pair("scope", &oauth.scopes.join(" "));
        q.append_pair("code_challenge", challenge);
        q.append_pair("code_challenge_method", "S256");
        q.append_pair("state", state);
        for (k, v) in oauth.extra_params {
            q.append_pair(k, v);
        }
    }
    url.to_string()
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    id_token: Option<String>,
}

fn into_tokens(
    provider: &ProviderSpec,
    r: TokenResponse,
    prior: Option<&OAuthTokens>,
) -> Result<OAuthTokens, String> {
    let mut tokens = OAuthTokens {
        access_token: r.access_token,
        refresh_token: r.refresh_token.or_else(|| prior.and_then(|t| t.refresh_token.clone())),
        expires_at: r.expires_in.map(|secs| unix_now() + secs),
        id_token: r.id_token.or_else(|| prior.and_then(|t| t.id_token.clone())),
        account_id: prior.and_then(|t| t.account_id.clone()),
    };
    if provider.id == "codex" {
        // The id_token only exists to carry `chatgpt_account_id` — once that
        // claim is lifted, keeping the JWT would push the stored bundle past
        // the Credential Manager blob limit (~2560 bytes).
        if let Some(id) = tokens.id_token.as_deref().and_then(crate::providers::openai::chatgpt_account_id) {
            tokens.account_id = Some(id);
        }
        tokens.id_token = None;
    }
    let raw = serde_json::to_string(&tokens).map_err(|e| e.to_string())?;
    crate::secrets::set(&super::oauth_secret(provider.id), &raw)
        .map_err(|e| format!("Couldn't store the {} sign-in: {e}", provider.name))?;
    Ok(tokens)
}

/// Resolves the OAuth client registration for `spec`: the baked-in public
/// literals first, then env vars, then the provider's Settings fields. Both
/// id and secret are required when the spec declares a user-supplied client —
/// installed-app clients that ship a secret always need the pair at the token
/// endpoint, so a half-configured pair fails here instead of mid-exchange.
pub fn client_credentials(
    spec: &ProviderSpec,
    settings: &Settings,
) -> Result<(String, Option<String>), String> {
    let oauth = spec.oauth.as_ref().ok_or_else(|| format!("{} has no sign-in.", spec.name))?;
    if !oauth.client_id.is_empty() {
        let secret = (!oauth.client_secret.is_empty()).then(|| oauth.client_secret.to_string());
        return Ok((oauth.client_id.to_string(), secret));
    }
    let from_env = |name: &str| {
        (!name.is_empty())
            .then(|| std::env::var(name).ok())
            .flatten()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    match from_env(oauth.client_id_env)
        .zip(from_env(oauth.client_secret_env))
        .or_else(|| settings.oauth_client(spec.id))
    {
        Some((id, secret)) => Ok((id, Some(secret))),
        None => Err(format!(
            "{} sign-in needs an OAuth client id + secret — paste them in Settings → Providers or set {} / {}.",
            spec.name, oauth.client_id_env, oauth.client_secret_env
        )),
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// What the user pasted back from the hosted page: `code#state`, the full
/// `code` alone (state checked separately when present), or a pasted URL.
pub fn split_pasted_code(input: &str) -> Option<(String, Option<String>)> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(url) = reqwest::Url::parse(trimmed) {
        let code = url
            .query_pairs()
            .find(|(k, _)| k == "code")
            .map(|(_, v)| v.into_owned());
        let state = url
            .query_pairs()
            .find(|(k, _)| k == "state")
            .map(|(_, v)| v.into_owned());
        return code.map(|c| (c, state));
    }
    // Claude's page shows `code#state`; a stray '&' after the code means the
    // user copied extra query text — take the first segment of each part.
    let (code, state) = trimmed.split_once('#').map(|(c, s)| (c, Some(s))).unwrap_or((trimmed, None));
    let code = code.split('&').next()?.trim().to_string();
    let state = state.map(|s| s.split('&').next().unwrap_or(s).trim().to_string());
    if code.is_empty() {
        return None;
    }
    Some((code, state.filter(|s| !s.is_empty())))
}

/// Manual-paste completion: checks the state when one was pasted, then does
/// the code exchange exactly like the loopback path.
pub async fn finish_paste(
    spec: &'static ProviderSpec,
    pending: Pending,
    pasted: &str,
    settings: &Settings,
) -> Result<OAuthTokens, String> {
    let (code, state) =
        split_pasted_code(pasted).ok_or_else(|| "Paste the code the page showed you.".to_string())?;
    if let Some(state) = state {
        if state != pending.state {
            return Err("That code doesn't match this sign-in — start over.".into());
        }
    }
    exchange(spec, &pending, &code, settings).await
}

/// Authorization-code exchange at the provider's token endpoint.
pub async fn exchange(
    spec: &'static ProviderSpec,
    pending: &Pending,
    code: &str,
    settings: &Settings,
) -> Result<OAuthTokens, String> {
    let (client_id, secret) = client_credentials(spec, settings)?;
    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("client_id", client_id),
        ("code", code.to_string()),
        ("redirect_uri", pending.redirect_uri.clone()),
        ("code_verifier", pending.verifier.clone()),
    ];
    if let Some(secret) = secret {
        form.push(("client_secret", secret));
    }
    let parsed: TokenResponse = token_post(spec, form.as_slice()).await?;
    into_tokens(spec, parsed, None)
}

/// Returns a usable token set: the stored one when fresh, a refresh grant
/// otherwise. Fails closed — an expired token without refresh means "connect
/// again" rather than sending a dead request.
pub async fn ensure_fresh(
    spec: &'static ProviderSpec,
    tokens: OAuthTokens,
    settings: &Settings,
) -> Result<OAuthTokens, String> {
    const SKEW: i64 = 60;
    let expired = tokens.expires_at.map(|at| unix_now() >= at - SKEW).unwrap_or(false);
    if !expired {
        return Ok(tokens);
    }
    let Some(refresh) = tokens.refresh_token.clone() else {
        return Err(format!("{} sign-in expired — sign in again in Settings.", spec.name));
    };
    let (client_id, secret) = client_credentials(spec, settings)?;
    let mut form = vec![
        ("grant_type", "refresh_token".to_string()),
        ("client_id", client_id),
        ("refresh_token", refresh),
    ];
    if let Some(secret) = secret {
        form.push(("client_secret", secret));
    }
    let parsed: TokenResponse = token_post(spec, form.as_slice())
        .await
        .map_err(|_| format!("{} sign-in expired — sign in again in Settings.", spec.name))?;
    into_tokens(spec, parsed, Some(&tokens))
}

async fn token_post(spec: &ProviderSpec, form: &[(&str, String)]) -> Result<TokenResponse, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let mut request = client.post(spec.oauth.as_ref().unwrap().token_url).form(form);
    if spec.id == "codex" {
        // codex-rs's default http client stamps every auth call with this —
        // the ChatGPT backend routes on it.
        request = request.header("originator", "codex_cli_rs");
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("Sign-in exchange failed: {e}"))?;
    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let detail = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                v.get("error_description")
                    .or_else(|| v.get("error"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| text.chars().take(200).collect());
        crate::log::line(format!("oauth {} exchange rejected ({status}): {detail}", spec.id));
        return Err(format!("Sign-in exchange failed ({status}): {detail}"));
    }
    serde_json::from_str(&text).map_err(|e| format!("Bad sign-in response: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream;

    #[test]
    fn pkce_challenge_is_s256_of_the_verifier() {
        // RFC 7636 appendix B: known verifier → known challenge.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(sha256_url(verifier), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn verifier_and_challenge_are_distinct_and_urlsafe() {
        let (v, c) = pkce_pair();
        assert_ne!(v, c);
        assert!(!v.contains('+') && !v.contains('/') && !c.contains('='));
    }

    #[test]
    fn authorize_url_carries_pkce_and_provider_params() {
        let url = authorize_url(
            super::super::spec_of("codex").oauth.as_ref().unwrap(),
            "http://localhost:1455/auth/callback",
            "CH",
            "ST",
            "app_test_client",
        );
        assert!(url.starts_with("https://auth.openai.com/oauth/authorize?"));
        assert!(url.contains("client_id=app_test_client"));
        assert!(url.contains("code_challenge=CH"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=ST"));
        assert!(url.contains("openid+profile+email+offline_access"));
    }

    #[test]
    fn split_pasted_accepts_code_hash_state() {
        assert_eq!(
            split_pasted_code("abc123#deadbeef"),
            Some(("abc123".to_string(), Some("deadbeef".to_string())))
        );
        assert_eq!(split_pasted_code("abc123"), Some(("abc123".to_string(), None)));
        assert_eq!(split_pasted_code(""), None);
        // Full callback URL pasted by mistake still works.
        assert_eq!(
            split_pasted_code("https://x/callback?code=abc&state=st1"),
            Some(("abc".to_string(), Some("st1".to_string())))
        );
    }

    #[test]
    fn callback_loop_captures_a_matching_code() {
        let (listener, port) = bind_listener(0).unwrap();
        let handle = std::thread::spawn({
            let state = "s-expected".to_string();
            move || wait_for_code(listener, "/oauth/callback", &state)
        });
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(
            stream,
            "GET /oauth/callback?code=the-code&state=s-expected HTTP/1.1\r\nhost: localhost\r\n\r\n"
        )
        .unwrap();
        assert_eq!(handle.join().unwrap().unwrap(), "the-code");
    }

    #[test]
    fn callback_loop_rejects_wrong_state() {
        let (listener, port) = bind_listener(0).unwrap();
        let handle = std::thread::spawn({
            let state = "s-expected".to_string();
            move || wait_for_code(listener, "/oauth/callback", &state)
        });
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(
            stream,
            "GET /oauth/callback?code=the-code&state=WRONG HTTP/1.1\r\nhost: localhost\r\n\r\n"
        )
        .unwrap();
        assert!(handle.join().unwrap().unwrap_err().contains("mismatch"));
    }

    #[test]
    fn expired_tokens_refresh_or_fail_closed() {
        let past = unix_now() - 10;
        // Fresh token passes through untouched.
        let fresh = OAuthTokens {
            access_token: "a".into(),
            refresh_token: None,
            expires_at: Some(unix_now() + 3600),
            id_token: None,
            account_id: None,
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let settings = Settings::default();
        let out = rt
            .block_on(ensure_fresh(super::super::spec_of("google"), fresh.clone(), &settings))
            .unwrap();
        assert_eq!(out.access_token, "a");
        // Expired without a refresh token fails closed.
        let stale = OAuthTokens {
            access_token: "a".into(),
            refresh_token: None,
            expires_at: Some(past),
            id_token: None,
            account_id: None,
        };
        assert!(rt
            .block_on(ensure_fresh(super::super::spec_of("google"), stale, &settings))
            .is_err());
    }
}
