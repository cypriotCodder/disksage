//! Claude integration. Only metadata is sent: path, size, file count, last
//! modified date and the rule category if one matched. Never file contents.

use std::collections::HashMap;

use disksage_core::candidates::{AiNote, Candidate};
use disksage_core::util::{human_size, ymd};
use disksage_core::Verdict;
use serde::Deserialize;
use serde_json::json;

use crate::settings::Settings;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

const SYSTEM_PROMPT: &str = "You are a Windows storage expert helping one person clean up their own PC. \
You get a list of large folders and files (metadata only, never contents). For each item, say what it most likely is \
and whether moving it to the Recycle Bin is safe.\n\
Verdicts:\n\
- \"safe\": caches, temp data, build output, leftovers that are recreated automatically or clearly unneeded.\n\
- \"caution\": probably removable but the person should look first (old downloads, installers, VM images, app data that may hold settings).\n\
- \"keep\": installed programs, games, the person's own work, photos, videos, music, documents, save games, mail, databases, anything you are unsure about.\n\
Be conservative. If you cannot tell what something is, say so and use \"caution\" or \"keep\". \
When there is a better way to reclaim the space (an app's own setting, an uninstall, a cleanup command), put it in \"tip\".\n\
Reply with JSON only, no prose, in exactly this shape:\n\
{\"items\":[{\"id\":1,\"what\":\"max 8 words\",\"verdict\":\"safe|caution|keep\",\"reason\":\"one short sentence\",\"tip\":\"\"}]}";

pub struct Redactor {
    home_norm: String,
    home_len: usize,
}

impl Redactor {
    pub fn new(enabled: bool) -> Self {
        let home = if enabled { std::env::var("USERPROFILE").unwrap_or_default() } else { String::new() };
        Self { home_norm: home.to_lowercase(), home_len: home.len() }
    }

    pub fn apply(&self, path: &str) -> String {
        if self.home_len == 0 {
            return path.to_string();
        }
        let (Some(head), Some(rest)) = (path.get(..self.home_len), path.get(self.home_len..)) else {
            return path.to_string();
        };
        if head.to_lowercase() == self.home_norm && (rest.is_empty() || rest.starts_with('\\')) {
            format!("C:\\Users\\%USER%{rest}")
        } else {
            path.to_string()
        }
    }
}

fn describe(c: &Candidate, r: &Redactor) -> String {
    let kind = if c.is_dir { "folder" } else { "file" };
    let mut line = format!(
        "id={} | {kind} | {} | {} | last modified {}",
        c.id,
        r.apply(&c.path),
        human_size(c.size),
        ymd(c.modified)
    );
    if c.is_dir {
        line.push_str(&format!(" | {} files", c.file_count));
    }
    if c.verdict.is_some() && !c.category.is_empty() {
        line.push_str(&format!(" | local rule says: {}", c.category));
    }
    line
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct AiReply {
    items: Vec<AiItem>,
}

#[derive(Deserialize)]
struct AiItem {
    id: u32,
    #[serde(default)]
    what: String,
    #[serde(default)]
    verdict: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    tip: String,
}

async fn call_claude(client: &reqwest::Client, s: &Settings, system: &str, user: &str, max_tokens: u32) -> Result<String, String> {
    if s.api_key.trim().is_empty() {
        return Err("No Claude API key yet. Add one in Settings.".into());
    }
    let body = json!({
        "model": s.model,
        "max_tokens": max_tokens,
        "system": system,
        "messages": [{ "role": "user", "content": user }]
    });
    let resp = client
        .post(API_URL)
        .header("x-api-key", s.api_key.trim())
        .header("anthropic-version", API_VERSION)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Could not reach the Claude API: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let msg = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(String::from))
            .unwrap_or_else(|| text.chars().take(300).collect());
        return Err(format!("Claude API error {}: {msg}", status.as_u16()));
    }
    let parsed: ApiResponse = serde_json::from_str(&text).map_err(|e| format!("Unexpected API response: {e}"))?;
    Ok(parsed
        .content
        .into_iter()
        .filter(|b| b.kind == "text")
        .map(|b| b.text)
        .collect::<Vec<_>>()
        .join(""))
}

/// Pull the JSON object out of a reply, tolerating ```json fences or stray text.
fn extract_json(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then(|| &text[start..=end])
}

/// Ask Claude about one batch. Returns notes keyed by candidate id.
pub async fn analyze_batch(
    client: &reqwest::Client,
    s: &Settings,
    batch: &[Candidate],
) -> Result<HashMap<u32, AiNote>, String> {
    let r = Redactor::new(s.redact_username);
    let mut user = String::from("Items on this Windows PC:\n");
    for c in batch {
        user.push_str(&describe(c, &r));
        user.push('\n');
    }
    let max_tokens = (batch.len() as u32 * 120).clamp(1024, 8192);
    let text = call_claude(client, s, SYSTEM_PROMPT, &user, max_tokens).await?;
    let json = extract_json(&text).ok_or("Claude did not return JSON. Try again.")?;
    let reply: AiReply = serde_json::from_str(json).map_err(|e| format!("Could not read Claude's answer: {e}"))?;

    let ids: std::collections::HashSet<u32> = batch.iter().map(|c| c.id).collect();
    Ok(reply
        .items
        .into_iter()
        .filter(|i| ids.contains(&i.id))
        .map(|i| {
            let tip = i.tip.trim().to_string();
            (
                i.id,
                AiNote {
                    what: i.what.trim().to_string(),
                    // Unknown or missing verdicts are treated as "keep".
                    verdict: Verdict::parse(&i.verdict).unwrap_or(Verdict::Keep),
                    reason: i.reason.trim().to_string(),
                    tip: (!tip.is_empty()).then_some(tip),
                },
            )
        })
        .collect())
}

/// Cheap request to check the key and model work.
pub async fn test_connection(client: &reqwest::Client, s: &Settings) -> Result<String, String> {
    let text = call_claude(client, s, "Reply with the single word: ok", "ping", 16).await?;
    Ok(format!("Connected to {} (reply: {})", s.model, text.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_fenced_json() {
        let t = "```json\n{\"items\":[]}\n```";
        assert_eq!(extract_json(t), Some("{\"items\":[]}"));
        assert_eq!(extract_json("nope"), None);
    }

    #[test]
    fn redacts_home() {
        let r = Redactor { home_norm: "c:\\users\\nedim".into(), home_len: 14 };
        assert_eq!(r.apply("C:\\Users\\Nedim\\AppData\\x"), "C:\\Users\\%USER%\\AppData\\x");
        assert_eq!(r.apply("D:\\games"), "D:\\games");
    }
}
