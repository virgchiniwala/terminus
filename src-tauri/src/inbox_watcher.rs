use crate::email_connections::{self, EmailProvider};
use crate::runner::RunnerEngine;
use crate::schema::{AutopilotPlan, ProviderId, RecipeKind};
use reqwest::blocking::Client;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_EMAIL_BODY_CHARS: usize = 12_000;

#[derive(Debug, Clone)]
struct InboundMessage {
    provider_message_id: String,
    provider_thread_id: Option<String>,
    sender_email: Option<String>,
    subject: String,
    body_preview: String,
    received_at_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxWatcherTickSummary {
    pub provider: String,
    pub autopilot_id: String,
    pub fetched: usize,
    pub deduped: usize,
    pub started_runs: usize,
    pub failed: usize,
}

pub fn run_watcher_tick(
    connection: &mut Connection,
    provider_raw: &str,
    autopilot_id: &str,
    max_items: usize,
) -> Result<InboxWatcherTickSummary, String> {
    let provider = EmailProvider::parse(provider_raw)
        .ok_or_else(|| "Unsupported email provider.".to_string())?;
    let token = email_connections::get_access_token(connection, provider)?;
    let messages = fetch_messages(provider, &token, max_items)?;
    let mut deduped = 0usize;
    let mut started_runs = 0usize;
    let mut failed = 0usize;

    for message in &messages {
        let dedupe_key = format!("{}:{}", provider.as_str(), message.provider_message_id);
        let already_seen: Option<String> = connection
            .query_row(
                "SELECT id FROM email_ingest_events WHERE dedupe_key = ?1 LIMIT 1",
                params![dedupe_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("Failed to check inbox dedupe: {e}"))?;
        if already_seen.is_some() {
            deduped += 1;
            continue;
        }

        let intent = format!("Triage inbox message: {}", message.subject);
        let mut plan =
            AutopilotPlan::from_intent(RecipeKind::InboxTriage, intent, ProviderId::OpenAi);
        if let Some(sender) = message.sender_email.as_ref() {
            plan.recipient_hints = vec![sender.clone()];
        }
        let source = format!(
            "Subject: {}\n\n{}",
            message.subject,
            message
                .body_preview
                .chars()
                .take(MAX_EMAIL_BODY_CHARS)
                .collect::<String>()
        );
        plan.inbox_source_text = Some(source);
        let idempotency_key = format!(
            "inbox:{}:{}",
            provider.as_str(),
            message.provider_message_id
        );

        let run_result =
            RunnerEngine::start_run(connection, autopilot_id, plan, &idempotency_key, 2);
        let (status, run_id) = match run_result {
            Ok(run) => {
                started_runs += 1;
                ("queued".to_string(), Some(run.id))
            }
            Err(_) => {
                failed += 1;
                ("failed".to_string(), None)
            }
        };

        connection
            .execute(
                "INSERT INTO email_ingest_events (
                   id, provider, provider_message_id, provider_thread_id, sender_email, dedupe_key, autopilot_id, subject, received_at_ms, run_id, status, created_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    make_id("ingest"),
                    provider.as_str(),
                    message.provider_message_id,
                    message.provider_thread_id.as_deref(),
                    message.sender_email.as_deref(),
                    dedupe_key,
                    autopilot_id,
                    message.subject,
                    message.received_at_ms,
                    run_id,
                    status,
                    now_ms()
                ],
            )
            .map_err(|e| format!("Failed to persist ingest event: {e}"))?;
    }

    Ok(InboxWatcherTickSummary {
        provider: provider.as_str().to_string(),
        autopilot_id: autopilot_id.to_string(),
        fetched: messages.len(),
        deduped,
        started_runs,
        failed,
    })
}

fn fetch_messages(
    provider: EmailProvider,
    access_token: &str,
    max_items: usize,
) -> Result<Vec<InboundMessage>, String> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "Could not initialize secure network client.".to_string())?;
    match provider {
        EmailProvider::Gmail => fetch_gmail_messages(&client, access_token, max_items),
        EmailProvider::Microsoft365 => fetch_ms_messages(&client, access_token, max_items),
    }
}

fn fetch_gmail_messages(
    client: &Client,
    access_token: &str,
    max_items: usize,
) -> Result<Vec<InboundMessage>, String> {
    let list_url = format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages?labelIds=INBOX&maxResults={}",
        max_items.clamp(1, 25)
    );
    let list_json = client
        .get(list_url)
        .bearer_auth(access_token)
        .send()
        .map_err(|_| "Could not read Gmail inbox. Check connection and try again.".to_string())?
        .json::<Value>()
        .map_err(|_| "Could not parse Gmail inbox response.".to_string())?;
    let ids = list_json
        .get("messages")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.get("id").and_then(|v| v.as_str()))
                .map(|id| id.to_string())
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    let mut out = Vec::new();
    for id in ids {
        let details_url = format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/messages/{id}?format=metadata&metadataHeaders=Subject"
        );
        let details = client
            .get(details_url)
            .bearer_auth(access_token)
            .send()
            .map_err(|_| "Could not read Gmail message details.".to_string())?
            .json::<Value>()
            .map_err(|_| "Could not parse Gmail message details.".to_string())?;
        let headers = details
            .get("payload")
            .and_then(|v| v.get("headers"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let subject = headers
            .iter()
            .find_map(|h| {
                let name = h.get("name").and_then(|v| v.as_str())?;
                if name.eq_ignore_ascii_case("subject") {
                    h.get("value")
                        .and_then(|v| v.as_str())
                        .map(|v| v.to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "(No subject)".to_string());
        let sender_email = headers.iter().find_map(|h| {
            let name = h.get("name").and_then(|v| v.as_str())?;
            if name.eq_ignore_ascii_case("from") {
                h.get("value")
                    .and_then(|v| v.as_str())
                    .map(extract_email_address)
            } else {
                None
            }
        });
        let snippet = details
            .get("snippet")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let received_at = details
            .get("internalDate")
            .and_then(|v| v.as_str())
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or_else(now_ms);
        out.push(InboundMessage {
            provider_message_id: id,
            provider_thread_id: details
                .get("threadId")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string()),
            sender_email,
            subject,
            body_preview: snippet,
            received_at_ms: received_at,
        });
    }
    Ok(out)
}

fn fetch_ms_messages(
    client: &Client,
    access_token: &str,
    max_items: usize,
) -> Result<Vec<InboundMessage>, String> {
    let url = format!(
        "https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages?$top={}&$select=id,subject,bodyPreview,receivedDateTime,internetMessageId",
        max_items.clamp(1, 25)
    );
    let json = client
        .get(url)
        .bearer_auth(access_token)
        .send()
        .map_err(|_| "Could not read Microsoft inbox. Check connection and try again.".to_string())?
        .json::<Value>()
        .map_err(|_| "Could not parse Microsoft inbox response.".to_string())?;
    let items = json
        .get("value")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for item in items {
        let id = item
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            continue;
        }
        let subject = item
            .get("subject")
            .and_then(|v| v.as_str())
            .unwrap_or("(No subject)")
            .to_string();
        let preview = item
            .get("bodyPreview")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let sender_email = item
            .get("from")
            .and_then(|v| v.get("emailAddress"))
            .and_then(|v| v.get("address"))
            .and_then(|v| v.as_str())
            .map(|v| v.to_ascii_lowercase());
        let received_at_ms = now_ms();
        out.push(InboundMessage {
            provider_message_id: id,
            provider_thread_id: item
                .get("conversationId")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string()),
            sender_email,
            subject,
            body_preview: preview,
            received_at_ms,
        });
    }
    Ok(out)
}

fn make_id(prefix: &str) -> String {
    format!("{}_{}", prefix, now_ms())
}

fn extract_email_address(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some((_, right)) = trimmed.rsplit_once('<') {
        return right.trim_end_matches('>').trim().to_ascii_lowercase();
    }
    trimmed
        .split_whitespace()
        .find(|part| part.contains('@'))
        .unwrap_or(trimmed)
        .trim_matches(|c: char| ",.;:!?()[]{}<>\"'".contains(c))
        .to_ascii_lowercase()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
