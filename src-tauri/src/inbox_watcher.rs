use crate::email_connections::{self, EmailProvider};
use crate::runner::RunnerEngine;
use crate::schema::{AutopilotPlan, ProviderId, RecipeKind};
use reqwest::blocking::Client;
use reqwest::header::CONTENT_TYPE;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_EMAIL_BODY_CHARS: usize = 12_000;
const WATCHER_BASE_BACKOFF_MS: i64 = 30_000;
const WATCHER_MAX_BACKOFF_MS: i64 = 15 * 60_000;

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
    let now = now_ms();
    if let Some(backoff_until_ms) = watcher_backoff_until(connection, provider)? {
        if backoff_until_ms > now {
            return Ok(InboxWatcherTickSummary {
                provider: provider.as_str().to_string(),
                autopilot_id: autopilot_id.to_string(),
                fetched: 0,
                deduped: 0,
                started_runs: 0,
                failed: 0,
            });
        }
    }
    let token = email_connections::get_access_token(connection, provider)?;
    let messages = match fetch_messages(provider, &token, max_items) {
        Ok(messages) => {
            clear_watcher_backoff(connection, provider)?;
            messages
        }
        Err(err) => {
            let retry_after_ms = if is_rate_limited_error(&err) {
                Some(next_backoff_ms(connection, provider)?)
            } else if is_retryable_watcher_error(&err) {
                Some(next_backoff_ms(connection, provider)?)
            } else {
                None
            };
            record_watcher_failure(connection, provider, &err, retry_after_ms)?;
            return Err(err);
        }
    };
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
        let provider_id = preferred_provider_for_autopilot(connection, autopilot_id)
            .unwrap_or(ProviderId::OpenAi);
        let mut plan = AutopilotPlan::from_intent(RecipeKind::InboxTriage, intent, provider_id);
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
        .error_for_status()
        .map_err(|e| {
            if e.status().map(|s| s.as_u16()) == Some(429) {
                "Gmail inbox is rate-limited right now. Terminus will try again shortly."
                    .to_string()
            } else {
                "Could not read Gmail inbox. Check connection and try again.".to_string()
            }
        })?
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

    if ids.is_empty() {
        return Ok(Vec::new());
    }

    match fetch_gmail_message_details_batch(client, access_token, &ids) {
        Ok(details_list) => {
            let mut out = Vec::new();
            for details in details_list {
                if let Some(item) = gmail_message_from_details(&details) {
                    out.push(item);
                }
            }
            Ok(out)
        }
        Err(_) => fetch_gmail_messages_sequential(client, access_token, &ids),
    }
}

fn fetch_gmail_messages_sequential(
    client: &Client,
    access_token: &str,
    ids: &[String],
) -> Result<Vec<InboundMessage>, String> {
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
            .error_for_status()
            .map_err(|e| {
                if e.status().map(|s| s.as_u16()) == Some(429) {
                    "Gmail message details are rate-limited right now.".to_string()
                } else {
                    "Could not read Gmail message details.".to_string()
                }
            })?
            .json::<Value>()
            .map_err(|_| "Could not parse Gmail message details.".to_string())?;
        if let Some(item) = gmail_message_from_details(&details) {
            out.push(item);
        } else {
            out.push(InboundMessage {
                provider_message_id: id.to_string(),
                provider_thread_id: None,
                sender_email: None,
                subject: "(No subject)".to_string(),
                body_preview: String::new(),
                received_at_ms: now_ms(),
            });
        }
    }
    Ok(out)
}

fn fetch_gmail_message_details_batch(
    client: &Client,
    access_token: &str,
    ids: &[String],
) -> Result<Vec<Value>, String> {
    let boundary = format!("terminus_batch_{}", now_ms());
    let mut body = String::new();
    for id in ids {
        body.push_str(&format!("--{boundary}\r\n"));
        body.push_str("Content-Type: application/http\r\n");
        body.push_str(&format!("Content-ID: <{id}>\r\n\r\n"));
        body.push_str(&format!(
            "GET /gmail/v1/users/me/messages/{id}?format=metadata&metadataHeaders=Subject HTTP/1.1\r\n\r\n"
        ));
    }
    body.push_str(&format!("--{boundary}--\r\n"));

    let response = client
        .post("https://gmail.googleapis.com/batch/gmail/v1")
        .bearer_auth(access_token)
        .header(
            CONTENT_TYPE,
            format!("multipart/mixed; boundary={boundary}"),
        )
        .body(body)
        .send()
        .map_err(|_| "Could not read Gmail message details.".to_string())?
        .error_for_status()
        .map_err(|e| {
            if e.status().map(|s| s.as_u16()) == Some(429) {
                "Gmail message details are rate-limited right now.".to_string()
            } else {
                "Could not read Gmail message details.".to_string()
            }
        })?;

    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let response_body = response
        .text()
        .map_err(|_| "Could not parse Gmail batch response.".to_string())?;
    parse_gmail_batch_response(&content_type, &response_body)
}

fn parse_gmail_batch_response(content_type: &str, raw: &str) -> Result<Vec<Value>, String> {
    let boundary = content_type
        .split(';')
        .find_map(|part| {
            let part = part.trim();
            part.strip_prefix("boundary=").map(|v| v.trim_matches('"'))
        })
        .ok_or_else(|| "Could not parse Gmail batch response.".to_string())?;
    let marker = format!("--{boundary}");
    let mut out = Vec::new();
    for part in raw.split(&marker) {
        let chunk = part.trim();
        if chunk.is_empty() || chunk == "--" {
            continue;
        }
        let http_start = chunk
            .find("HTTP/1.1")
            .ok_or_else(|| "Could not parse Gmail batch response.".to_string())?;
        let http_part = &chunk[http_start..];
        if let Some(status_line) = http_part.lines().next() {
            if status_line.contains(" 429 ") {
                return Err("Gmail message details are rate-limited right now.".to_string());
            }
        }
        let json_start = http_part
            .find("\r\n\r\n")
            .map(|idx| idx + 4)
            .or_else(|| http_part.find("\n\n").map(|idx| idx + 2))
            .ok_or_else(|| "Could not parse Gmail batch response.".to_string())?;
        let json_text = http_part[json_start..].trim();
        let parsed = serde_json::from_str::<Value>(json_text)
            .map_err(|_| "Could not parse Gmail batch response.".to_string())?;
        out.push(parsed);
    }
    Ok(out)
}

fn gmail_message_from_details(details: &Value) -> Option<InboundMessage> {
    let id = details.get("id").and_then(|v| v.as_str())?.to_string();
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
    Some(InboundMessage {
        provider_message_id: id,
        provider_thread_id: details
            .get("threadId")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string()),
        sender_email,
        subject,
        body_preview: snippet,
        received_at_ms: received_at,
    })
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
        .error_for_status()
        .map_err(|e| {
            if e.status().map(|s| s.as_u16()) == Some(429) {
                "Microsoft inbox is rate-limited right now. Terminus will try again shortly."
                    .to_string()
            } else {
                "Could not read Microsoft inbox. Check connection and try again.".to_string()
            }
        })?
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
        let received_at_ms = item
            .get("receivedDateTime")
            .and_then(|v| v.as_str())
            .and_then(parse_rfc3339_ms)
            .unwrap_or_else(now_ms);
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
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}_{}_{}", prefix, now_ms(), seq)
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

fn parse_rfc3339_ms(raw: &str) -> Option<i64> {
    let dt = chrono::DateTime::parse_from_rfc3339(raw).ok()?;
    Some(dt.timestamp_millis())
}

fn preferred_provider_for_autopilot(
    connection: &Connection,
    autopilot_id: &str,
) -> Option<ProviderId> {
    let provider_kind: Option<String> = connection
        .query_row(
            "SELECT provider_kind FROM runs WHERE autopilot_id = ?1 ORDER BY created_at DESC LIMIT 1",
            params![autopilot_id],
            |row| row.get(0),
        )
        .optional()
        .ok()?;
    match provider_kind.as_deref() {
        Some("anthropic") => Some(ProviderId::Anthropic),
        Some("gemini") => Some(ProviderId::Gemini),
        Some("openai") => Some(ProviderId::OpenAi),
        _ => None,
    }
}

fn watcher_backoff_until(
    connection: &Connection,
    provider: EmailProvider,
) -> Result<Option<i64>, String> {
    connection
        .query_row(
            "SELECT backoff_until_ms FROM inbox_watcher_state WHERE provider = ?1",
            params![provider.as_str()],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()
        .map_err(|e| format!("Failed to load watcher state: {e}"))
        .map(|v| v.flatten())
}

fn next_backoff_ms(connection: &Connection, provider: EmailProvider) -> Result<i64, String> {
    let failures: i64 = connection
        .query_row(
            "SELECT consecutive_failures FROM inbox_watcher_state WHERE provider = ?1",
            params![provider.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("Failed to load watcher failure count: {e}"))?
        .unwrap_or(0);
    let exponent = (failures as u32).min(6);
    let backoff = WATCHER_BASE_BACKOFF_MS.saturating_mul(1_i64 << exponent);
    Ok(backoff.min(WATCHER_MAX_BACKOFF_MS))
}

fn record_watcher_failure(
    connection: &Connection,
    provider: EmailProvider,
    error: &str,
    backoff_ms: Option<i64>,
) -> Result<(), String> {
    let now = now_ms();
    let capped_error = error.chars().take(240).collect::<String>();
    let backoff_until = backoff_ms.map(|ms| now.saturating_add(ms));
    connection
        .execute(
            "INSERT INTO inbox_watcher_state (provider, backoff_until_ms, consecutive_failures, last_error, updated_at_ms)
             VALUES (?1, ?2, 1, ?3, ?4)
             ON CONFLICT(provider) DO UPDATE SET
               backoff_until_ms = excluded.backoff_until_ms,
               consecutive_failures = inbox_watcher_state.consecutive_failures + 1,
               last_error = excluded.last_error,
               updated_at_ms = excluded.updated_at_ms",
            params![provider.as_str(), backoff_until, capped_error, now],
        )
        .map_err(|e| format!("Failed to persist watcher failure state: {e}"))?;
    Ok(())
}

fn clear_watcher_backoff(connection: &Connection, provider: EmailProvider) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO inbox_watcher_state (provider, backoff_until_ms, consecutive_failures, last_error, updated_at_ms)
             VALUES (?1, NULL, 0, NULL, ?2)
             ON CONFLICT(provider) DO UPDATE SET
               backoff_until_ms = NULL,
               consecutive_failures = 0,
               last_error = NULL,
               updated_at_ms = excluded.updated_at_ms",
            params![provider.as_str(), now_ms()],
        )
        .map_err(|e| format!("Failed to clear watcher backoff state: {e}"))?;
    Ok(())
}

fn is_rate_limited_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("rate-limit") || lower.contains("rate limited")
}

fn is_retryable_watcher_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("try again")
        || lower.contains("temporarily")
        || lower.contains("could not read")
        || lower.contains("could not parse")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::bootstrap_schema;
    use rusqlite::Connection;

    #[test]
    fn parses_gmail_batch_response() {
        let boundary = "batch_abc";
        let content_type = format!("multipart/mixed; boundary={boundary}");
        let body = format!(
            "--{b}\r\nContent-Type: application/http\r\n\r\nHTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{{\"id\":\"m1\",\"threadId\":\"t1\",\"snippet\":\"hello\",\"internalDate\":\"1700000000000\",\"payload\":{{\"headers\":[{{\"name\":\"Subject\",\"value\":\"Hi\"}},{{\"name\":\"From\",\"value\":\"Jane <jane@example.com>\"}}]}}}}\r\n--{b}--\r\n",
            b = boundary
        );
        let rows = parse_gmail_batch_response(&content_type, &body).expect("parse batch");
        assert_eq!(rows.len(), 1);
        let msg = gmail_message_from_details(&rows[0]).expect("message");
        assert_eq!(msg.provider_message_id, "m1");
        assert_eq!(msg.subject, "Hi");
        assert_eq!(msg.sender_email.as_deref(), Some("jane@example.com"));
    }

    #[test]
    fn watcher_backoff_state_increments_and_clears() {
        let mut conn = Connection::open_in_memory().expect("db");
        bootstrap_schema(&mut conn).expect("schema");

        let first_backoff = next_backoff_ms(&conn, EmailProvider::Gmail).expect("backoff");
        assert_eq!(first_backoff, WATCHER_BASE_BACKOFF_MS);

        record_watcher_failure(
            &conn,
            EmailProvider::Gmail,
            "Gmail inbox is rate-limited right now.",
            Some(first_backoff),
        )
        .expect("record failure");
        let stored = watcher_backoff_until(&conn, EmailProvider::Gmail).expect("state");
        assert!(stored.is_some());

        let second_backoff = next_backoff_ms(&conn, EmailProvider::Gmail).expect("next backoff");
        assert!(second_backoff >= first_backoff);

        clear_watcher_backoff(&conn, EmailProvider::Gmail).expect("clear");
        let cleared = watcher_backoff_until(&conn, EmailProvider::Gmail).expect("state");
        assert!(cleared.is_none());
    }
}
