// OpenAI adapter — covers every provider that speaks the OpenAI wire format:
// Codex (api.openai.com key auth, or the ChatGPT-subscription Responses
// backend over OAuth), Hermes (Nous, OpenAI-compatible) and the user's own
// OpenAI-compatible endpoint ("custom").

use serde_json::{json, Value};

use super::{Credential, OAuthTokens, Part, ProviderSpec, Role, Turn, SYSTEM_PROMPT};

const MAX_TOKENS: u32 = 4096;
/// Codex's subscription path — the same backend the official Codex CLI hits.
const CODEX_RESPONSES: &str = "https://chatgpt.com/backend-api/codex/responses";
/// Codex's own models list — subscription tokens are scoped to the ChatGPT
/// backend, not api.openai.com.
const CODEX_MODELS: &str = "https://chatgpt.com/backend-api/codex/models";
/// The backend filters /models by each model's `minimal_client_version` —
/// sending Coucou's own version (0.1.x) makes every gated model disappear,
/// which is exactly the "empty list → fallback" bug this constant prevents.
/// Report the highest gate in Codex's published model manifest
/// (codex-rs/models-manager/models.json); bump it when newer gated models
/// stop appearing.
const CODEX_CLIENT_VERSION: &str = "0.155.0";

/// Parts of a turn in chat/completions shape. Binary files only work as
/// images (data URIs); a PDF degrades to a plain-text note.
fn openai_content(parts: &[Part]) -> Value {
    let mut content: Vec<Value> = Vec::new();
    let mut dropped = 0usize;
    for part in parts {
        match part {
            Part::Text(t) => content.push(json!({ "type": "text", "text": t })),
            Part::File { media_type, data_b64 } if media_type.starts_with("image/") => {
                content.push(json!({
                    "type": "image_url",
                    "image_url": { "url": format!("data:{media_type};base64,{data_b64}") },
                }));
            }
            Part::File { .. } => dropped += 1,
            Part::Native(_) => {}
        }
    }
    if dropped > 0 {
        content.push(json!({
            "type": "text",
            "text": format!("({dropped} attachment(s) omitted — this provider can't read them)")
        }));
    }
    json!(content)
}

fn chat_messages(history: &[Turn]) -> Vec<Value> {
    let mut messages = vec![json!({ "role": "system", "content": SYSTEM_PROMPT })];
    for turn in history {
        messages.push(json!({
            "role": match turn.role { Role::User => "user", Role::Assistant => "assistant" },
            "content": openai_content(&turn.parts),
        }));
    }
    messages
}

/// Any OpenAI-compatible endpoint: `{base}/chat/completions` + Bearer key.
pub async fn send_compatible(
    spec: &ProviderSpec,
    credential: &Credential,
    model: &str,
    history: &[Turn],
) -> Result<Turn, String> {
    let base = spec.base_url.trim_end_matches('/');
    if base.is_empty() {
        return Err("Set the provider's base URL in Settings first.".into());
    }
    if model.is_empty() {
        return Err("Set a model name for this provider in Settings first.".into());
    }
    let key = match credential {
        Credential::ApiKey(k) => k.clone(),
        Credential::OAuth(_) => return Err(format!("{} sign-in isn't supported.", spec.name)),
    };

    let body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "messages": chat_messages(history),
    });

    let client = http_client()?;
    let response = client
        .post(format!("{base}/chat/completions"))
        .bearer_auth(key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;
    let json = read_json(response).await?;

    // Standard chat/completions answer; OpenRouter-style `choices[0].message`.
    let text = json
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::to_string)
        // Some compatible servers answer with a content array.
        .or_else(|| {
            json.pointer("/choices/0/message/content")
                .and_then(Value::as_array)
                .map(|parts| {
                    parts
                        .iter()
                        .filter(|p| p.get("type").and_then(Value::as_str) == Some("text"))
                        .filter_map(|p| p.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
        })
        .ok_or_else(|| "Unexpected API response.".to_string())?;

    Ok(Turn { role: Role::Assistant, parts: vec![Part::Text(text)] })
}

/// Codex: API key → plain chat/completions on api.openai.com; OAuth → the
/// subscription-scoped Responses backend the Codex CLI itself uses.
pub async fn send_codex(
    credential: &Credential,
    model: &str,
    history: &[Turn],
) -> Result<Turn, String> {
    match credential {
        Credential::ApiKey(_) => {
            let spec = super::spec_of("codex");
            send_compatible(spec, credential, model, history).await
        }
        Credential::OAuth(tokens) => send_codex_responses(tokens, model, history).await,
    }
}

/// Responses API over the ChatGPT backend. The backend requires SSE, so the
/// reply is read off `response.completed` / output-text deltas from the stream.
async fn send_codex_responses(
    tokens: &OAuthTokens,
    model: &str,
    history: &[Turn],
) -> Result<Turn, String> {
    let input: Vec<Value> = history
        .iter()
        .map(|turn| {
            json!({
                "type": "message",
                "role": match turn.role { Role::User => "user", Role::Assistant => "assistant" },
                "content": turn.parts.iter().filter_map(|p| match p {
                    Part::Text(t) => Some(json!({
                        "type": if turn.role == Role::User { "input_text" } else { "output_text" },
                        "text": t,
                    })),
                    _ => None,
                }).collect::<Vec<_>>(),
            })
        })
        .collect();

    let body = json!({
        "model": model,
        "instructions": SYSTEM_PROMPT,
        "input": input,
        "store": false,
        "stream": true,
    });

    let client = http_client()?;
    let mut request = client
        .post(CODEX_RESPONSES)
        .bearer_auth(&tokens.access_token)
        .header("OpenAI-Beta", "responses=experimental")
        .header("originator", "codex_cli_rs")
        .header("accept", "text/event-stream")
        .json(&body);
    if let Some(account) = tokens.account_id.as_deref() {
        request = request.header("chatgpt-account-id", account);
    }

    let response = request.send().await.map_err(|e| format!("Network error: {e}"))?;
    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("Codex API {status}: {}", text.chars().take(200).collect::<String>()));
    }

    parse_responses_sse(&text)
        .map(|t| Turn { role: Role::Assistant, parts: vec![Part::Text(t)] })
}

/// Pulls the final text out of a Responses SSE body: prefer the
/// `response.completed` event's output items, fall back to delta accumulation.
fn parse_responses_sse(body: &str) -> Result<String, String> {
    let mut completed: Option<String> = None;
    let mut delta = String::new();
    for line in body.lines().filter_map(|l| l.strip_prefix("data:")) {
        let Ok(event) = serde_json::from_str::<Value>(line.trim()) else { continue };
        match event.get("type").and_then(Value::as_str) {
            Some("response.output_text.delta") => {
                if let Some(d) = event.get("delta").and_then(Value::as_str) {
                    delta.push_str(d);
                }
            }
            Some("response.completed") => {
                if let Some(items) = event.pointer("/response/output").and_then(Value::as_array) {
                    let text = items
                        .iter()
                        .flat_map(|item| item.pointer("/content").and_then(Value::as_array).into_iter().flatten())
                        .filter(|c| c.get("type").and_then(Value::as_str) == Some("output_text"))
                        .filter_map(|c| c.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !text.is_empty() {
                        completed = Some(text);
                    }
                }
            }
            _ => {}
        }
    }
    completed.or(if delta.is_empty() { None } else { Some(delta) }).ok_or_else(|| {
        "No response text in the Codex stream.".to_string()
    })
}

/// `chatgpt_account_id` lives inside the id_token JWT payload — read it
/// without verifying (it's our own stored token, not an authentication check).
/// `pub(crate)` so oauth.rs can lift the claim before dropping the JWT.
pub(crate) fn chatgpt_account_id(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = decode_base64url(payload)?;
    let json: Value = serde_json::from_slice(&bytes).ok()?;
    // The claim name contains slashes, so JSON pointer is out — index directly.
    json.get("https://api.openai.com/auth")
        .and_then(|auth| auth.get("chatgpt_account_id"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn decode_base64url(input: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(input).ok()
}

/// `GET {base}/models` for OpenAI-compatible endpoints; Codex OAuth goes to the
/// ChatGPT backend instead, where subscription tokens are actually valid.
pub async fn list_models(
    spec: &ProviderSpec,
    credential: &Credential,
) -> Result<Vec<String>, String> {
    let client = http_client()?;
    if spec.id == "codex" {
        if let Credential::OAuth(tokens) = credential {
            let mut request = client
                .get(CODEX_MODELS)
                .query(&[("client_version", CODEX_CLIENT_VERSION)])
                .bearer_auth(&tokens.access_token)
                .header("originator", "codex_cli_rs");
            if let Some(account) = tokens.account_id.as_deref() {
                request = request.header("chatgpt-account-id", account);
            }
            let body = read_json(request.send().await.map_err(|e| format!("Network error: {e}"))?).await?;
            return Ok(parse_codex_models(&body));
        }
    }
    let base = spec.base_url.trim_end_matches('/');
    if base.is_empty() {
        return Err("Set the provider's base URL in Settings first.".into());
    }
    let key = match credential {
        Credential::ApiKey(k) => k,
        Credential::OAuth(_) => return Err(format!("{} sign-in isn't supported.", spec.name)),
    };
    let body = read_json(
        client
            .get(format!("{base}/models"))
            .bearer_auth(key)
            .send()
            .await
            .map_err(|e| format!("Network error: {e}"))?,
    )
    .await?;
    Ok(body
        .get("data")
        .and_then(Value::as_array)
        .map(|data| {
            data.iter()
                .filter_map(|m| m.get("id").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

/// The ChatGPT catalog returns internal entries next to the real picker list:
/// `visibility:"hide"` marks non-selectable tools (codex-auto-review,
/// daybreak-*), `supported_in_api:false` marks entries the Responses backend
/// won't serve, and `priority` is the server's own ordering. Offer only what
/// can actually answer a request — picking a hidden/internal slug is the
/// "model not available" failure the user hits.
fn parse_codex_models(body: &Value) -> Vec<String> {
    let list = body
        .get("models")
        .or_else(|| body.get("data"))
        .and_then(Value::as_array);
    let Some(list) = list else { return Vec::new() };
    let mut usable: Vec<(i64, String)> = list
        .iter()
        .filter(|m| {
            m.get("visibility")
                .and_then(Value::as_str)
                .map_or(true, |v| v == "list")
        })
        .filter(|m| {
            m.get("supported_in_api")
                .and_then(Value::as_bool)
                .map_or(true, |b| b)
        })
        .filter_map(|m| {
            let slug = m
                .get("slug")
                .or_else(|| m.get("id"))
                .and_then(Value::as_str)?;
            let priority = m.get("priority").and_then(Value::as_i64).unwrap_or(i64::MAX);
            Some((priority, slug.to_string()))
        })
        .collect();
    usable.sort_by_key(|(priority, _)| *priority);
    usable.into_iter().map(|(_, slug)| slug).collect()
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .build()
        .map_err(|e| e.to_string())
}

async fn read_json(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let detail = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                v.pointer("/error/message")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| text.chars().take(200).collect());
        return Err(format!("HTTP {status}: {detail}"));
    }
    serde_json::from_str(&text).map_err(|e| format!("Bad API response: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_becomes_chat_completions_messages() {
        let history = vec![Turn {
            role: Role::User,
            parts: vec![
                Part::Text("hello".into()),
                Part::File { media_type: "image/png".into(), data_b64: "AA==".into() },
                Part::Native(json!({ "x": 1 })),
            ],
        }];
        let msgs = chat_messages(&history);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[1]["content"][0]["type"], "text");
        assert_eq!(
            msgs[1]["content"][1]["image_url"]["url"],
            "data:image/png;base64,AA=="
        );
        // Native parts don't leak into the OpenAI shape.
        assert_eq!(msgs[1]["content"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn non_image_files_degrade_to_a_note() {
        let content = openai_content(&[Part::File {
            media_type: "application/pdf".into(),
            data_b64: "AA==".into(),
        }]);
        assert_eq!(content[0]["type"], "text");
        assert!(content[0]["text"].as_str().unwrap().contains("omitted"));
    }

    #[test]
    fn sse_completed_event_wins_over_deltas() {
        let sse = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hel\"}\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"output\":[",
            "{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hello!\"}]}",
            "]}}\n",
        );
        assert_eq!(parse_responses_sse(sse).unwrap(), "Hello!");
    }

    #[test]
    fn sse_without_completed_falls_back_to_deltas() {
        let sse = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hi\"}\n";
        assert_eq!(parse_responses_sse(sse).unwrap(), "Hi");
        assert!(parse_responses_sse("data: {}\n").is_err());
    }

    #[test]
    fn codex_models_keep_only_selectable_slugs_in_priority_order() {
        // Shape mirrors chatgpt.com/backend-api/codex/models.
        let body = json!({
            "models": [
                { "slug": "codex-auto-review", "visibility": "hide",
                  "supported_in_api": true, "priority": 43 },
                { "slug": "gpt-5.5", "visibility": "list",
                  "supported_in_api": true, "priority": 13 },
                { "slug": "gpt-6.1-sol", "visibility": "list",
                  "supported_in_api": true, "priority": 1 },
                { "slug": "gpt-legacy", "visibility": "list",
                  "supported_in_api": false, "priority": 2 }
            ]
        });
        assert_eq!(parse_codex_models(&body), ["gpt-6.1-sol", "gpt-5.5"]);
        // Entries without the new fields stay usable (older API shape).
        assert_eq!(parse_codex_models(&json!({ "data": [{ "id": "a" }] })), ["a"]);
    }

    #[test]
    fn account_id_reads_the_jwt_payload() {
        use base64::Engine;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"https://api.openai.com/auth":{"chatgpt_account_id":"acc-123"}}"#);
        let token = format!("header.{payload}.sig");
        assert_eq!(chatgpt_account_id(&token).as_deref(), Some("acc-123"));
        assert_eq!(chatgpt_account_id("not-a-jwt"), None);
    }
}
