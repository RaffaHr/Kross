// Claude adapter — the Anthropic Messages API, reachable with either an API
// key (`x-api-key`) or a Claude Code OAuth subscription token (`Bearer` +
// the oauth beta flag, like the official CLI sends).

use serde_json::{json, Value};

use super::{Credential, Part, Role, Turn, SYSTEM_PROMPT};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Server-side fallback: on a policy decline the API retries the same request
/// on a fallback model inside the same call, so the island never dead-ends.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
/// Required so the API accepts a subscription OAuth token instead of a key.
const OAUTH_BETA: &str = "oauth-2025-04-20";
const MAX_TOKENS: u32 = 4096;

fn content_block(part: &Part) -> Option<Value> {
    match part {
        Part::Text(text) => Some(json!({ "type": "text", "text": text })),
        Part::File { media_type, data_b64 } => Some(json!({
            "type": if media_type == "application/pdf" { "document" } else { "image" },
            "source": { "type": "base64", "media_type": media_type, "data": data_b64 },
        })),
        Part::Native(block) => Some(block.clone()),
    }
}

fn messages(history: &[Turn]) -> Vec<Value> {
    history
        .iter()
        .map(|turn| {
            json!({
                "role": match turn.role { Role::User => "user", Role::Assistant => "assistant" },
                "content": turn.parts.iter().filter_map(content_block).collect::<Vec<_>>(),
            })
        })
        .collect()
}

pub async fn send(credential: &Credential, model: &str, history: &[Turn]) -> Result<Turn, String> {
    let body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "system": SYSTEM_PROMPT,
        "tools": [{ "type": "web_search_20260209", "name": "web_search", "max_uses": 5 }],
        "fallbacks": "default",
        "messages": messages(history),
    });

    let response = call(credential, &body).await?;

    // A policy decline comes back as HTTP 200 with stop_reason "refusal".
    if response.get("stop_reason").and_then(Value::as_str) == Some("refusal") {
        let why = response
            .get("stop_details")
            .and_then(|d| d.get("explanation"))
            .and_then(Value::as_str)
            .unwrap_or("Claude declined this one.");
        return Err(why.to_string());
    }

    let Some(blocks) = response.get("content").and_then(Value::as_array).cloned() else {
        return Err("Unexpected API response.".into());
    };

    // Keep every block: text parts are what the island renders, the rest are
    // provider-native (tool_use / tool_result) and go back verbatim next turn.
    let parts = blocks
        .iter()
        .map(|b| {
            if b.get("type").and_then(Value::as_str) == Some("text") {
                Part::Text(b.get("text").and_then(Value::as_str).unwrap_or("").to_string())
            } else {
                Part::Native(b.clone())
            }
        })
        .collect();

    Ok(Turn { role: Role::Assistant, parts })
}

async fn call(credential: &Credential, body: &Value) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .build()
        .map_err(|e| e.to_string())?;

    let request = client
        .post(ENDPOINT)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("anthropic-beta", FALLBACK_BETA)
        .header("content-type", "application/json");
    let request = match credential {
        Credential::ApiKey(key) => request.header("x-api-key", key),
        Credential::OAuth(tokens) => request
            .header("authorization", format!("Bearer {}", tokens.access_token))
            .header("anthropic-beta", format!("{FALLBACK_BETA},{OAUTH_BETA}")),
    };

    let response = request.json(body).send().await.map_err(|e| format!("Network error: {e}"))?;

    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        // Surface the API's own message, which is what makes a bad key obvious.
        let detail = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| text.chars().take(200).collect());
        return Err(format!("Claude API {status}: {detail}"));
    }
    serde_json::from_str(&text).map_err(|e| format!("Bad API response: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{Part, Role, Turn};

    #[test]
    fn history_serializes_to_messages_api_shape() {
        let history = vec![
            Turn {
                role: Role::User,
                parts: vec![
                    Part::File { media_type: "image/png".into(), data_b64: "AA==".into() },
                    Part::Text("File: shot.png".into()),
                    Part::Text("what is this".into()),
                ],
            },
            Turn {
                role: Role::Assistant,
                parts: vec![
                    Part::Native(json!({ "type": "server_tool_use", "id": "t1" })),
                    Part::Text("a screenshot".into()),
                ],
            },
        ];
        let msgs = messages(&history);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"][0]["type"], "image");
        assert_eq!(msgs[0]["content"][0]["source"]["media_type"], "image/png");
        assert_eq!(msgs[0]["content"][2]["text"], "what is this");
        // Assistant native blocks are spliced back verbatim, order preserved.
        assert_eq!(msgs[1]["content"][0]["type"], "server_tool_use");
        assert_eq!(msgs[1]["content"][1]["text"], "a screenshot");
    }

    #[test]
    fn pdf_becomes_document_block() {
        let block = content_block(&Part::File {
            media_type: "application/pdf".into(),
            data_b64: "AA==".into(),
        })
        .unwrap();
        assert_eq!(block["type"], "document");
    }
}
