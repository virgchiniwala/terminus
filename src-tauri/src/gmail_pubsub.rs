use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use chrono::DateTime;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailPubSubStatus {
    pub provider: String,
    pub status: String,
    pub trigger_mode: String,
    pub watch_expiration_ms: Option<i64>,
    pub history_id: Option<String>,
    pub topic_name: Option<String>,
    pub subscription_name: Option<String>,
    pub callback_mode: String,
    pub last_event_at_ms: Option<i64>,
    pub last_error: Option<String>,
    pub consecutive_failures: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailPubSubEventRecord {
    pub id: String,
    pub provider: String,
    pub message_id: Option<String>,
    pub event_dedupe_key: String,
    pub history_id: Option<String>,
    pub published_at_ms: Option<i64>,
    pub received_at_ms: i64,
    pub status: String,
    pub failure_reason: Option<String>,
    pub created_run_count: i64,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct GmailPubSubEventInsert {
    pub id: String,
    pub provider: String,
    pub message_id: Option<String>,
    pub event_dedupe_key: String,
    pub history_id: Option<String>,
    pub published_at_ms: Option<i64>,
    pub received_at_ms: i64,
    pub status: String,
    pub failure_reason: Option<String>,
    pub created_run_count: i64,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct GmailPubSubEnvelope {
    pub message_id: String,
    pub dedupe_key: String,
    pub history_id: Option<String>,
    pub published_at_ms: Option<i64>,
}

pub fn get_status(connection: &Connection) -> Result<GmailPubSubStatus, String> {
    let row = connection
        .query_row(
            "SELECT provider, status, trigger_mode, watch_expiration_ms, history_id, topic_name,
                    subscription_name, callback_mode, last_event_at_ms, last_error,
                    consecutive_failures, updated_at_ms
             FROM gmail_pubsub_state WHERE provider = 'gmail'",
            [],
            |r| {
                Ok(GmailPubSubStatus {
                    provider: r.get(0)?,
                    status: r.get(1)?,
                    trigger_mode: r.get(2)?,
                    watch_expiration_ms: r.get(3)?,
                    history_id: r.get(4)?,
                    topic_name: r.get(5)?,
                    subscription_name: r.get(6)?,
                    callback_mode: r.get(7)?,
                    last_event_at_ms: r.get(8)?,
                    last_error: r.get(9)?,
                    consecutive_failures: r.get(10)?,
                    updated_at_ms: r.get(11)?,
                })
            },
        )
        .optional()
        .map_err(|e| format!("Failed to load Gmail PubSub status: {e}"))?;
    Ok(row.unwrap_or_else(default_status))
}

pub fn upsert_state(
    connection: &Connection,
    status: &str,
    trigger_mode: &str,
    topic_name: Option<&str>,
    subscription_name: Option<&str>,
    callback_mode: &str,
    watch_expiration_ms: Option<i64>,
    history_id: Option<&str>,
    last_error: Option<&str>,
    consecutive_failures: i64,
    now: i64,
) -> Result<GmailPubSubStatus, String> {
    connection
        .execute(
            "INSERT INTO gmail_pubsub_state (
               provider, status, trigger_mode, watch_expiration_ms, history_id, topic_name,
               subscription_name, callback_mode, last_event_at_ms, last_error, consecutive_failures, updated_at_ms
             ) VALUES ('gmail', ?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?9, ?10)
             ON CONFLICT(provider) DO UPDATE SET
               status = excluded.status,
               trigger_mode = excluded.trigger_mode,
               watch_expiration_ms = excluded.watch_expiration_ms,
               history_id = excluded.history_id,
               topic_name = excluded.topic_name,
               subscription_name = excluded.subscription_name,
               callback_mode = excluded.callback_mode,
               last_error = excluded.last_error,
               consecutive_failures = excluded.consecutive_failures,
               updated_at_ms = excluded.updated_at_ms",
            params![
                status,
                trigger_mode,
                watch_expiration_ms,
                history_id,
                topic_name,
                subscription_name,
                callback_mode,
                last_error,
                consecutive_failures,
                now
            ],
        )
        .map_err(|e| format!("Failed to persist Gmail PubSub status: {e}"))?;
    get_status(connection)
}

pub fn update_watch_success(
    connection: &Connection,
    watch_expiration_ms: Option<i64>,
    history_id: Option<&str>,
    now: i64,
) -> Result<GmailPubSubStatus, String> {
    connection
        .execute(
            "UPDATE gmail_pubsub_state
             SET status = 'active',
                 watch_expiration_ms = ?1,
                 history_id = COALESCE(?2, history_id),
                 last_error = NULL,
                 consecutive_failures = 0,
                 updated_at_ms = ?3
             WHERE provider = 'gmail'",
            params![watch_expiration_ms, history_id, now],
        )
        .map_err(|e| format!("Failed to update Gmail PubSub watch status: {e}"))?;
    get_status(connection)
}

pub fn touch_event_success(
    connection: &Connection,
    now: i64,
    history_id: Option<&str>,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE gmail_pubsub_state
             SET last_event_at_ms = ?1,
                 history_id = COALESCE(?2, history_id),
                 last_error = NULL,
                 consecutive_failures = 0,
                 updated_at_ms = ?1
             WHERE provider = 'gmail'",
            params![now, history_id],
        )
        .map_err(|e| format!("Failed to update Gmail PubSub event state: {e}"))?;
    Ok(())
}

pub fn record_failure(connection: &Connection, reason: &str, now: i64) -> Result<(), String> {
    connection
        .execute(
            "UPDATE gmail_pubsub_state
             SET status = CASE
                   WHEN watch_expiration_ms IS NOT NULL AND watch_expiration_ms < ?1 THEN 'expired'
                   ELSE 'error'
                 END,
                 last_error = ?2,
                 consecutive_failures = consecutive_failures + 1,
                 updated_at_ms = ?1
             WHERE provider = 'gmail'",
            params![now, reason],
        )
        .map_err(|e| format!("Failed to record Gmail PubSub failure: {e}"))?;
    Ok(())
}

pub fn maybe_mark_expired(connection: &Connection, now: i64) -> Result<GmailPubSubStatus, String> {
    connection
        .execute(
            "UPDATE gmail_pubsub_state
             SET status = 'expired', updated_at_ms = ?1
             WHERE provider = 'gmail'
               AND status = 'active'
               AND watch_expiration_ms IS NOT NULL
               AND watch_expiration_ms < ?1",
            params![now],
        )
        .map_err(|e| format!("Failed to update Gmail PubSub expiry state: {e}"))?;
    get_status(connection)
}

pub fn insert_event(connection: &Connection, row: &GmailPubSubEventInsert) -> Result<bool, String> {
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO gmail_pubsub_events (
               id, provider, message_id, event_dedupe_key, history_id, published_at_ms,
               received_at_ms, status, failure_reason, created_run_count, created_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                row.id,
                row.provider,
                row.message_id,
                row.event_dedupe_key,
                row.history_id,
                row.published_at_ms,
                row.received_at_ms,
                row.status,
                row.failure_reason,
                row.created_run_count,
                row.created_at_ms
            ],
        )
        .map_err(|e| format!("Failed to insert Gmail PubSub event: {e}"))?;
    Ok(inserted > 0)
}

pub fn update_event_status(
    connection: &Connection,
    event_dedupe_key: &str,
    status: &str,
    failure_reason: Option<&str>,
    created_run_count: Option<i64>,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE gmail_pubsub_events
             SET status = ?1,
                 failure_reason = COALESCE(?2, failure_reason),
                 created_run_count = COALESCE(?3, created_run_count)
             WHERE provider = 'gmail' AND event_dedupe_key = ?4",
            params![status, failure_reason, created_run_count, event_dedupe_key],
        )
        .map_err(|e| format!("Failed to update Gmail PubSub event status: {e}"))?;
    Ok(())
}

pub fn list_events(
    connection: &Connection,
    limit: usize,
) -> Result<Vec<GmailPubSubEventRecord>, String> {
    let mut stmt = connection
        .prepare(
            "SELECT id, provider, message_id, event_dedupe_key, history_id, published_at_ms,
                    received_at_ms, status, failure_reason, created_run_count, created_at_ms
             FROM gmail_pubsub_events
             WHERE provider = 'gmail'
             ORDER BY received_at_ms DESC
             LIMIT ?1",
        )
        .map_err(|e| format!("Failed to prepare Gmail PubSub events query: {e}"))?;
    let rows = stmt
        .query_map(params![limit.clamp(1, 100) as i64], |r| {
            Ok(GmailPubSubEventRecord {
                id: r.get(0)?,
                provider: r.get(1)?,
                message_id: r.get(2)?,
                event_dedupe_key: r.get(3)?,
                history_id: r.get(4)?,
                published_at_ms: r.get(5)?,
                received_at_ms: r.get(6)?,
                status: r.get(7)?,
                failure_reason: r.get(8)?,
                created_run_count: r.get(9)?,
                created_at_ms: r.get(10)?,
            })
        })
        .map_err(|e| format!("Failed to query Gmail PubSub events: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("Failed to parse Gmail PubSub event row: {e}"))?);
    }
    Ok(out)
}

pub fn parse_pubsub_envelope(body_json: &str) -> Result<GmailPubSubEnvelope, String> {
    let root: Value = serde_json::from_str(body_json)
        .map_err(|_| "Gmail PubSub payload must be valid JSON.".to_string())?;
    let msg = root
        .get("message")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "Gmail PubSub payload is missing message envelope.".to_string())?;
    let message_id = msg
        .get("messageId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Gmail PubSub payload is missing message id.".to_string())?
        .to_string();
    let published_at_ms = msg
        .get("publishTime")
        .and_then(|v| v.as_str())
        .and_then(parse_rfc3339_ms);
    let attrs_history_id = msg
        .get("attributes")
        .and_then(|v| v.as_object())
        .and_then(|m| m.get("historyId"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let data_history_id = msg
        .get("data")
        .and_then(|v| v.as_str())
        .and_then(|raw| BASE64.decode(raw).ok())
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|decoded| serde_json::from_str::<Value>(&decoded).ok())
        .and_then(|v| {
            v.get("historyId")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        });
    let history_id = data_history_id.or(attrs_history_id);
    let dedupe_key = format!(
        "{}:{}",
        message_id,
        history_id.as_deref().unwrap_or("no_history")
    );
    Ok(GmailPubSubEnvelope {
        message_id,
        dedupe_key,
        history_id,
        published_at_ms,
    })
}

pub fn should_poll_gmail(status: &GmailPubSubStatus, now: i64) -> bool {
    (match status.trigger_mode.as_str() {
        "polling" => true,
        "gmail_pubsub" => !matches!(status.status.as_str(), "active" | "pending_setup"),
        "auto" => !matches!(status.status.as_str(), "active"),
        _ => true,
    }) || {
        status
            .watch_expiration_ms
            .is_some_and(|exp| exp > 0 && exp < now && status.trigger_mode != "polling")
    }
}

fn default_status() -> GmailPubSubStatus {
    GmailPubSubStatus {
        provider: "gmail".to_string(),
        status: "disabled".to_string(),
        trigger_mode: "polling".to_string(),
        watch_expiration_ms: None,
        history_id: None,
        topic_name: None,
        subscription_name: None,
        callback_mode: "relay".to_string(),
        last_event_at_ms: None,
        last_error: None,
        consecutive_failures: 0,
        updated_at_ms: 0,
    }
}

fn parse_rfc3339_ms(input: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(input)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::bootstrap_schema;

    #[test]
    fn parses_gmail_pubsub_envelope_and_history_id() {
        let body = r#"{
          "message": {
            "messageId": "msg_1",
            "publishTime": "2026-02-25T12:00:00Z",
            "data": "eyJlbWFpbEFkZHJlc3MiOiAidXNlckBleGFtcGxlLmNvbSIsICJoaXN0b3J5SWQiOiAiMTIzNDUifQ=="
          }
        }"#;
        let env = parse_pubsub_envelope(body).expect("parse");
        assert_eq!(env.message_id, "msg_1");
        assert_eq!(env.history_id.as_deref(), Some("12345"));
        assert!(env.published_at_ms.is_some());
        assert_eq!(env.dedupe_key, "msg_1:12345");
    }

    #[test]
    fn event_insert_is_idempotent_by_dedupe_key() {
        let mut conn = Connection::open_in_memory().expect("db");
        bootstrap_schema(&mut conn).expect("bootstrap");
        upsert_state(
            &conn,
            "active",
            "auto",
            Some("projects/x/topics/t"),
            Some("projects/x/subscriptions/s"),
            "relay",
            None,
            None,
            None,
            0,
            1,
        )
        .expect("state");
        let row = GmailPubSubEventInsert {
            id: "evt1".to_string(),
            provider: "gmail".to_string(),
            message_id: Some("m1".to_string()),
            event_dedupe_key: "m1:h1".to_string(),
            history_id: Some("h1".to_string()),
            published_at_ms: Some(1),
            received_at_ms: 2,
            status: "accepted".to_string(),
            failure_reason: None,
            created_run_count: 0,
            created_at_ms: 2,
        };
        assert!(insert_event(&conn, &row).expect("insert1"));
        assert!(!insert_event(&conn, &row).expect("insert2"));
    }
}
