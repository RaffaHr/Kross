// Google adapter — Gemini generateContent, authenticated either with a plain
// API key (x-goog-api-key) or the OAuth subscription token (Bearer) the
// Gemini CLI obtains with its own desktop client.

use serde_json::{json, Value};

use super::{Credential, Part, Role, Turn, SYSTEM_PROMPT};

const MAX_TOKENS: u32 = 4096;

fn part_json(part: &Part) -> Option<Value> {
    match part {
        Part::Text(t) => Some(json!({ "text": t })),
        // Gemini reads PDFs and images natively as inline_data.
        Part::File { media_type, data_b64 } => {
            Some(json!({ "inline_data": { "mime_type": media_type, "data": data_b64 } }))
        }
        Part::Native(_) => None,
    }
}

fn contents(history: &[Turn]) -> Vec<Value> {
    history
        .iter()
        .map(|turn| {
            json!({
                "role": match turn.role { Role::User => "user", Role::Assistant => "model" },
                "parts": turn.parts.iter().filter_map(part_json).collect::<Vec<_>>(),
            })
        })
        .collect()
}

pub async fn send(
    credential: &Credential,
    model: &str,
    history: &[Turn],
) -> Result<Turn, String> {
    let base = super::spec_of("google").base_url.trim_end_matches('/');
    let url = format!("{base}/models/{model}:generateContent");

    let body = json!({
        "system_instruction": { "parts": [{ "text": SYSTEM_PROMPT }] },
        "contents": contents(history),
        "generationConfig": { "maxOutputTokens": MAX_TOKENS },
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .build()
        .map_err(|e| e.to_string())?;

    let request = client.post(&url).json(&body);
    let request = match credential {
        Credential::ApiKey(key) => request.header("x-goog-api-key", key),
        Credential::OAuth(tokens) => request.bearer_auth(&tokens.access_token),
    };

    let response = request.send().await.map_err(|e| format!("Network error: {e}"))?;
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
        return Err(format!("Gemini API {status}: {detail}"));
    }
    let json: Value = serde_json::from_str(&text).map_err(|e| format!("Bad API response: {e}"))?;

    if let Some(reason) = json
        .pointer("/promptFeedback/blockReason")
        .and_then(Value::as_str)
    {
        return Err(format!("Gemini blocked this request ({reason})."));
    }

    let text = json
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "Unexpected API response.".to_string())?;

    Ok(Turn { role: Role::Assistant, parts: vec![Part::Text(text)] })
}

/// `GET {base}/models` — the cloud-platform scope on our OAuth token covers it,
/// and an API key works too, so this doubles as the connectivity probe.
pub async fn list_models(credential: &Credential) -> Result<Vec<String>, String> {
    let base = super::spec_of("google").base_url.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let request = client.get(format!("{base}/models"));
    let request = match credential {
        Credential::ApiKey(key) => request.header("x-goog-api-key", key),
        Credential::OAuth(tokens) => request.bearer_auth(&tokens.access_token),
    };
    let response = request.send().await.map_err(|e| format!("Network error: {e}"))?;
    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {}", text.chars().take(200).collect::<String>()));
    }
    let body: Value = serde_json::from_str(&text).map_err(|e| format!("Bad models response: {e}"))?;
    Ok(body
        .get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|m| {
                    m.get("name")
                        .and_then(Value::as_str)
                        .map(|n| n.strip_prefix("models/").unwrap_or(n).to_string())
                })
                .collect()
        })
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_becomes_gemini_contents() {
        let history = vec![
            Turn {
                role: Role::User,
                parts: vec![
                    Part::File { media_type: "application/pdf".into(), data_b64: "AA==".into() },
                    Part::Text("summarize".into()),
                ],
            },
            Turn { role: Role::Assistant, parts: vec![Part::Text("done".into())] },
        ];
        let msgs = contents(&history);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["parts"][0]["inline_data"]["mime_type"], "application/pdf");
        assert_eq!(msgs[1]["role"], "model");
    }
}
