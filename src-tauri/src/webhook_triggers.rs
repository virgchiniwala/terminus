use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookTriggerRecord {
    pub id: String,
    pub autopilot_id: String,
    pub status: String,
    pub endpoint_path: String,
    pub endpoint_url: String,
    pub signature_mode: String,
    pub description: String,
    pub max_payload_bytes: i64,
    pub allowed_content_types: Vec<String>,
    pub provider_kind: String,
    pub last_event_at_ms: Option<i64>,
    pub last_error: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub secret_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookTriggerEventRecord {
    pub id: String,
    pub trigger_id: String,
    pub delivery_id: String,
    pub event_idempotency_key: String,
    pub received_at_ms: i64,
    pub status: String,
    pub http_status: Option<i64>,
    pub headers_redacted_json: String,
    pub payload_excerpt: String,
    pub payload_hash: String,
    pub failure_reason: Option<String>,
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWebhookTriggerInput {
    pub autopilot_id: String,
    pub description: Option<String>,
    pub max_payload_bytes: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookTriggerCreateResponse {
    pub trigger: WebhookTriggerRecord,
    pub signing_secret_preview: String,
}

#[derive(Debug, Clone)]
pub struct WebhookTriggerCreateInternal {
    pub id: String,
    pub autopilot_id: String,
    pub status: String,
    pub endpoint_path: String,
    pub signature_mode: String,
    pub description: String,
    pub max_payload_bytes: i64,
    pub allowed_content_types_json: String,
    pub plan_json: String,
    pub provider_kind: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct WebhookTriggerEventInsert {
    pub id: String,
    pub trigger_id: String,
    pub delivery_id: String,
    pub event_idempotency_key: String,
    pub received_at_ms: i64,
    pub status: String,
    pub http_status: Option<i64>,
    pub headers_redacted_json: String,
    pub payload_excerpt: String,
    pub payload_hash: String,
    pub failure_reason: Option<String>,
    pub run_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WebhookTriggerRouteConfig {
    pub trigger_id: String,
    pub autopilot_id: String,
    pub status: String,
    pub signature_mode: String,
    pub max_payload_bytes: i64,
    pub allowed_content_types: Vec<String>,
    pub plan_json: String,
    pub provider_kind: String,
}

pub fn list_webhook_triggers(
    connection: &Connection,
    autopilot_id: Option<&str>,
    relay_base_url: &str,
    secret_lookup: &dyn Fn(&str) -> bool,
) -> Result<Vec<WebhookTriggerRecord>, String> {
    let mut sql = String::from(
        "SELECT id, autopilot_id, status, endpoint_path, signature_mode, description,
                max_payload_bytes, allowed_content_types_json, provider_kind,
                last_event_at_ms, last_error, created_at_ms, updated_at_ms
         FROM webhook_triggers",
    );
    if autopilot_id.is_some() {
        sql.push_str(" WHERE autopilot_id = ?1");
    }
    sql.push_str(" ORDER BY updated_at_ms DESC");

    let mut stmt = connection
        .prepare(&sql)
        .map_err(|e| format!("Failed to prepare webhook trigger list query: {e}"))?;
    let mut out = Vec::new();
    if let Some(autopilot_id) = autopilot_id {
        let rows = stmt
            .query_map(params![autopilot_id], |row| {
                map_webhook_trigger_row(row, relay_base_url, secret_lookup)
            })
            .map_err(|e| format!("Failed to query webhook triggers: {e}"))?;
        for row in rows {
            out.push(row.map_err(|e| format!("Failed to parse webhook trigger row: {e}"))?);
        }
    } else {
        let rows = stmt
            .query_map([], |row| {
                map_webhook_trigger_row(row, relay_base_url, secret_lookup)
            })
            .map_err(|e| format!("Failed to query webhook triggers: {e}"))?;
        for row in rows {
            out.push(row.map_err(|e| format!("Failed to parse webhook trigger row: {e}"))?);
        }
    }
    Ok(out)
}

pub fn get_webhook_trigger(
    connection: &Connection,
    trigger_id: &str,
    relay_base_url: &str,
    secret_lookup: &dyn Fn(&str) -> bool,
) -> Result<Option<WebhookTriggerRecord>, String> {
    connection
        .query_row(
            "SELECT id, autopilot_id, status, endpoint_path, signature_mode, description,
                    max_payload_bytes, allowed_content_types_json, provider_kind,
                    last_event_at_ms, last_error, created_at_ms, updated_at_ms
             FROM webhook_triggers WHERE id = ?1",
            params![trigger_id],
            |row| map_webhook_trigger_row(row, relay_base_url, secret_lookup),
        )
        .optional()
        .map_err(|e| format!("Failed to load webhook trigger: {e}"))
}

pub fn create_webhook_trigger(
    connection: &Connection,
    payload: &WebhookTriggerCreateInternal,
    relay_base_url: &str,
    secret_lookup: &dyn Fn(&str) -> bool,
) -> Result<WebhookTriggerRecord, String> {
    connection
        .execute(
            "INSERT INTO webhook_triggers (
               id, autopilot_id, status, endpoint_path, signature_mode, description,
               max_payload_bytes, allowed_content_types_json, plan_json, provider_kind,
               created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                payload.id,
                payload.autopilot_id,
                payload.status,
                payload.endpoint_path,
                payload.signature_mode,
                payload.description,
                payload.max_payload_bytes,
                payload.allowed_content_types_json,
                payload.plan_json,
                payload.provider_kind,
                payload.created_at_ms,
                payload.updated_at_ms,
            ],
        )
        .map_err(|e| format!("Failed to create webhook trigger: {e}"))?;
    get_webhook_trigger(connection, &payload.id, relay_base_url, secret_lookup)?
        .ok_or_else(|| "Webhook trigger was created but could not be reloaded.".to_string())
}

pub fn update_webhook_trigger_status(
    connection: &Connection,
    trigger_id: &str,
    status: &str,
    error: Option<&str>,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE webhook_triggers
             SET status = ?1,
                 last_error = ?2,
                 updated_at_ms = strftime('%s','now') * 1000
             WHERE id = ?3",
            params![status, error, trigger_id],
        )
        .map_err(|e| format!("Failed to update webhook trigger status: {e}"))?;
    Ok(())
}

pub fn get_webhook_trigger_route_config(
    connection: &Connection,
    trigger_id: &str,
) -> Result<Option<WebhookTriggerRouteConfig>, String> {
    connection
        .query_row(
            "SELECT id, autopilot_id, status, signature_mode, max_payload_bytes,
                    allowed_content_types_json, plan_json, provider_kind
             FROM webhook_triggers WHERE id = ?1",
            params![trigger_id],
            |row| {
                let content_types_json: String = row.get(5)?;
                let allowed_content_types =
                    serde_json::from_str::<Vec<String>>(&content_types_json).unwrap_or_default();
                Ok(WebhookTriggerRouteConfig {
                    trigger_id: row.get(0)?,
                    autopilot_id: row.get(1)?,
                    status: row.get(2)?,
                    signature_mode: row.get(3)?,
                    max_payload_bytes: row.get(4)?,
                    allowed_content_types,
                    plan_json: row.get(6)?,
                    provider_kind: row.get(7)?,
                })
            },
        )
        .optional()
        .map_err(|e| format!("Failed to load webhook trigger route config: {e}"))
}

pub fn list_webhook_trigger_events(
    connection: &Connection,
    trigger_id: &str,
    limit: usize,
) -> Result<Vec<WebhookTriggerEventRecord>, String> {
    let mut stmt = connection
        .prepare(
            "SELECT id, trigger_id, delivery_id, event_idempotency_key, received_at_ms, status,
                    http_status, headers_redacted_json, payload_excerpt, payload_hash, failure_reason, run_id
             FROM webhook_trigger_events
             WHERE trigger_id = ?1
             ORDER BY received_at_ms DESC
             LIMIT ?2",
        )
        .map_err(|e| format!("Failed to prepare webhook event list query: {e}"))?;
    let rows = stmt
        .query_map(params![trigger_id, limit as i64], |row| {
            Ok(WebhookTriggerEventRecord {
                id: row.get(0)?,
                trigger_id: row.get(1)?,
                delivery_id: row.get(2)?,
                event_idempotency_key: row.get(3)?,
                received_at_ms: row.get(4)?,
                status: row.get(5)?,
                http_status: row.get(6)?,
                headers_redacted_json: row.get(7)?,
                payload_excerpt: row.get(8)?,
                payload_hash: row.get(9)?,
                failure_reason: row.get(10)?,
                run_id: row.get(11)?,
            })
        })
        .map_err(|e| format!("Failed to query webhook events: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("Failed to parse webhook event row: {e}"))?);
    }
    Ok(out)
}

pub fn insert_webhook_trigger_event(
    connection: &Connection,
    payload: &WebhookTriggerEventInsert,
) -> Result<bool, String> {
    let changed = connection
        .execute(
            "INSERT OR IGNORE INTO webhook_trigger_events (
               id, trigger_id, delivery_id, event_idempotency_key, received_at_ms, status,
               http_status, headers_redacted_json, payload_excerpt, payload_hash, failure_reason, run_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                payload.id,
                payload.trigger_id,
                payload.delivery_id,
                payload.event_idempotency_key,
                payload.received_at_ms,
                payload.status,
                payload.http_status,
                payload.headers_redacted_json,
                payload.payload_excerpt,
                payload.payload_hash,
                payload.failure_reason,
                payload.run_id
            ],
        )
        .map_err(|e| format!("Failed to insert webhook trigger event: {e}"))?;
    Ok(changed > 0)
}

pub fn update_webhook_trigger_event_status(
    connection: &Connection,
    trigger_id: &str,
    event_idempotency_key: &str,
    status: &str,
    failure_reason: Option<&str>,
    run_id: Option<&str>,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE webhook_trigger_events
             SET status = ?1, failure_reason = ?2, run_id = COALESCE(?3, run_id)
             WHERE trigger_id = ?4 AND event_idempotency_key = ?5",
            params![
                status,
                failure_reason,
                run_id,
                trigger_id,
                event_idempotency_key
            ],
        )
        .map_err(|e| format!("Failed to update webhook trigger event status: {e}"))?;
    Ok(())
}

pub fn touch_webhook_trigger_delivery(
    connection: &Connection,
    trigger_id: &str,
    received_at_ms: i64,
    last_error: Option<&str>,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE webhook_triggers
             SET last_event_at_ms = ?1,
                 last_error = ?2,
                 updated_at_ms = strftime('%s','now') * 1000
             WHERE id = ?3",
            params![received_at_ms, last_error, trigger_id],
        )
        .map_err(|e| format!("Failed to touch webhook trigger delivery state: {e}"))?;
    Ok(())
}

fn map_webhook_trigger_row(
    row: &rusqlite::Row<'_>,
    relay_base_url: &str,
    secret_lookup: &dyn Fn(&str) -> bool,
) -> rusqlite::Result<WebhookTriggerRecord> {
    let id: String = row.get(0)?;
    let allowed_content_types_json: String = row.get(7)?;
    let allowed_content_types =
        serde_json::from_str::<Vec<String>>(&allowed_content_types_json).unwrap_or_default();
    Ok(WebhookTriggerRecord {
        id: id.clone(),
        autopilot_id: row.get(1)?,
        status: row.get(2)?,
        endpoint_path: row.get(3)?,
        endpoint_url: format!(
            "{}/{}",
            relay_base_url.trim_end_matches('/'),
            row.get::<_, String>(3)?.trim_start_matches('/')
        ),
        signature_mode: row.get(4)?,
        description: row.get(5)?,
        max_payload_bytes: row.get(6)?,
        allowed_content_types,
        provider_kind: row.get(8)?,
        last_event_at_ms: row.get(9)?,
        last_error: row.get(10)?,
        created_at_ms: row.get(11)?,
        updated_at_ms: row.get(12)?,
        secret_configured: secret_lookup(&id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_connection() -> Connection {
        let mut conn = Connection::open_in_memory().expect("in-memory db");
        crate::db::bootstrap_schema(&mut conn).expect("bootstrap schema");
        conn.execute(
            "INSERT INTO autopilots (id, name, created_at) VALUES (?1, ?2, ?3)",
            params!["auto_test", "Test", 1_i64],
        )
        .expect("insert autopilot");
        conn
    }

    #[test]
    fn create_list_and_toggle_webhook_trigger_round_trip() {
        let conn = setup_connection();
        let created = create_webhook_trigger(
            &conn,
            &WebhookTriggerCreateInternal {
                id: "wh_1".to_string(),
                autopilot_id: "auto_test".to_string(),
                status: "active".to_string(),
                endpoint_path: "hooks/abc".to_string(),
                signature_mode: "terminus_hmac_sha256".to_string(),
                description: "Webhook for tests".to_string(),
                max_payload_bytes: 32_768,
                allowed_content_types_json: "[\"application/json\"]".to_string(),
                plan_json: "{\"schema_version\":\"1.0\"}".to_string(),
                provider_kind: "openai".to_string(),
                created_at_ms: 10,
                updated_at_ms: 10,
            },
            "https://relay.terminus.run/webhooks",
            &|_| true,
        )
        .expect("create");
        assert_eq!(created.status, "active");
        assert!(created.secret_configured);
        assert!(created.endpoint_url.contains("/hooks/abc"));

        update_webhook_trigger_status(&conn, "wh_1", "paused", Some("Paused"))
            .expect("update status");
        let rows = list_webhook_triggers(
            &conn,
            Some("auto_test"),
            "https://relay.terminus.run/webhooks",
            &|_| false,
        )
        .expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "paused");
        assert_eq!(rows[0].last_error.as_deref(), Some("Paused"));
    }

    #[test]
    fn webhook_event_insert_is_idempotent_by_trigger_and_event_key() {
        let conn = setup_connection();
        conn.execute(
            "INSERT INTO webhook_triggers (
               id, autopilot_id, status, endpoint_path, signature_mode, description,
               max_payload_bytes, allowed_content_types_json, plan_json, provider_kind,
               created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, 'active', ?3, 'terminus_hmac_sha256', '', 32768, '[\"application/json\"]', '{}', 'openai', 1, 1)",
            params!["wh_1", "auto_test", "hooks/abc"],
        )
        .expect("insert trigger");

        let event = WebhookTriggerEventInsert {
            id: "evt_1".to_string(),
            trigger_id: "wh_1".to_string(),
            delivery_id: "delivery_1".to_string(),
            event_idempotency_key: "dedupe_1".to_string(),
            received_at_ms: 100,
            status: "accepted".to_string(),
            http_status: Some(202),
            headers_redacted_json: "{}".to_string(),
            payload_excerpt: "{\"ok\":true}".to_string(),
            payload_hash: "hash".to_string(),
            failure_reason: None,
            run_id: None,
        };
        assert!(insert_webhook_trigger_event(&conn, &event).expect("first insert"));
        let duplicate = WebhookTriggerEventInsert {
            id: "evt_2".to_string(),
            ..event.clone()
        };
        assert!(!insert_webhook_trigger_event(&conn, &duplicate).expect("duplicate insert"));

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM webhook_trigger_events WHERE trigger_id = ?1 AND event_idempotency_key = ?2",
                params!["wh_1", "dedupe_1"],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 1);
    }
}
