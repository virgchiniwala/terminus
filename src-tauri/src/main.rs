mod db;
mod diagnostics;
mod email_connections;
mod gmail_pubsub;
mod guidance_utils;
mod inbox_watcher;
mod learning;
mod missions;
mod primitives;
mod providers;
mod runner;
mod schema;
mod transport;
mod vault_spike;
mod web;
mod webhook_triggers;

use base64::Engine as _;
use guidance_utils::{
    classify_guidance, compute_missed_cycles, normalize_guidance_instruction, sanitize_log_message,
    GuidanceMode,
};
use hmac::{Hmac, Mac};
use providers::runtime::{ProviderRuntime, TransportStatus};
use providers::types::{
    ProviderErrorKind, ProviderKind as ApiProviderKind, ProviderRequest,
    ProviderTier as ApiProviderTier,
};
use reqwest::blocking::Client as HttpClient;
use runner::{ApprovalRecord, ClarificationRecord, RunReceipt, RunRecord, RunnerEngine};
use rusqlite::OptionalExtension;
use schema::{
    ApiCallRequest, AutopilotPlan, PlanStep, PrimitiveId, ProviderId, RecipeKind, RiskTier,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::menu::{MenuBuilder, MenuEvent, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
use transport::{RelayApprovalDecision, RelayTransport};
use webhook_triggers::{CreateWebhookTriggerInput, WebhookTriggerCreateResponse};

#[derive(Default)]
struct AppState {
    db_path: std::sync::Mutex<Option<PathBuf>>,
}

static MAIN_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum IntentDraftKind {
    OneOffRun,
    DraftAutopilot,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct IntentDraftPreview {
    reads: Vec<String>,
    writes: Vec<String>,
    approvals_required: Vec<String>,
    estimated_spend: String,
    primary_cta: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct IntentDraftResponse {
    kind: IntentDraftKind,
    classification_reason: String,
    plan: AutopilotPlan,
    preview: IntentDraftPreview,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunnerControlInput {
    background_enabled: bool,
    watcher_enabled: bool,
    gmail_trigger_mode: String,
    watcher_poll_seconds: i64,
    watcher_max_items: i64,
    gmail_autopilot_id: String,
    microsoft_autopilot_id: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct OnboardingStateInput {
    role_text: String,
    work_focus_text: String,
    biggest_pain_text: String,
    recommended_intent: Option<String>,
    onboarding_complete: Option<bool>,
    dismissed: Option<bool>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct VoiceConfigInput {
    tone: String,
    length: String,
    humor: String,
    notes: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutopilotVoiceConfigInput {
    autopilot_id: String,
    enabled: bool,
    tone: String,
    length: String,
    humor: String,
    notes: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunnerCycleSummary {
    watcher_status: String,
    relay_sync_status: String,
    providers_polled: usize,
    fetched: usize,
    deduped: usize,
    started_runs: usize,
    failed: usize,
    resumed_due_runs: usize,
    relay_decisions_applied: usize,
    missed_runs_detected: i64,
    catch_up_cycles_run: i64,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutopilotSendPolicyInput {
    autopilot_id: String,
    allow_sending: bool,
    recipient_allowlist: Vec<String>,
    max_sends_per_day: i64,
    quiet_hours_start_local: i64,
    quiet_hours_end_local: i64,
    allow_outside_quiet_hours: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct GuidanceInput {
    scope_type: String,
    scope_id: String,
    instruction: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GuidanceResponse {
    mode: GuidanceMode,
    message: String,
    proposed_rule: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TransportStatusResponse {
    mode: String,
    relay_configured: bool,
    relay_url: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelaySubscriberTokenInput {
    token: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyRefInput {
    ref_name: String,
    secret: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyRefDeleteInput {
    ref_name: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct VaultExtractionProbeInput {
    path: String,
    max_preview_chars: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyRefStatus {
    ref_name: String,
    configured: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CodexOauthStatusResponse {
    configured: bool,
    local_auth_found: bool,
    local_auth_path: String,
    local_auth_mode: Option<String>,
    imported_auth_mode: Option<String>,
    imported_at_ms: Option<i64>,
    last_refresh: Option<String>,
    has_refresh_token: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalResolutionContextInput {
    approval_id: String,
    actor_label: Option<String>,
    channel: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelayApprovalCallbackInput {
    request_id: String,
    approval_id: String,
    decision: String, // approve|reject
    callback_secret: String,
    actor_label: Option<String>,
    channel: Option<String>,
    reason: Option<String>,
    issued_at_ms: i64,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelayWebhookCallbackInput {
    request_id: String,
    callback_secret: String,
    issued_at_ms: i64,
    trigger_id: String,
    delivery_id: String,
    content_type: String,
    body_json: String,
    signature: String,
    signature_ts_ms: i64,
    headers_redacted_json: Option<String>,
    channel: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebhookEventLocalDebugInput {
    trigger_id: String,
    delivery_id: String,
    body_json: String,
    content_type: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct GmailPubSubEnableInput {
    trigger_mode: Option<String>, // polling|gmail_pubsub|auto
    topic_name: String,
    subscription_name: String,
    callback_mode: Option<String>, // relay|local_debug
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelayGmailPubSubCallbackInput {
    request_id: String,
    callback_secret: String,
    issued_at_ms: i64,
    body_json: String,
    channel: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct GmailPubSubLocalDebugInput {
    body_json: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GmailPubSubIngestResult {
    status: String,
    event_dedupe_key: String,
    created_run_count: i64,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebhookIngestResult {
    status: String,
    trigger_id: String,
    delivery_id: String,
    run_id: Option<String>,
    message: String,
}

#[derive(Debug, Clone)]
struct WebhookIngestInput {
    relay_request_id: Option<String>,
    relay_callback_secret: Option<String>,
    relay_issued_at_ms: Option<i64>,
    trigger_id: String,
    delivery_id: String,
    content_type: String,
    body_json: String,
    signature: Option<String>,
    signature_ts_ms: Option<i64>,
    headers_redacted_json: Option<String>,
    relay_channel: Option<String>,
    require_relay_callback_auth: bool,
    require_webhook_signature: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RemoteApprovalReadinessResponse {
    transport_mode: String,
    relay_configured: bool,
    relay_url: String,
    callback_ready: bool,
    device_id: String,
    pending_approvals: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RelayCallbackSecretIssuedResponse {
    readiness: RemoteApprovalReadinessResponse,
    callback_secret: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RelayApprovalSyncStatusResponse {
    channel: String,
    enabled: bool,
    relay_configured: bool,
    callback_ready: bool,
    device_id: String,
    status: String,
    last_poll_at_ms: Option<i64>,
    last_success_at_ms: Option<i64>,
    consecutive_failures: i64,
    backoff_until_ms: Option<i64>,
    last_error: Option<String>,
    last_processed_count: i64,
    total_processed_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RelayApprovalSyncTickResponse {
    status: RelayApprovalSyncStatusResponse,
    applied_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RelayDeviceRecord {
    device_id: String,
    device_label: String,
    status: String,
    last_seen_at_ms: Option<i64>,
    capabilities_json: String,
    is_preferred_target: bool,
    updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RelayRoutingPolicyResponse {
    approval_target_mode: String,
    trigger_target_mode: String,
    fallback_policy: String,
    updated_at_ms: i64,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelayDeviceStatusInput {
    device_id: String,
    status: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelayRoutingPolicyInput {
    approval_target_mode: String,
    trigger_target_mode: String,
    fallback_policy: String,
}

fn open_connection(state: &tauri::State<AppState>) -> Result<rusqlite::Connection, String> {
    let db_path = state
        .db_path
        .lock()
        .map_err(|_| "Failed to access app state".to_string())?
        .clone()
        .ok_or_else(|| "Database is not initialized yet".to_string())?;

    let mut connection = rusqlite::Connection::open(db_path)
        .map_err(|e| format!("Failed to open sqlite db: {e}"))?;
    db::configure_connection(&connection)?;
    db::bootstrap_schema(&mut connection)?;
    Ok(connection)
}

fn open_connection_from_path(db_path: &PathBuf) -> Result<rusqlite::Connection, String> {
    let mut connection = rusqlite::Connection::open(db_path)
        .map_err(|e| format!("Failed to open sqlite db: {e}"))?;
    db::configure_connection(&connection)?;
    db::bootstrap_schema(&mut connection)?;
    Ok(connection)
}

#[tauri::command]
fn get_home_snapshot(state: tauri::State<AppState>) -> Result<db::HomeSnapshot, String> {
    let db_path = state
        .db_path
        .lock()
        .map_err(|_| "Failed to access app state".to_string())?
        .clone()
        .ok_or_else(|| "Database is not initialized yet".to_string())?;

    db::get_home_snapshot(db_path)
}

#[tauri::command]
fn list_primary_outcomes(
    state: tauri::State<AppState>,
    limit: Option<usize>,
) -> Result<Vec<db::PrimaryOutcomeRecord>, String> {
    let connection = open_connection(&state)?;
    db::list_primary_outcomes(&connection, limit.unwrap_or(50))
}

#[tauri::command]
fn get_remote_approval_readiness(
    state: tauri::State<AppState>,
) -> Result<RemoteApprovalReadinessResponse, String> {
    let status = ProviderRuntime::default().transport_status();
    let callback_ready = providers::keychain::get_relay_callback_secret()
        .map_err(|e| e.to_string())?
        .is_some_and(|v| !v.trim().is_empty());
    let connection = open_connection(&state)?;
    let device_id = ensure_local_relay_device_registered(&connection)?;
    let pending_approvals = RunnerEngine::list_pending_approvals(&connection)
        .map_err(|e| e.to_string())?
        .len();
    Ok(RemoteApprovalReadinessResponse {
        transport_mode: status.mode.as_str().to_string(),
        relay_configured: status.relay_configured,
        relay_url: status.relay_url,
        callback_ready,
        device_id,
        pending_approvals,
    })
}

#[tauri::command]
fn issue_relay_callback_secret(
    state: tauri::State<AppState>,
) -> Result<RelayCallbackSecretIssuedResponse, String> {
    let secret = generate_secret_token("relaycb");
    providers::keychain::set_relay_callback_secret(&secret).map_err(|e| e.to_string())?;
    let readiness = get_remote_approval_readiness(state)?;
    Ok(RelayCallbackSecretIssuedResponse {
        readiness,
        callback_secret: secret,
    })
}

#[tauri::command]
fn clear_relay_callback_secret(
    state: tauri::State<AppState>,
) -> Result<RemoteApprovalReadinessResponse, String> {
    providers::keychain::delete_relay_callback_secret().map_err(|e| e.to_string())?;
    get_remote_approval_readiness(state)
}

fn normalize_relay_device_status(input: &str) -> Result<String, String> {
    match input.trim().to_ascii_lowercase().as_str() {
        "active" | "standby" | "offline" | "disabled" => Ok(input.trim().to_ascii_lowercase()),
        _ => Err("Relay device status must be Active, Standby, Offline, or Disabled.".to_string()),
    }
}

fn normalize_relay_target_mode(input: &str) -> Result<String, String> {
    match input.trim().to_ascii_lowercase().as_str() {
        "preferred_only" | "manual_target_only" => Ok(input.trim().to_ascii_lowercase()),
        _ => Err(
            "Relay routing target mode must be Preferred Only or Manual Target Only.".to_string(),
        ),
    }
}

fn normalize_relay_fallback_policy(input: &str) -> Result<String, String> {
    match input.trim().to_ascii_lowercase().as_str() {
        "queue_until_online" | "fallback_to_standby" => Ok(input.trim().to_ascii_lowercase()),
        _ => Err(
            "Relay fallback policy must be Queue Until Online or Fallback To Standby.".to_string(),
        ),
    }
}

fn local_relay_device_label() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "This Mac".to_string())
}

fn upsert_relay_device(
    connection: &rusqlite::Connection,
    device_id: &str,
    device_label: &str,
    status: &str,
    is_preferred_target: bool,
    touch_last_seen: bool,
) -> Result<(), String> {
    let now = now_ms();
    let existing_status: Option<String> = connection
        .query_row(
            "SELECT status FROM relay_devices WHERE device_id = ?1",
            rusqlite::params![device_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("Could not read relay device: {e}"))?;
    let status_to_write = existing_status.unwrap_or_else(|| status.to_string());
    let last_seen = if touch_last_seen { Some(now) } else { None };
    connection
        .execute(
            "INSERT INTO relay_devices (
                device_id, device_label, status, last_seen_at_ms, capabilities_json, is_preferred_target, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(device_id) DO UPDATE SET
                device_label = excluded.device_label,
                status = COALESCE(relay_devices.status, excluded.status),
                last_seen_at_ms = COALESCE(excluded.last_seen_at_ms, relay_devices.last_seen_at_ms),
                capabilities_json = excluded.capabilities_json,
                updated_at_ms = excluded.updated_at_ms",
            rusqlite::params![
                device_id,
                device_label,
                status_to_write,
                last_seen,
                r#"{"relayPush":true,"relayCallback":true}"#,
                if is_preferred_target { 1 } else { 0 },
                now
            ],
        )
        .map_err(|e| format!("Could not upsert relay device: {e}"))?;
    Ok(())
}

fn ensure_local_relay_device_registered(
    connection: &rusqlite::Connection,
) -> Result<String, String> {
    let device_id = ensure_relay_device_id().map_err(|e| e.to_string())?;
    let existing_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM relay_devices", [], |row| row.get(0))
        .map_err(|e| format!("Could not count relay devices: {e}"))?;
    let existing_pref: Option<i64> = connection
        .query_row(
            "SELECT is_preferred_target FROM relay_devices WHERE device_id = ?1",
            rusqlite::params![&device_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("Could not read relay device preference: {e}"))?;
    upsert_relay_device(
        connection,
        &device_id,
        &local_relay_device_label(),
        "active",
        existing_pref.unwrap_or(if existing_count == 0 { 1 } else { 0 }) != 0,
        true,
    )?;
    Ok(device_id)
}

fn list_relay_devices_internal(
    connection: &rusqlite::Connection,
) -> Result<Vec<RelayDeviceRecord>, String> {
    let mut stmt = connection
        .prepare(
            "SELECT device_id, device_label, status, last_seen_at_ms, capabilities_json, is_preferred_target, updated_at_ms
             FROM relay_devices
             ORDER BY is_preferred_target DESC, updated_at_ms DESC, device_label ASC",
        )
        .map_err(|e| format!("Could not prepare relay devices query: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(RelayDeviceRecord {
                device_id: row.get(0)?,
                device_label: row.get(1)?,
                status: row.get(2)?,
                last_seen_at_ms: row.get(3)?,
                capabilities_json: row.get(4)?,
                is_preferred_target: row.get::<_, i64>(5)? != 0,
                updated_at_ms: row.get(6)?,
            })
        })
        .map_err(|e| format!("Could not read relay devices: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("Could not parse relay device row: {e}"))?);
    }
    Ok(out)
}

fn get_relay_routing_policy_internal(
    connection: &rusqlite::Connection,
) -> Result<RelayRoutingPolicyResponse, String> {
    connection
        .query_row(
            "SELECT approval_target_mode, trigger_target_mode, fallback_policy, updated_at_ms
             FROM relay_routing_policy WHERE singleton_id = 1",
            [],
            |row| {
                Ok(RelayRoutingPolicyResponse {
                    approval_target_mode: row.get(0)?,
                    trigger_target_mode: row.get(1)?,
                    fallback_policy: row.get(2)?,
                    updated_at_ms: row.get(3)?,
                })
            },
        )
        .map_err(|e| format!("Could not read relay routing policy: {e}"))
}

fn relay_local_execution_allowed(
    connection: &rusqlite::Connection,
    local_device_id: &str,
    channel: RelayDecisionSyncChannel,
) -> Result<Option<String>, String> {
    let policy = get_relay_routing_policy_internal(connection)?;
    let mode = match channel {
        RelayDecisionSyncChannel::Poll | RelayDecisionSyncChannel::Push => {
            policy.approval_target_mode.as_str()
        }
    };
    let local: Option<(String, i64)> = connection
        .query_row(
            "SELECT status, is_preferred_target FROM relay_devices WHERE device_id = ?1",
            rusqlite::params![local_device_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| format!("Could not read local relay device status: {e}"))?;
    let Some((status, preferred_flag)) = local else {
        return Ok(Some("Relay device is not registered yet.".to_string()));
    };
    if status == "disabled" {
        return Ok(Some(
            "This device is disabled for relay routing. Re-enable it in Relay Devices.".to_string(),
        ));
    }
    if status == "offline" {
        return Ok(Some(
            "This device is marked offline for relay routing. Set it to Active to receive relay decisions.".to_string(),
        ));
    }
    if mode == "manual_target_only" {
        return Ok(Some(
            "Relay routing is set to manual target only. This device will not pull decisions automatically.".to_string(),
        ));
    }
    if preferred_flag == 0 {
        let preferred: Option<String> = connection
            .query_row(
                "SELECT device_label FROM relay_devices
                 WHERE is_preferred_target = 1 AND status = 'active'
                 ORDER BY updated_at_ms DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("Could not read preferred relay device: {e}"))?;
        if let Some(label) = preferred {
            return Ok(Some(format!(
                "This device is standby. Relay decisions are routed to preferred device {label}."
            )));
        }
    }
    Ok(None)
}

#[tauri::command]
fn list_relay_devices(state: tauri::State<AppState>) -> Result<Vec<RelayDeviceRecord>, String> {
    let connection = open_connection(&state)?;
    let _ = ensure_local_relay_device_registered(&connection)?;
    list_relay_devices_internal(&connection)
}

#[tauri::command]
fn get_relay_routing_policy(
    state: tauri::State<AppState>,
) -> Result<RelayRoutingPolicyResponse, String> {
    let connection = open_connection(&state)?;
    let _ = ensure_local_relay_device_registered(&connection)?;
    get_relay_routing_policy_internal(&connection)
}

#[tauri::command]
fn set_relay_device_status(
    state: tauri::State<AppState>,
    input: RelayDeviceStatusInput,
) -> Result<Vec<RelayDeviceRecord>, String> {
    let connection = open_connection(&state)?;
    let _ = ensure_local_relay_device_registered(&connection)?;
    let status = normalize_relay_device_status(&input.status)?;
    let now = now_ms();
    let affected = connection
        .execute(
            "UPDATE relay_devices SET status = ?1, updated_at_ms = ?2 WHERE device_id = ?3",
            rusqlite::params![status, now, input.device_id.trim()],
        )
        .map_err(|e| format!("Could not update relay device status: {e}"))?;
    if affected == 0 {
        return Err("Relay device was not found.".to_string());
    }
    list_relay_devices_internal(&connection)
}

#[tauri::command]
fn set_preferred_relay_device(
    state: tauri::State<AppState>,
    device_id: String,
) -> Result<Vec<RelayDeviceRecord>, String> {
    let connection = open_connection(&state)?;
    let _ = ensure_local_relay_device_registered(&connection)?;
    let target = device_id.trim();
    if target.is_empty() {
        return Err("Relay device id is required.".to_string());
    }
    let exists: Option<String> = connection
        .query_row(
            "SELECT device_id FROM relay_devices WHERE device_id = ?1",
            rusqlite::params![target],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("Could not read relay device: {e}"))?;
    if exists.is_none() {
        return Err("Relay device was not found.".to_string());
    }
    connection
        .execute("UPDATE relay_devices SET is_preferred_target = 0", [])
        .map_err(|e| format!("Could not clear preferred relay device: {e}"))?;
    connection.execute(
        "UPDATE relay_devices SET is_preferred_target = 1, updated_at_ms = ?1 WHERE device_id = ?2",
        rusqlite::params![now_ms(), target],
    )
    .map_err(|e| format!("Could not set preferred relay device: {e}"))?;
    list_relay_devices_internal(&connection)
}

#[tauri::command]
fn update_relay_routing_policy(
    state: tauri::State<AppState>,
    input: RelayRoutingPolicyInput,
) -> Result<RelayRoutingPolicyResponse, String> {
    let connection = open_connection(&state)?;
    let _ = ensure_local_relay_device_registered(&connection)?;
    let approval_target_mode = normalize_relay_target_mode(&input.approval_target_mode)?;
    let trigger_target_mode = normalize_relay_target_mode(&input.trigger_target_mode)?;
    let fallback_policy = normalize_relay_fallback_policy(&input.fallback_policy)?;
    connection
        .execute(
            "UPDATE relay_routing_policy
             SET approval_target_mode = ?1,
                 trigger_target_mode = ?2,
                 fallback_policy = ?3,
                 updated_at_ms = ?4
             WHERE singleton_id = 1",
            rusqlite::params![
                approval_target_mode,
                trigger_target_mode,
                fallback_policy,
                now_ms()
            ],
        )
        .map_err(|e| format!("Could not update relay routing policy: {e}"))?;
    get_relay_routing_policy_internal(&connection)
}

#[derive(Debug, Clone, Default)]
struct RelaySyncStateRow {
    last_poll_at_ms: Option<i64>,
    last_success_at_ms: Option<i64>,
    consecutive_failures: i64,
    backoff_until_ms: Option<i64>,
    last_error: Option<String>,
    last_processed_count: i64,
    total_processed_count: i64,
}

#[derive(Debug, Clone, Copy)]
enum RelayDecisionSyncChannel {
    Poll,
    Push,
}

impl RelayDecisionSyncChannel {
    fn as_row_key(&self) -> &'static str {
        match self {
            Self::Poll => "approval_sync",
            Self::Push => "approval_push",
        }
    }

    fn as_api_label(&self) -> &'static str {
        match self {
            Self::Poll => "poll",
            Self::Push => "push",
        }
    }
}

#[tauri::command]
fn get_relay_sync_status(
    state: tauri::State<AppState>,
) -> Result<RelayApprovalSyncStatusResponse, String> {
    let connection = open_connection(&state)?;
    get_relay_sync_status_internal(&connection, RelayDecisionSyncChannel::Poll)
}

#[tauri::command]
fn get_relay_push_status(
    state: tauri::State<AppState>,
) -> Result<RelayApprovalSyncStatusResponse, String> {
    let connection = open_connection(&state)?;
    get_relay_sync_status_internal(&connection, RelayDecisionSyncChannel::Push)
}

#[tauri::command]
fn tick_relay_approval_sync(
    state: tauri::State<AppState>,
) -> Result<RelayApprovalSyncTickResponse, String> {
    let mut connection = open_connection(&state)?;
    tick_relay_approval_sync_internal(&mut connection, true, RelayDecisionSyncChannel::Poll)
}

#[tauri::command]
fn tick_relay_approval_push(
    state: tauri::State<AppState>,
) -> Result<RelayApprovalSyncTickResponse, String> {
    let mut connection = open_connection(&state)?;
    tick_relay_approval_sync_internal(&mut connection, true, RelayDecisionSyncChannel::Push)
}

fn get_relay_sync_status_internal(
    connection: &rusqlite::Connection,
    channel: RelayDecisionSyncChannel,
) -> Result<RelayApprovalSyncStatusResponse, String> {
    let transport = ProviderRuntime::default().transport_status();
    let relay_configured = transport.relay_configured;
    let callback_ready = providers::keychain::get_relay_callback_secret()
        .map_err(|e| e.to_string())?
        .is_some_and(|v| !v.trim().is_empty());
    let device_id = ensure_local_relay_device_registered(connection)?;
    let state = load_relay_sync_state(connection, channel)?;
    let routing_block_reason = relay_local_execution_allowed(connection, &device_id, channel)?;
    let enabled = relay_configured && callback_ready;
    let now = now_ms();
    let status = if !relay_configured {
        "relay_not_configured"
    } else if !callback_ready {
        "callback_not_ready"
    } else if routing_block_reason.is_some() {
        "device_not_target"
    } else if state.backoff_until_ms.is_some_and(|until| until > now) {
        "backoff"
    } else if state
        .last_error
        .as_ref()
        .is_some_and(|err| !err.trim().is_empty())
        && state.last_success_at_ms.is_none()
    {
        "error"
    } else if state.last_success_at_ms.is_some() {
        "ready"
    } else {
        "idle"
    };
    Ok(RelayApprovalSyncStatusResponse {
        channel: channel.as_api_label().to_string(),
        enabled,
        relay_configured,
        callback_ready,
        device_id,
        status: status.to_string(),
        last_poll_at_ms: state.last_poll_at_ms,
        last_success_at_ms: state.last_success_at_ms,
        consecutive_failures: state.consecutive_failures,
        backoff_until_ms: state.backoff_until_ms,
        last_error: routing_block_reason.or(state.last_error),
        last_processed_count: state.last_processed_count,
        total_processed_count: state.total_processed_count,
    })
}

fn tick_relay_approval_sync_internal(
    connection: &mut rusqlite::Connection,
    manual: bool,
    channel: RelayDecisionSyncChannel,
) -> Result<RelayApprovalSyncTickResponse, String> {
    let status = ProviderRuntime::default().transport_status();
    let relay_token = providers::keychain::get_relay_subscriber_token()
        .map_err(|e| e.to_string())?
        .filter(|v| !v.trim().is_empty());
    let callback_secret = providers::keychain::get_relay_callback_secret()
        .map_err(|e| e.to_string())?
        .filter(|v| !v.trim().is_empty());
    let device_id = ensure_local_relay_device_registered(connection)?;
    let mut sync_state = load_relay_sync_state(connection, channel)?;
    let now = now_ms();

    if relay_token.is_none() || !status.relay_configured {
        sync_state.last_error = None;
        sync_state.backoff_until_ms = None;
        persist_relay_sync_state(connection, channel, &sync_state, now)?;
        return Ok(RelayApprovalSyncTickResponse {
            status: get_relay_sync_status_internal(connection, channel)?,
            applied_count: 0,
        });
    }
    if callback_secret.is_none() {
        sync_state.last_error = Some(
            "Remote approvals are not ready yet. Generate a callback secret first.".to_string(),
        );
        persist_relay_sync_state(connection, channel, &sync_state, now)?;
        return Ok(RelayApprovalSyncTickResponse {
            status: get_relay_sync_status_internal(connection, channel)?,
            applied_count: 0,
        });
    }
    if let Some(reason) = relay_local_execution_allowed(connection, &device_id, channel)? {
        sync_state.last_error = Some(reason);
        sync_state.last_processed_count = 0;
        sync_state.backoff_until_ms = None;
        persist_relay_sync_state(connection, channel, &sync_state, now)?;
        return Ok(RelayApprovalSyncTickResponse {
            status: get_relay_sync_status_internal(connection, channel)?,
            applied_count: 0,
        });
    }
    if !manual && sync_state.backoff_until_ms.is_some_and(|until| until > now) {
        return Ok(RelayApprovalSyncTickResponse {
            status: get_relay_sync_status_internal(connection, channel)?,
            applied_count: 0,
        });
    }

    sync_state.last_poll_at_ms = Some(now);
    persist_relay_sync_state(connection, channel, &sync_state, now)?;

    let relay = RelayTransport::new(RelayTransport::default_url());
    let poll_result = match channel {
        RelayDecisionSyncChannel::Poll => relay.poll_approval_decisions(
            relay_token.as_deref().unwrap_or_default(),
            &device_id,
            20,
        ),
        RelayDecisionSyncChannel::Push => relay
            .stream_approval_decisions(
                relay_token.as_deref().unwrap_or_default(),
                &device_id,
                20,
                20,
            )
            .or_else(|stream_err| {
                if stream_err.is_retryable() {
                    Err(stream_err)
                } else {
                    relay.poll_approval_decisions(
                        relay_token.as_deref().unwrap_or_default(),
                        &device_id,
                        20,
                    )
                }
            }),
    };

    let mut applied_count = 0usize;
    match poll_result {
        Ok(payload) => {
            for decision in payload.decisions {
                if apply_relay_polled_decision(
                    connection,
                    &decision,
                    callback_secret.as_deref().unwrap_or_default(),
                )?
                .is_some()
                {
                    applied_count += 1;
                }
            }
            sync_state.last_success_at_ms = Some(now_ms());
            sync_state.consecutive_failures = 0;
            sync_state.backoff_until_ms = None;
            sync_state.last_error = None;
            sync_state.last_processed_count = applied_count as i64;
            sync_state.total_processed_count = sync_state
                .total_processed_count
                .saturating_add(applied_count as i64);
            persist_relay_sync_state(connection, channel, &sync_state, now_ms())?;
        }
        Err(err) => {
            sync_state.consecutive_failures = sync_state.consecutive_failures.saturating_add(1);
            sync_state.last_error = Some(err.message.clone());
            sync_state.last_processed_count = 0;
            if matches!(err.kind, ProviderErrorKind::Retryable) {
                let base = 5_000_i64;
                let step = (sync_state.consecutive_failures - 1).clamp(0, 5) as u32;
                let delay = base.saturating_mul(2_i64.saturating_pow(step));
                sync_state.backoff_until_ms = Some(now.saturating_add(delay.min(300_000)));
            } else {
                sync_state.backoff_until_ms = None;
            }
            persist_relay_sync_state(connection, channel, &sync_state, now)?;
        }
    }

    Ok(RelayApprovalSyncTickResponse {
        status: get_relay_sync_status_internal(connection, channel)?,
        applied_count,
    })
}

fn apply_relay_polled_decision(
    connection: &mut rusqlite::Connection,
    decision: &RelayApprovalDecision,
    callback_secret: &str,
) -> Result<Option<RunRecord>, String> {
    if decision.request_id.trim().is_empty() || decision.approval_id.trim().is_empty() {
        return Ok(None);
    }
    let input = RelayApprovalCallbackInput {
        request_id: decision.request_id.clone(),
        approval_id: decision.approval_id.clone(),
        decision: decision.decision.clone(),
        callback_secret: callback_secret.to_string(),
        actor_label: decision.actor_label.clone(),
        channel: decision.channel.clone(),
        reason: decision.reason.clone(),
        issued_at_ms: decision.issued_at_ms,
    };
    resolve_relay_approval_callback_with_connection(connection, &input).map(Some)
}

fn load_relay_sync_state(
    connection: &rusqlite::Connection,
    channel: RelayDecisionSyncChannel,
) -> Result<RelaySyncStateRow, String> {
    connection
        .query_row(
            "SELECT last_poll_at_ms, last_success_at_ms, consecutive_failures, backoff_until_ms,
                    last_error, last_processed_count, total_processed_count
             FROM relay_sync_state WHERE channel = ?1 LIMIT 1",
            rusqlite::params![channel.as_row_key()],
            |row| {
                Ok(RelaySyncStateRow {
                    last_poll_at_ms: row.get(0)?,
                    last_success_at_ms: row.get(1)?,
                    consecutive_failures: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    backoff_until_ms: row.get(3)?,
                    last_error: row.get(4)?,
                    last_processed_count: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    total_processed_count: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                })
            },
        )
        .optional()
        .map_err(|e| format!("Could not read relay sync status: {e}"))?
        .map_or_else(|| Ok(RelaySyncStateRow::default()), Ok)
}

fn persist_relay_sync_state(
    connection: &rusqlite::Connection,
    channel: RelayDecisionSyncChannel,
    state: &RelaySyncStateRow,
    now: i64,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO relay_sync_state (
                channel, last_poll_at_ms, last_success_at_ms, consecutive_failures, backoff_until_ms,
                last_error, last_processed_count, total_processed_count, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(channel) DO UPDATE SET
                last_poll_at_ms = excluded.last_poll_at_ms,
                last_success_at_ms = excluded.last_success_at_ms,
                consecutive_failures = excluded.consecutive_failures,
                backoff_until_ms = excluded.backoff_until_ms,
                last_error = excluded.last_error,
                last_processed_count = excluded.last_processed_count,
                total_processed_count = excluded.total_processed_count,
                updated_at_ms = excluded.updated_at_ms",
            rusqlite::params![
                channel.as_row_key(),
                state.last_poll_at_ms,
                state.last_success_at_ms,
                state.consecutive_failures,
                state.backoff_until_ms,
                state.last_error,
                state.last_processed_count,
                state.total_processed_count,
                now
            ],
        )
        .map_err(|e| format!("Could not persist relay sync status: {e}"))?;
    Ok(())
}

#[tauri::command]
fn get_transport_status() -> Result<TransportStatusResponse, String> {
    let status: TransportStatus = ProviderRuntime::default().transport_status();
    Ok(TransportStatusResponse {
        mode: status.mode.as_str().to_string(),
        relay_configured: status.relay_configured,
        relay_url: status.relay_url,
    })
}

#[tauri::command]
fn set_subscriber_token(
    input: RelaySubscriberTokenInput,
) -> Result<TransportStatusResponse, String> {
    providers::keychain::set_relay_subscriber_token(input.token.trim())
        .map_err(|e| e.to_string())?;
    get_transport_status()
}

#[tauri::command]
fn remove_subscriber_token() -> Result<TransportStatusResponse, String> {
    providers::keychain::delete_relay_subscriber_token().map_err(|e| e.to_string())?;
    get_transport_status()
}

#[tauri::command]
fn set_api_key_ref(input: ApiKeyRefInput) -> Result<ApiKeyRefStatus, String> {
    let ref_name = sanitize_api_key_ref_name(&input.ref_name)?;
    providers::keychain::set_api_key_ref_secret(&ref_name, input.secret.trim())
        .map_err(|e| e.to_string())?;
    Ok(ApiKeyRefStatus {
        ref_name,
        configured: true,
    })
}

#[tauri::command]
fn remove_api_key_ref(input: ApiKeyRefDeleteInput) -> Result<ApiKeyRefStatus, String> {
    let ref_name = sanitize_api_key_ref_name(&input.ref_name)?;
    providers::keychain::delete_api_key_ref_secret(&ref_name).map_err(|e| e.to_string())?;
    Ok(ApiKeyRefStatus {
        ref_name,
        configured: false,
    })
}

#[tauri::command]
fn get_api_key_ref_status(ref_name: String) -> Result<ApiKeyRefStatus, String> {
    let ref_name = sanitize_api_key_ref_name(&ref_name)?;
    let configured = providers::keychain::get_api_key_ref_secret(&ref_name)
        .map_err(|e| e.to_string())?
        .is_some_and(|v| !v.trim().is_empty());
    Ok(ApiKeyRefStatus {
        ref_name,
        configured,
    })
}

#[tauri::command]
fn probe_vault_extraction(
    input: VaultExtractionProbeInput,
) -> Result<vault_spike::VaultExtractionProbe, String> {
    vault_spike::probe_extraction(&input.path, input.max_preview_chars).map_err(|e| e.to_string())
}

fn codex_oauth_status_response() -> Result<CodexOauthStatusResponse, String> {
    let local_path = providers::keychain::codex_cli_auth_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "~/.codex/auth.json".to_string());
    let local_snapshot =
        providers::keychain::read_codex_cli_auth_snapshot().map_err(|e| e.to_string())?;
    let imported = providers::keychain::get_codex_oauth_bundle().map_err(|e| e.to_string())?;
    Ok(CodexOauthStatusResponse {
        configured: imported.is_some(),
        local_auth_found: local_snapshot.is_some(),
        local_auth_path: local_path,
        local_auth_mode: local_snapshot.as_ref().map(|s| s.auth_mode.clone()),
        imported_auth_mode: imported.as_ref().map(|b| b.auth_mode.clone()),
        imported_at_ms: imported.as_ref().map(|b| b.imported_at_ms),
        last_refresh: imported
            .as_ref()
            .and_then(|b| b.last_refresh.clone())
            .or_else(|| local_snapshot.as_ref().and_then(|s| s.last_refresh.clone())),
        has_refresh_token: imported
            .as_ref()
            .and_then(|b| b.refresh_token.as_ref())
            .is_some_and(|v| !v.trim().is_empty()),
    })
}

#[tauri::command]
fn get_codex_oauth_status() -> Result<CodexOauthStatusResponse, String> {
    codex_oauth_status_response()
}

#[tauri::command]
fn import_codex_oauth_from_local_auth() -> Result<CodexOauthStatusResponse, String> {
    let _bundle = providers::keychain::import_codex_oauth_from_local_auth(now_ms())
        .map_err(|e| e.to_string())?;
    codex_oauth_status_response()
}

#[tauri::command]
fn remove_codex_oauth() -> Result<CodexOauthStatusResponse, String> {
    providers::keychain::delete_codex_oauth_bundle().map_err(|e| e.to_string())?;
    codex_oauth_status_response()
}

#[tauri::command]
fn list_webhook_triggers(
    state: tauri::State<AppState>,
    autopilot_id: Option<String>,
) -> Result<Vec<webhook_triggers::WebhookTriggerRecord>, String> {
    let connection = open_connection(&state)?;
    let relay_base = relay_webhook_base_url();
    webhook_triggers::list_webhook_triggers(
        &connection,
        autopilot_id
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty()),
        &relay_base,
        &|trigger_id| {
            providers::keychain::get_webhook_trigger_secret(trigger_id)
                .ok()
                .flatten()
                .is_some_and(|v| !v.trim().is_empty())
        },
    )
}

#[tauri::command]
fn create_webhook_trigger(
    state: tauri::State<AppState>,
    input: CreateWebhookTriggerInput,
) -> Result<WebhookTriggerCreateResponse, String> {
    let connection = open_connection(&state)?;
    let autopilot_id = input.autopilot_id.trim();
    if autopilot_id.is_empty() {
        return Err("Autopilot ID is required to create a webhook trigger.".to_string());
    }
    let (plan_json, provider_kind) = latest_run_plan_snapshot(&connection, autopilot_id)?;
    let trigger_id = make_main_id("whtrig");
    let endpoint_path = format!("hooks/{}", make_hashed_token("wh", &trigger_id));
    let now = now_ms();
    let max_payload_bytes = input
        .max_payload_bytes
        .unwrap_or(32_768)
        .clamp(1_024, 65_536);
    let description = input
        .description
        .unwrap_or_else(|| format!("Webhook trigger for {autopilot_id}"));
    let payload = webhook_triggers::WebhookTriggerCreateInternal {
        id: trigger_id.clone(),
        autopilot_id: autopilot_id.to_string(),
        status: "active".to_string(),
        endpoint_path,
        signature_mode: "terminus_hmac_sha256".to_string(),
        description: description.chars().take(120).collect(),
        max_payload_bytes,
        allowed_content_types_json: "[\"application/json\"]".to_string(),
        plan_json,
        provider_kind,
        created_at_ms: now,
        updated_at_ms: now,
    };
    let signing_secret = generate_secret_token("whsec");
    providers::keychain::set_webhook_trigger_secret(&trigger_id, &signing_secret)
        .map_err(|e| e.to_string())?;
    let relay_base = relay_webhook_base_url();
    let trigger =
        webhook_triggers::create_webhook_trigger(&connection, &payload, &relay_base, &|id| {
            providers::keychain::get_webhook_trigger_secret(id)
                .ok()
                .flatten()
                .is_some_and(|v| !v.trim().is_empty())
        })?;
    Ok(WebhookTriggerCreateResponse {
        trigger,
        signing_secret_preview: signing_secret,
    })
}

#[tauri::command]
fn rotate_webhook_trigger_secret(
    state: tauri::State<AppState>,
    trigger_id: String,
) -> Result<WebhookTriggerCreateResponse, String> {
    let connection = open_connection(&state)?;
    let trigger_id = trigger_id.trim();
    if trigger_id.is_empty() {
        return Err("Trigger ID is required.".to_string());
    }
    let new_secret = generate_secret_token("whsec");
    providers::keychain::set_webhook_trigger_secret(trigger_id, &new_secret)
        .map_err(|e| e.to_string())?;
    let relay_base = relay_webhook_base_url();
    let trigger =
        webhook_triggers::get_webhook_trigger(&connection, trigger_id, &relay_base, &|id| {
            providers::keychain::get_webhook_trigger_secret(id)
                .ok()
                .flatten()
                .is_some_and(|v| !v.trim().is_empty())
        })?
        .ok_or_else(|| "Webhook trigger not found.".to_string())?;
    Ok(WebhookTriggerCreateResponse {
        trigger,
        signing_secret_preview: new_secret,
    })
}

#[tauri::command]
fn disable_webhook_trigger(
    state: tauri::State<AppState>,
    trigger_id: String,
) -> Result<webhook_triggers::WebhookTriggerRecord, String> {
    update_webhook_trigger_enabled(state, trigger_id, false)
}

#[tauri::command]
fn enable_webhook_trigger(
    state: tauri::State<AppState>,
    trigger_id: String,
) -> Result<webhook_triggers::WebhookTriggerRecord, String> {
    update_webhook_trigger_enabled(state, trigger_id, true)
}

#[tauri::command]
fn get_webhook_trigger_events(
    state: tauri::State<AppState>,
    trigger_id: String,
    limit: Option<usize>,
) -> Result<Vec<webhook_triggers::WebhookTriggerEventRecord>, String> {
    let connection = open_connection(&state)?;
    let trigger_id = trigger_id.trim();
    if trigger_id.is_empty() {
        return Err("Trigger ID is required.".to_string());
    }
    webhook_triggers::list_webhook_trigger_events(&connection, trigger_id, limit.unwrap_or(20))
}

#[tauri::command]
fn ingest_webhook_event_local_debug(
    state: tauri::State<AppState>,
    input: WebhookEventLocalDebugInput,
) -> Result<WebhookIngestResult, String> {
    if !cfg!(debug_assertions) {
        return Err("Webhook debug ingestion is only available in development builds.".to_string());
    }
    let mut connection = open_connection(&state)?;
    ingest_webhook_event_internal(
        &mut connection,
        WebhookIngestInput {
            relay_request_id: Some(make_main_id("relay_wh_dbg")),
            relay_callback_secret: None,
            relay_issued_at_ms: Some(now_ms()),
            trigger_id: input.trigger_id,
            delivery_id: input.delivery_id,
            content_type: input
                .content_type
                .unwrap_or_else(|| "application/json".to_string()),
            body_json: input.body_json,
            signature: None,
            signature_ts_ms: None,
            headers_redacted_json: None,
            relay_channel: Some("local_debug".to_string()),
            require_relay_callback_auth: false,
            require_webhook_signature: false,
        },
    )
}

#[tauri::command]
fn resolve_relay_webhook_callback(
    state: tauri::State<AppState>,
    input: RelayWebhookCallbackInput,
) -> Result<WebhookIngestResult, String> {
    let mut connection = open_connection(&state)?;
    ingest_webhook_event_internal(
        &mut connection,
        WebhookIngestInput {
            relay_request_id: Some(input.request_id),
            relay_callback_secret: Some(input.callback_secret),
            relay_issued_at_ms: Some(input.issued_at_ms),
            trigger_id: input.trigger_id,
            delivery_id: input.delivery_id,
            content_type: input.content_type,
            body_json: input.body_json,
            signature: Some(input.signature),
            signature_ts_ms: Some(input.signature_ts_ms),
            headers_redacted_json: input.headers_redacted_json,
            relay_channel: input.channel.or(Some("relay_webhook_callback".to_string())),
            require_relay_callback_auth: true,
            require_webhook_signature: true,
        },
    )
}

#[tauri::command]
fn get_gmail_pubsub_status(
    state: tauri::State<AppState>,
) -> Result<gmail_pubsub::GmailPubSubStatus, String> {
    let connection = open_connection(&state)?;
    gmail_pubsub::maybe_mark_expired(&connection, now_ms())
}

#[tauri::command]
fn list_gmail_pubsub_events(
    state: tauri::State<AppState>,
    limit: Option<usize>,
) -> Result<Vec<gmail_pubsub::GmailPubSubEventRecord>, String> {
    let connection = open_connection(&state)?;
    gmail_pubsub::list_events(&connection, limit.unwrap_or(20))
}

#[tauri::command]
fn enable_gmail_pubsub(
    state: tauri::State<AppState>,
    input: GmailPubSubEnableInput,
) -> Result<gmail_pubsub::GmailPubSubStatus, String> {
    let connection = open_connection(&state)?;
    let topic_name = sanitize_gmail_pubsub_resource_name(&input.topic_name, "topic")?;
    let subscription_name =
        sanitize_gmail_pubsub_resource_name(&input.subscription_name, "subscription")?;
    let callback_mode = input
        .callback_mode
        .as_deref()
        .map(validate_gmail_pubsub_callback_mode)
        .transpose()?
        .unwrap_or_else(|| "relay".to_string());
    let trigger_mode = input
        .trigger_mode
        .as_deref()
        .map(validate_gmail_trigger_mode)
        .transpose()?
        .unwrap_or_else(|| "auto".to_string());
    gmail_pubsub::upsert_state(
        &connection,
        "pending_setup",
        &trigger_mode,
        Some(&topic_name),
        Some(&subscription_name),
        &callback_mode,
        None,
        None,
        None,
        0,
        now_ms(),
    )
}

#[tauri::command]
fn disable_gmail_pubsub(
    state: tauri::State<AppState>,
) -> Result<gmail_pubsub::GmailPubSubStatus, String> {
    let connection = open_connection(&state)?;
    let current = gmail_pubsub::get_status(&connection)?;
    gmail_pubsub::upsert_state(
        &connection,
        "disabled",
        "polling",
        current.topic_name.as_deref(),
        current.subscription_name.as_deref(),
        &current.callback_mode,
        current.watch_expiration_ms,
        current.history_id.as_deref(),
        None,
        0,
        now_ms(),
    )
}

#[tauri::command]
fn renew_gmail_pubsub_watch(
    state: tauri::State<AppState>,
) -> Result<gmail_pubsub::GmailPubSubStatus, String> {
    let connection = open_connection(&state)?;
    let status = gmail_pubsub::get_status(&connection)?;
    let topic = status
        .topic_name
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "Set a Gmail PubSub topic name before renewing the watch.".to_string())?;
    let token =
        email_connections::get_access_token(&connection, email_connections::EmailProvider::Gmail)?;
    let (expiration_ms, history_id) = gmail_watch_register(&token, topic)?;
    gmail_pubsub::update_watch_success(
        &connection,
        Some(expiration_ms),
        Some(&history_id),
        now_ms(),
    )
}

#[tauri::command]
fn ingest_gmail_pubsub_local_debug(
    state: tauri::State<AppState>,
    input: GmailPubSubLocalDebugInput,
) -> Result<GmailPubSubIngestResult, String> {
    if !cfg!(debug_assertions) {
        return Err(
            "Gmail PubSub debug ingestion is only available in development builds.".to_string(),
        );
    }
    let mut connection = open_connection(&state)?;
    ingest_gmail_pubsub_event_internal(
        &mut connection,
        Some(make_main_id("relay_gpub_dbg")),
        Some("local_debug"),
        &input.body_json,
        false,
        run_gmail_watcher_from_control,
    )
}

#[tauri::command]
fn resolve_relay_gmail_pubsub_callback(
    state: tauri::State<AppState>,
    input: RelayGmailPubSubCallbackInput,
) -> Result<GmailPubSubIngestResult, String> {
    let mut connection = open_connection(&state)?;
    validate_relay_callback_auth_fields(
        &input.request_id,
        &input.callback_secret,
        input.issued_at_ms,
        "Remote Gmail PubSub delivery is not ready yet. Generate a callback secret first.",
    )?;
    ingest_gmail_pubsub_event_internal(
        &mut connection,
        Some(input.request_id),
        input.channel.as_deref(),
        &input.body_json,
        true,
        run_gmail_watcher_from_control,
    )
}

#[tauri::command]
fn start_recipe_run(
    state: tauri::State<AppState>,
    autopilot_id: String,
    recipe: String,
    intent: String,
    pasted_text: Option<String>,
    daily_sources: Option<Vec<String>>,
    provider: String,
    idempotency_key: String,
    max_retries: Option<i64>,
    plan_json: Option<String>,
) -> Result<RunRecord, String> {
    let mut connection = open_connection(&state)?;
    let recipe_kind = parse_recipe(&recipe)?;
    let provider_id = parse_provider(&provider)?;
    let mut plan = match (recipe_kind, plan_json.as_deref()) {
        (RecipeKind::Custom, Some(json)) => {
            let parsed = serde_json::from_str::<AutopilotPlan>(json)
                .map_err(|e| format!("Custom plan is invalid JSON: {e}"))?;
            validate_custom_execution_plan(parsed, provider_id)?
        }
        (RecipeKind::Custom, None) => {
            return Err(
                "Custom runs require a generated plan. Draft the intent again and retry."
                    .to_string(),
            );
        }
        (_, _) => AutopilotPlan::from_intent(recipe_kind, intent, provider_id),
    };
    if let Some(text) = pasted_text {
        if !text.trim().is_empty() {
            plan.inbox_source_text = Some(text);
        }
    }
    if let Some(sources) = daily_sources {
        let cleaned = sources
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<String>>();
        if !cleaned.is_empty() {
            for source in &cleaned {
                if let Some((_, host)) = crate::web::parse_scheme_host(source) {
                    if !plan
                        .web_allowed_domains
                        .iter()
                        .any(|h| h.eq_ignore_ascii_case(&host))
                    {
                        plan.web_allowed_domains.push(host);
                    }
                }
            }
            plan.daily_sources = cleaned;
        }
    }

    RunnerEngine::start_run(
        &mut connection,
        &autopilot_id,
        plan,
        &idempotency_key,
        max_retries.unwrap_or(2),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn run_tick(state: tauri::State<AppState>, run_id: String) -> Result<RunRecord, String> {
    let mut connection = open_connection(&state)?;
    RunnerEngine::run_tick(&mut connection, &run_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn resume_due_runs(
    state: tauri::State<AppState>,
    limit: Option<usize>,
) -> Result<Vec<RunRecord>, String> {
    let mut connection = open_connection(&state)?;
    RunnerEngine::resume_due_runs(&mut connection, limit.unwrap_or(20)).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_mission_draft(
    input: missions::CreateMissionDraftInput,
) -> Result<missions::MissionDraft, String> {
    missions::create_mission_draft(input)
}

#[tauri::command]
fn start_mission(
    state: tauri::State<AppState>,
    input: missions::StartMissionInput,
) -> Result<missions::MissionDetail, String> {
    let mut connection = open_connection(&state)?;
    missions::start_mission(&mut connection, input)
}

#[tauri::command]
fn get_mission(
    state: tauri::State<AppState>,
    mission_id: String,
) -> Result<missions::MissionDetail, String> {
    let connection = open_connection(&state)?;
    missions::get_mission(&connection, &mission_id)
}

#[tauri::command]
fn list_missions(
    state: tauri::State<AppState>,
    limit: Option<usize>,
) -> Result<Vec<missions::MissionRecord>, String> {
    let connection = open_connection(&state)?;
    missions::list_missions(&connection, limit.unwrap_or(20))
}

#[tauri::command]
fn run_mission_tick(
    state: tauri::State<AppState>,
    mission_id: String,
) -> Result<missions::MissionTickResult, String> {
    let mut connection = open_connection(&state)?;
    missions::run_mission_tick(&mut connection, &mission_id)
}

#[tauri::command]
fn approve_run_approval(
    state: tauri::State<AppState>,
    approval_id: String,
) -> Result<RunRecord, String> {
    let mut connection = open_connection(&state)?;
    approve_run_approval_with_context(
        &mut connection,
        &approval_id,
        Some("local_ui"),
        Some("User"),
    )
}

#[tauri::command]
fn reject_run_approval(
    state: tauri::State<AppState>,
    approval_id: String,
    reason: Option<String>,
) -> Result<RunRecord, String> {
    let mut connection = open_connection(&state)?;
    reject_run_approval_with_context(
        &mut connection,
        &approval_id,
        reason,
        Some("local_ui"),
        Some("User"),
    )
}

#[tauri::command]
fn approve_run_approval_remote(
    state: tauri::State<AppState>,
    input: ApprovalResolutionContextInput,
) -> Result<RunRecord, String> {
    let mut connection = open_connection(&state)?;
    approve_run_approval_with_context(
        &mut connection,
        &input.approval_id,
        input.channel.as_deref().or(Some("relay")),
        input.actor_label.as_deref(),
    )
}

#[tauri::command]
fn reject_run_approval_remote(
    state: tauri::State<AppState>,
    input: ApprovalResolutionContextInput,
) -> Result<RunRecord, String> {
    let mut connection = open_connection(&state)?;
    reject_run_approval_with_context(
        &mut connection,
        &input.approval_id,
        input.reason,
        input.channel.as_deref().or(Some("relay")),
        input.actor_label.as_deref(),
    )
}

#[tauri::command]
fn resolve_relay_approval_callback(
    state: tauri::State<AppState>,
    input: RelayApprovalCallbackInput,
) -> Result<RunRecord, String> {
    let mut connection = open_connection(&state)?;
    resolve_relay_approval_callback_with_connection(&mut connection, &input)
}

fn resolve_relay_approval_callback_with_connection(
    connection: &mut rusqlite::Connection,
    input: &RelayApprovalCallbackInput,
) -> Result<RunRecord, String> {
    validate_relay_callback_auth(&input)?;
    if let Some(existing) = get_relay_callback_existing_run(connection, &input.request_id)? {
        return Ok(existing);
    }
    let channel = input.channel.as_deref().or(Some("relay_callback"));
    let actor = input.actor_label.as_deref();
    if let Err(err) = reserve_relay_callback_event(
        connection,
        &input.request_id,
        &input.approval_id,
        &input.decision,
        channel,
        actor,
    ) {
        if err.contains("already processed") {
            if let Some(existing) = get_relay_callback_existing_run(connection, &input.request_id)?
            {
                return Ok(existing);
            }
        }
        return Err(err);
    }
    let run = match input.decision.trim().to_ascii_lowercase().as_str() {
        "approve" | "approved" => {
            approve_run_approval_with_context(connection, &input.approval_id, channel, actor)?
        }
        "reject" | "rejected" => reject_run_approval_with_context(
            connection,
            &input.approval_id,
            input.reason.clone(),
            channel,
            actor,
        )?,
        _ => return Err("Unknown approval decision. Use approve or reject.".to_string()),
    };
    update_relay_callback_event_status(connection, &input.request_id, "applied")?;
    Ok(run)
}

#[tauri::command]
fn list_pending_approvals(state: tauri::State<AppState>) -> Result<Vec<ApprovalRecord>, String> {
    let connection = open_connection(&state)?;
    RunnerEngine::list_pending_approvals(&connection).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_pending_clarifications(
    state: tauri::State<AppState>,
) -> Result<Vec<ClarificationRecord>, String> {
    let connection = open_connection(&state)?;
    RunnerEngine::list_pending_clarifications(&connection).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_run_diagnostics(
    state: tauri::State<AppState>,
    limit: Option<usize>,
) -> Result<Vec<diagnostics::RunDiagnosticRecord>, String> {
    let connection = open_connection(&state)?;
    diagnostics::list_run_diagnostics(&connection, limit.unwrap_or(20))
}

#[tauri::command]
fn apply_intervention(
    state: tauri::State<AppState>,
    input: diagnostics::ApplyInterventionInput,
) -> Result<diagnostics::ApplyInterventionResult, String> {
    let mut connection = open_connection(&state)?;
    diagnostics::apply_intervention(&mut connection, input)
}

#[tauri::command]
fn submit_clarification_answer(
    state: tauri::State<AppState>,
    clarification_id: String,
    answer_json: String,
) -> Result<RunRecord, String> {
    let mut connection = open_connection(&state)?;
    RunnerEngine::submit_clarification_answer(&mut connection, &clarification_id, &answer_json)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_run(state: tauri::State<AppState>, run_id: String) -> Result<RunRecord, String> {
    let connection = open_connection(&state)?;
    RunnerEngine::get_run(&connection, &run_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_terminal_receipt(
    state: tauri::State<AppState>,
    run_id: String,
) -> Result<Option<RunReceipt>, String> {
    let connection = open_connection(&state)?;
    RunnerEngine::get_terminal_receipt(&connection, &run_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_email_connections(
    state: tauri::State<AppState>,
) -> Result<Vec<email_connections::EmailConnectionRecord>, String> {
    let connection = open_connection(&state)?;
    email_connections::list_connections(&connection)
}

#[tauri::command]
fn save_email_oauth_config(
    state: tauri::State<AppState>,
    input: email_connections::OAuthConfigInput,
) -> Result<(), String> {
    let connection = open_connection(&state)?;
    email_connections::upsert_oauth_config(&connection, input)
}

#[tauri::command]
fn start_email_oauth(
    state: tauri::State<AppState>,
    provider: String,
) -> Result<email_connections::OAuthStartResponse, String> {
    let connection = open_connection(&state)?;
    email_connections::start_oauth(&connection, &provider)
}

#[tauri::command]
fn complete_email_oauth(
    state: tauri::State<AppState>,
    input: email_connections::OAuthCompleteInput,
) -> Result<email_connections::EmailConnectionRecord, String> {
    let connection = open_connection(&state)?;
    email_connections::complete_oauth(&connection, input)
}

#[tauri::command]
fn disconnect_email_provider(
    state: tauri::State<AppState>,
    provider: String,
) -> Result<(), String> {
    let connection = open_connection(&state)?;
    email_connections::disconnect(&connection, &provider)
}

#[tauri::command]
fn run_inbox_watcher_tick(
    state: tauri::State<AppState>,
    provider: String,
    autopilot_id: String,
    max_items: Option<usize>,
) -> Result<inbox_watcher::InboxWatcherTickSummary, String> {
    let mut connection = open_connection(&state)?;
    inbox_watcher::run_watcher_tick(
        &mut connection,
        &provider,
        &autopilot_id,
        max_items.unwrap_or(10),
    )
}

#[tauri::command]
fn get_runner_control(state: tauri::State<AppState>) -> Result<db::RunnerControlRecord, String> {
    let connection = open_connection(&state)?;
    db::get_runner_control(&connection)
}

#[tauri::command]
fn update_runner_control(
    state: tauri::State<AppState>,
    input: RunnerControlInput,
) -> Result<db::RunnerControlRecord, String> {
    if !(15..=900).contains(&input.watcher_poll_seconds) {
        return Err("Watcher poll interval must be between 15 and 900 seconds.".to_string());
    }
    if !(1..=25).contains(&input.watcher_max_items) {
        return Err("Watcher max emails must be between 1 and 25.".to_string());
    }
    if input.gmail_autopilot_id.trim().is_empty() || input.microsoft_autopilot_id.trim().is_empty()
    {
        return Err("Autopilot IDs cannot be empty.".to_string());
    }

    let connection = open_connection(&state)?;
    let mut current = db::get_runner_control(&connection)?;
    current.gmail_trigger_mode = validate_gmail_trigger_mode(&input.gmail_trigger_mode)?;
    current.background_enabled = input.background_enabled;
    current.watcher_enabled = input.watcher_enabled;
    current.watcher_poll_seconds = input.watcher_poll_seconds;
    current.watcher_max_items = input.watcher_max_items;
    current.gmail_autopilot_id = input.gmail_autopilot_id.trim().to_string();
    current.microsoft_autopilot_id = input.microsoft_autopilot_id.trim().to_string();
    db::upsert_runner_control(&connection, &current)?;
    db::get_runner_control(&connection)
}

#[tauri::command]
fn get_onboarding_state(
    state: tauri::State<AppState>,
) -> Result<db::OnboardingStateRecord, String> {
    let connection = open_connection(&state)?;
    db::get_onboarding_state(&connection)
}

#[tauri::command]
fn save_onboarding_state(
    state: tauri::State<AppState>,
    input: OnboardingStateInput,
) -> Result<db::OnboardingStateRecord, String> {
    let connection = open_connection(&state)?;
    let current = db::get_onboarding_state(&connection)?;
    let now = now_ms();
    let mark_complete = input
        .onboarding_complete
        .unwrap_or(current.onboarding_complete);
    let dismissed = input.dismissed.unwrap_or(current.dismissed);
    let payload = db::OnboardingStateRecord {
        onboarding_complete: mark_complete,
        dismissed,
        role_text: input.role_text,
        work_focus_text: input.work_focus_text,
        biggest_pain_text: input.biggest_pain_text,
        recommended_intent: input.recommended_intent,
        started_at_ms: current.started_at_ms,
        updated_at_ms: current.updated_at_ms,
        completed_at_ms: if mark_complete {
            current.completed_at_ms.or(Some(now))
        } else {
            None
        },
        dismissed_at_ms: if dismissed { Some(now) } else { None },
        first_successful_run_at_ms: current.first_successful_run_at_ms,
    };
    db::upsert_onboarding_state(&connection, &payload)
}

#[tauri::command]
fn dismiss_onboarding(state: tauri::State<AppState>) -> Result<db::OnboardingStateRecord, String> {
    let connection = open_connection(&state)?;
    let current = db::get_onboarding_state(&connection)?;
    let now = now_ms();
    let payload = db::OnboardingStateRecord {
        onboarding_complete: current.onboarding_complete,
        dismissed: true,
        role_text: current.role_text,
        work_focus_text: current.work_focus_text,
        biggest_pain_text: current.biggest_pain_text,
        recommended_intent: current.recommended_intent,
        started_at_ms: current.started_at_ms,
        updated_at_ms: current.updated_at_ms,
        completed_at_ms: current.completed_at_ms,
        dismissed_at_ms: Some(now),
        first_successful_run_at_ms: current.first_successful_run_at_ms,
    };
    db::upsert_onboarding_state(&connection, &payload)
}

#[tauri::command]
fn get_global_voice_config(state: tauri::State<AppState>) -> Result<db::VoiceConfigRecord, String> {
    let connection = open_connection(&state)?;
    db::get_global_voice_config(&connection)
}

#[tauri::command]
fn update_global_voice_config(
    state: tauri::State<AppState>,
    input: VoiceConfigInput,
) -> Result<db::VoiceConfigRecord, String> {
    let connection = open_connection(&state)?;
    let payload = db::VoiceConfigRecord {
        tone: validate_voice_tone(&input.tone)?,
        length: validate_voice_length(&input.length)?,
        humor: validate_voice_humor(&input.humor)?,
        notes: sanitize_voice_notes(&input.notes),
        updated_at_ms: now_ms(),
    };
    db::upsert_global_voice_config(&connection, &payload)
}

#[tauri::command]
fn get_autopilot_voice_config(
    state: tauri::State<AppState>,
    autopilot_id: String,
) -> Result<db::AutopilotVoiceConfigRecord, String> {
    let connection = open_connection(&state)?;
    if autopilot_id.trim().is_empty() {
        return Err("Autopilot ID is required.".to_string());
    }
    db::get_autopilot_voice_config(&connection, autopilot_id.trim())
}

#[tauri::command]
fn update_autopilot_voice_config(
    state: tauri::State<AppState>,
    input: AutopilotVoiceConfigInput,
) -> Result<db::AutopilotVoiceConfigRecord, String> {
    let connection = open_connection(&state)?;
    let autopilot_id = input.autopilot_id.trim();
    if autopilot_id.is_empty() {
        return Err("Autopilot ID is required.".to_string());
    }
    let payload = db::AutopilotVoiceConfigRecord {
        autopilot_id: autopilot_id.to_string(),
        enabled: input.enabled,
        tone: validate_voice_tone(&input.tone)?,
        length: validate_voice_length(&input.length)?,
        humor: validate_voice_humor(&input.humor)?,
        notes: sanitize_voice_notes(&input.notes),
        updated_at_ms: now_ms(),
    };
    db::upsert_autopilot_voice_config(&connection, &payload)
}

#[tauri::command]
fn clear_autopilot_voice_config(
    state: tauri::State<AppState>,
    autopilot_id: String,
) -> Result<db::AutopilotVoiceConfigRecord, String> {
    let connection = open_connection(&state)?;
    let trimmed = autopilot_id.trim();
    if trimmed.is_empty() {
        return Err("Autopilot ID is required.".to_string());
    }
    db::clear_autopilot_voice_config(&connection, trimmed)?;
    db::get_autopilot_voice_config(&connection, trimmed)
}

#[tauri::command]
fn tick_runner_cycle(state: tauri::State<AppState>) -> Result<RunnerCycleSummary, String> {
    let mut connection = open_connection(&state)?;
    tick_runner_cycle_internal(&mut connection, false)
}

fn tick_runner_cycle_internal(
    connection: &mut rusqlite::Connection,
    require_background_enabled: bool,
) -> Result<RunnerCycleSummary, String> {
    let mut control = db::get_runner_control(&connection)?;
    if require_background_enabled && !control.background_enabled {
        return Ok(RunnerCycleSummary {
            watcher_status: "background_off".to_string(),
            relay_sync_status: "background_off".to_string(),
            providers_polled: 0,
            fetched: 0,
            deduped: 0,
            started_runs: 0,
            failed: 0,
            resumed_due_runs: 0,
            relay_decisions_applied: 0,
            missed_runs_detected: 0,
            catch_up_cycles_run: 0,
        });
    }
    let now = now_ms();
    let poll_ms = control.watcher_poll_seconds.saturating_mul(1000);

    let mut summary = RunnerCycleSummary {
        watcher_status: "idle".to_string(),
        relay_sync_status: "idle".to_string(),
        providers_polled: 0,
        fetched: 0,
        deduped: 0,
        started_runs: 0,
        failed: 0,
        resumed_due_runs: 0,
        relay_decisions_applied: 0,
        missed_runs_detected: 0,
        catch_up_cycles_run: 0,
    };

    let missed_cycles = compute_missed_cycles(control.watcher_last_tick_ms, now, poll_ms);
    if missed_cycles > 0 {
        summary.missed_runs_detected = missed_cycles;
        control.missed_runs_count = missed_cycles;
    }

    if !control.watcher_enabled {
        summary.watcher_status = "paused".to_string();
    } else if let Some(last_tick) = control.watcher_last_tick_ms {
        if now - last_tick < poll_ms {
            summary.watcher_status = "throttled".to_string();
        } else {
            let catch_up_cycles = missed_cycles.min(3);
            for _ in 0..catch_up_cycles {
                run_watchers(connection, &control, &mut summary)?;
                summary.catch_up_cycles_run += 1;
            }
            run_watchers(connection, &control, &mut summary)?;
            control.watcher_last_tick_ms = Some(now);
            control.missed_runs_count = 0;
            db::upsert_runner_control(&connection, &control)?;
            summary.watcher_status = "ran".to_string();
        }
    } else {
        run_watchers(connection, &control, &mut summary)?;
        control.watcher_last_tick_ms = Some(now);
        control.missed_runs_count = 0;
        db::upsert_runner_control(&connection, &control)?;
        summary.watcher_status = "ran".to_string();
    }

    let resumed = RunnerEngine::resume_due_runs(connection, 20).map_err(|e| e.to_string())?;
    summary.resumed_due_runs = resumed.len();
    match tick_relay_approval_sync_internal(connection, false, RelayDecisionSyncChannel::Poll) {
        Ok(sync) => {
            summary.relay_sync_status = sync.status.status;
            summary.relay_decisions_applied = sync.applied_count;
        }
        Err(err) => {
            summary.relay_sync_status = "error".to_string();
            eprintln!("relay approval sync failed: {}", sanitize_log_message(&err));
        }
    }
    if summary.watcher_status == "throttled" && control.missed_runs_count > 0 {
        db::upsert_runner_control(&connection, &control)?;
    }
    Ok(summary)
}

fn spawn_background_cycle_thread(app: &tauri::AppHandle, db_path: PathBuf) {
    let app_handle = app.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(10));
        let app_state = app_handle.state::<AppState>();
        if app_state
            .db_path
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .is_none()
        {
            continue;
        }
        let mut connection = match open_connection_from_path(&db_path) {
            Ok(conn) => conn,
            Err(_) => continue,
        };
        if let Err(err) = tick_runner_cycle_internal(&mut connection, true) {
            eprintln!(
                "background runner cycle failed: {}",
                sanitize_log_message(&err)
            );
        }
    });
}

fn spawn_background_relay_push_thread(app: &tauri::AppHandle, db_path: PathBuf) {
    let app_handle = app.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(5));
        let app_state = app_handle.state::<AppState>();
        if app_state
            .db_path
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .is_none()
        {
            continue;
        }
        let mut connection = match open_connection_from_path(&db_path) {
            Ok(conn) => conn,
            Err(_) => continue,
        };
        let control = match db::get_runner_control(&connection) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if !control.background_enabled {
            continue;
        }
        if let Err(err) = tick_relay_approval_sync_internal(
            &mut connection,
            false,
            RelayDecisionSyncChannel::Push,
        ) {
            eprintln!("relay push sync failed: {}", sanitize_log_message(&err));
        }
    });
}

fn install_tray(app: &tauri::AppHandle) -> Result<(), String> {
    let open_item = MenuItemBuilder::with_id("tray_open", "Open Terminus")
        .build(app)
        .map_err(|e| e.to_string())?;
    let run_item = MenuItemBuilder::with_id("tray_run_now", "Run Cycle Now")
        .build(app)
        .map_err(|e| e.to_string())?;
    let quit_item = MenuItemBuilder::with_id("tray_quit", "Quit")
        .build(app)
        .map_err(|e| e.to_string())?;
    let menu = MenuBuilder::new(app)
        .items(&[&open_item, &run_item, &quit_item])
        .build()
        .map_err(|e| e.to_string())?;

    let app_handle = app.clone();
    TrayIconBuilder::new()
        .menu(&menu)
        .on_menu_event(move |_, event: MenuEvent| match event.id().as_ref() {
            "tray_open" => {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "tray_run_now" => {
                let app_state = app_handle.state::<AppState>();
                let db_path = app_state.db_path.lock().ok().and_then(|g| g.clone());
                if let Some(path) = db_path {
                    if let Ok(mut connection) = open_connection_from_path(&path) {
                        if let Err(err) = tick_runner_cycle_internal(&mut connection, false) {
                            eprintln!("tray run cycle failed: {}", sanitize_log_message(&err));
                        }
                    }
                }
            }
            "tray_quit" => {
                app_handle.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(move |tray: &TrayIcon, event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(window) = tray.app_handle().get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
fn get_autopilot_send_policy(
    state: tauri::State<AppState>,
    autopilot_id: String,
) -> Result<db::AutopilotSendPolicyRecord, String> {
    let connection = open_connection(&state)?;
    db::get_autopilot_send_policy(&connection, autopilot_id.trim())
}

#[tauri::command]
fn update_autopilot_send_policy(
    state: tauri::State<AppState>,
    input: AutopilotSendPolicyInput,
) -> Result<db::AutopilotSendPolicyRecord, String> {
    let autopilot_id = input.autopilot_id.trim();
    if autopilot_id.is_empty() {
        return Err("Autopilot ID is required.".to_string());
    }
    if !(1..=200).contains(&input.max_sends_per_day) {
        return Err("Max sends per day must be between 1 and 200.".to_string());
    }
    if !(0..=23).contains(&input.quiet_hours_start_local)
        || !(0..=23).contains(&input.quiet_hours_end_local)
    {
        return Err("Quiet hours must use 0-23 clock values.".to_string());
    }
    if input.allow_sending && input.recipient_allowlist.is_empty() {
        return Err("Add at least one allowed recipient before enabling sending.".to_string());
    }

    let connection = open_connection(&state)?;
    let cleaned_allowlist = input
        .recipient_allowlist
        .into_iter()
        .map(|r| r.trim().to_ascii_lowercase())
        .filter(|r| !r.is_empty())
        .collect::<Vec<String>>();
    let updated = db::AutopilotSendPolicyRecord {
        autopilot_id: autopilot_id.to_string(),
        allow_sending: input.allow_sending,
        recipient_allowlist: cleaned_allowlist,
        max_sends_per_day: input.max_sends_per_day,
        quiet_hours_start_local: input.quiet_hours_start_local,
        quiet_hours_end_local: input.quiet_hours_end_local,
        allow_outside_quiet_hours: input.allow_outside_quiet_hours,
        updated_at_ms: now_ms(),
    };
    db::upsert_autopilot_send_policy(&connection, &updated)?;
    db::get_autopilot_send_policy(&connection, autopilot_id)
}

#[tauri::command]
fn submit_guidance(
    state: tauri::State<AppState>,
    input: GuidanceInput,
) -> Result<GuidanceResponse, String> {
    let scope_type = input.scope_type.trim().to_ascii_lowercase();
    if !matches!(
        scope_type.as_str(),
        "autopilot" | "run" | "approval" | "outcome"
    ) {
        return Err("Choose a valid guidance scope.".to_string());
    }
    let scope_id = input.scope_id.trim();
    if scope_id.is_empty() {
        return Err("Scope ID is required.".to_string());
    }
    let cleaned_instruction = normalize_guidance_instruction(&input.instruction)?;
    let (mode, message, proposed_rule) = classify_guidance(&cleaned_instruction);

    let connection = open_connection(&state)?;
    let (autopilot_id, run_id, approval_id, outcome_id) = match scope_type.as_str() {
        "autopilot" => (Some(scope_id.to_string()), None, None, None),
        "run" => {
            let autopilot: Option<String> = connection
                .query_row(
                    "SELECT autopilot_id FROM runs WHERE id = ?1 LIMIT 1",
                    rusqlite::params![scope_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| format!("Failed to resolve run scope: {e}"))?;
            (autopilot, Some(scope_id.to_string()), None, None)
        }
        "approval" => {
            let run_ref: Option<(String, String)> = connection
                .query_row(
                    "SELECT a.run_id, r.autopilot_id
                     FROM approvals a
                     JOIN runs r ON r.id = a.run_id
                     WHERE a.id = ?1
                     LIMIT 1",
                    rusqlite::params![scope_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|e| format!("Failed to resolve approval scope: {e}"))?;
            match run_ref {
                Some((run, auto)) => (Some(auto), Some(run), Some(scope_id.to_string()), None),
                None => (None, None, Some(scope_id.to_string()), None),
            }
        }
        _ => (None, None, None, Some(scope_id.to_string())),
    };

    let response = GuidanceResponse {
        mode,
        message,
        proposed_rule: proposed_rule.clone(),
    };
    let result_json =
        serde_json::to_string(&response).map_err(|e| format!("Failed to store guidance: {e}"))?;

    db::insert_guidance_event(
        &connection,
        &db::GuidanceEventInsert {
            id: make_main_id("guide"),
            scope_type: scope_type.clone(),
            scope_id: scope_id.to_string(),
            autopilot_id,
            run_id: run_id.clone(),
            approval_id,
            outcome_id,
            mode: match mode {
                GuidanceMode::Applied => "applied".to_string(),
                GuidanceMode::ProposedRule => "proposed_rule".to_string(),
                GuidanceMode::NeedsApproval => "needs_approval".to_string(),
            },
            instruction: cleaned_instruction.clone(),
            result_json,
            created_at_ms: now_ms(),
        },
    )?;

    if let Some(run_id) = run_id {
        let _ = connection.execute(
            "
            INSERT INTO activities (id, run_id, activity_type, from_state, to_state, user_message, created_at)
            VALUES (?1, ?2, 'guidance_received', NULL, NULL, ?3, ?4)
            ",
            rusqlite::params![
                make_main_id("activity"),
                run_id,
                truncate_for_activity(&cleaned_instruction),
                now_ms()
            ],
        );
    }

    Ok(response)
}

fn run_watchers(
    connection: &mut rusqlite::Connection,
    control: &db::RunnerControlRecord,
    summary: &mut RunnerCycleSummary,
) -> Result<(), String> {
    let connections = email_connections::list_connections(connection)?;
    for provider in connections
        .into_iter()
        .filter(|record| record.status == "connected")
    {
        if provider.provider == "gmail" {
            let mut pubsub = gmail_pubsub::maybe_mark_expired(connection, now_ms())?;
            pubsub.trigger_mode = control.gmail_trigger_mode.clone();
            if !gmail_pubsub::should_poll_gmail(&pubsub, now_ms()) {
                summary.providers_polled += 1;
                continue;
            }
        }
        let autopilot_id = if provider.provider == "gmail" {
            control.gmail_autopilot_id.as_str()
        } else {
            control.microsoft_autopilot_id.as_str()
        };
        match inbox_watcher::run_watcher_tick(
            connection,
            &provider.provider,
            autopilot_id,
            control.watcher_max_items as usize,
        ) {
            Ok(result) => {
                summary.providers_polled += 1;
                summary.fetched += result.fetched;
                summary.deduped += result.deduped;
                summary.started_runs += result.started_runs;
                summary.failed += result.failed;
            }
            Err(err) => {
                summary.providers_polled += 1;
                summary.failed += 1;
                eprintln!(
                    "inbox watcher tick failed for {}: {}",
                    provider.provider,
                    sanitize_log_message(&err)
                );
            }
        }
    }
    Ok(())
}

fn truncate_for_activity(input: &str) -> String {
    let max = 180;
    if input.chars().count() <= max {
        return input.to_string();
    }
    input.chars().take(max).collect::<String>()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn make_main_id(prefix: &str) -> String {
    let seq = MAIN_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}_{}_{}", prefix, now_ms(), seq)
}

#[tauri::command]
fn record_decision_event(
    state: tauri::State<AppState>,
    autopilot_id: String,
    run_id: String,
    event_type: String,
    step_id: Option<String>,
    metadata_json: Option<String>,
    client_event_id: Option<String>,
) -> Result<(), String> {
    let connection = open_connection(&state)?;
    learning::record_decision_event_from_json(
        &connection,
        &autopilot_id,
        &run_id,
        step_id.as_deref(),
        &event_type,
        metadata_json.as_deref(),
        client_event_id.as_deref(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn compact_learning_data(
    state: tauri::State<AppState>,
    autopilot_id: Option<String>,
    dry_run: Option<bool>,
) -> Result<learning::LearningCompactionSummary, String> {
    let connection = open_connection(&state)?;
    learning::compact_learning_data(
        &connection,
        autopilot_id.as_deref(),
        dry_run.unwrap_or(false),
    )
    .map_err(|e| e.to_string())
}

fn generate_secret_token(prefix: &str) -> String {
    let raw = format!(
        "{}:{}:{}:{}",
        prefix,
        now_ms(),
        make_main_id("tok"),
        std::process::id()
    );
    let digest = Sha256::digest(raw.as_bytes());
    format!(
        "{}_{}",
        prefix,
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
    )
}

fn ensure_relay_device_id() -> Result<String, providers::types::ProviderError> {
    if let Some(existing) =
        providers::keychain::get_relay_device_id()?.filter(|v| !v.trim().is_empty())
    {
        return Ok(existing);
    }
    let device_id = generate_secret_token("device");
    providers::keychain::set_relay_device_id(&device_id)?;
    Ok(device_id)
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    if a_bytes.len() != b_bytes.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a_bytes.iter().zip(b_bytes.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn validate_relay_callback_auth(input: &RelayApprovalCallbackInput) -> Result<(), String> {
    validate_relay_callback_auth_fields(
        &input.request_id,
        &input.callback_secret,
        input.issued_at_ms,
        "Remote approvals are not ready yet. Generate a callback secret first.",
    )
}

fn validate_relay_callback_auth_fields(
    request_id: &str,
    callback_secret: &str,
    issued_at_ms: i64,
    missing_secret_message: &str,
) -> Result<(), String> {
    let request_id = request_id.trim();
    if request_id.is_empty() || request_id.len() > 120 {
        return Err("Relay callback request id is invalid.".to_string());
    }
    let expected = providers::keychain::get_relay_callback_secret()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| missing_secret_message.to_string())?;
    if !constant_time_eq(expected.trim(), callback_secret.trim()) {
        return Err("Relay callback authentication failed.".to_string());
    }
    let now = now_ms();
    if issued_at_ms <= 0 || (now - issued_at_ms).abs() > 15 * 60 * 1000 {
        return Err("Relay callback request expired. Retry from Terminus relay.".to_string());
    }
    Ok(())
}

fn get_relay_callback_existing_run(
    connection: &rusqlite::Connection,
    request_id: &str,
) -> Result<Option<RunRecord>, String> {
    let approval_id: Option<String> = connection
        .query_row(
            "SELECT approval_id FROM relay_callback_events WHERE request_id = ?1 LIMIT 1",
            rusqlite::params![request_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("Could not read relay callback history: {e}"))?;
    let Some(approval_id) = approval_id else {
        return Ok(None);
    };
    let approval = RunnerEngine::get_approval_for_external(connection, &approval_id)
        .map_err(|e| e.to_string())?;
    let run = RunnerEngine::get_run_for_external(connection, &approval.run_id)
        .map_err(|e| e.to_string())?;
    Ok(Some(run))
}

fn reserve_relay_callback_event(
    connection: &rusqlite::Connection,
    request_id: &str,
    approval_id: &str,
    decision: &str,
    channel: Option<&str>,
    actor_label: Option<&str>,
) -> Result<(), String> {
    let id = make_main_id("relay_cb");
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO relay_callback_events
             (id, request_id, approval_id, decision, status, channel, actor_label, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                id,
                request_id.trim(),
                approval_id,
                decision.trim().to_ascii_lowercase(),
                "received",
                sanitize_approval_resolution_field(channel, 32),
                sanitize_approval_resolution_field(actor_label, 80),
                now_ms()
            ],
        )
        .map_err(|e| format!("Could not record relay callback event: {e}"))?;
    if inserted == 0 {
        return Err("Relay callback request was already processed.".to_string());
    }
    Ok(())
}

fn update_relay_callback_event_status(
    connection: &rusqlite::Connection,
    request_id: &str,
    status: &str,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE relay_callback_events SET status = ?1 WHERE request_id = ?2",
            rusqlite::params![status, request_id.trim()],
        )
        .map_err(|e| format!("Could not update relay callback status: {e}"))?;
    Ok(())
}

fn sanitize_approval_resolution_field(value: Option<&str>, max_len: usize) -> Option<String> {
    let raw = value?.trim();
    if raw.is_empty() {
        return None;
    }
    let bounded = raw
        .chars()
        .take(max_len)
        .filter(|c| {
            c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.' | ':' | '@' | '/')
        })
        .collect::<String>()
        .trim()
        .to_string();
    if bounded.is_empty() {
        None
    } else {
        Some(bounded)
    }
}

fn relay_webhook_base_url() -> String {
    if let Ok(url) = std::env::var("TERMINUS_RELAY_WEBHOOK_URL") {
        if !url.trim().is_empty() {
            return url;
        }
    }
    let dispatch = transport::RelayTransport::default_url();
    if let Some((prefix, _)) = dispatch.rsplit_once('/') {
        format!("{prefix}/webhooks")
    } else {
        format!("{dispatch}/webhooks")
    }
}

fn make_hashed_token(prefix: &str, seed: &str) -> String {
    let digest = Sha256::digest(format!("{prefix}:{seed}:{}", now_ms()).as_bytes());
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    format!(
        "{}_{}",
        prefix,
        encoded.chars().take(24).collect::<String>()
    )
}

fn latest_run_plan_snapshot(
    connection: &rusqlite::Connection,
    autopilot_id: &str,
) -> Result<(String, String), String> {
    connection
        .query_row(
            "SELECT plan_json, provider_kind
             FROM runs
             WHERE autopilot_id = ?1
             ORDER BY updated_at DESC, created_at DESC
             LIMIT 1",
            rusqlite::params![autopilot_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| format!("Could not load latest run plan for this Autopilot: {e}"))?
        .ok_or_else(|| {
            "Run a test once before adding a webhook trigger so Terminus can snapshot the plan."
                .to_string()
        })
}

fn update_webhook_trigger_enabled(
    state: tauri::State<AppState>,
    trigger_id: String,
    enabled: bool,
) -> Result<webhook_triggers::WebhookTriggerRecord, String> {
    let connection = open_connection(&state)?;
    let trigger_id = trigger_id.trim();
    if trigger_id.is_empty() {
        return Err("Trigger ID is required.".to_string());
    }
    let status = if enabled { "active" } else { "paused" };
    webhook_triggers::update_webhook_trigger_status(&connection, trigger_id, status, None)?;
    let relay_base = relay_webhook_base_url();
    webhook_triggers::get_webhook_trigger(&connection, trigger_id, &relay_base, &|id| {
        providers::keychain::get_webhook_trigger_secret(id)
            .ok()
            .flatten()
            .is_some_and(|v| !v.trim().is_empty())
    })?
    .ok_or_else(|| "Webhook trigger not found.".to_string())
}

fn reserve_relay_webhook_callback_event(
    connection: &rusqlite::Connection,
    request_id: &str,
    trigger_id: &str,
    delivery_id: &str,
    channel: Option<&str>,
) -> Result<(), String> {
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO relay_webhook_callback_events
             (id, request_id, trigger_id, delivery_id, status, channel, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, 'received', ?5, ?6)",
            rusqlite::params![
                make_main_id("relay_wh"),
                request_id.trim(),
                trigger_id,
                delivery_id,
                sanitize_approval_resolution_field(channel, 32),
                now_ms(),
            ],
        )
        .map_err(|e| format!("Could not record relay webhook callback event: {e}"))?;
    if inserted == 0 {
        return Err("Relay webhook callback request was already processed.".to_string());
    }
    Ok(())
}

fn update_relay_webhook_callback_event_status(
    connection: &rusqlite::Connection,
    request_id: &str,
    status: &str,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE relay_webhook_callback_events SET status = ?1 WHERE request_id = ?2",
            rusqlite::params![status, request_id.trim()],
        )
        .map_err(|e| format!("Could not update relay webhook callback status: {e}"))?;
    Ok(())
}

fn redact_webhook_headers_json(input: Option<&str>) -> String {
    let Some(raw) = input.map(str::trim).filter(|v| !v.is_empty()) else {
        return "{}".to_string();
    };
    let parsed = serde_json::from_str::<serde_json::Map<String, Value>>(raw);
    let Ok(map) = parsed else {
        return "{}".to_string();
    };
    let mut out = serde_json::Map::new();
    for (key, value) in map.into_iter().take(24) {
        let lower = key.to_ascii_lowercase();
        let redacted = if lower.contains("authorization")
            || lower.contains("cookie")
            || lower.contains("secret")
            || lower.contains("signature")
            || lower.contains("token")
        {
            Value::String("[REDACTED]".to_string())
        } else {
            let text = match value {
                Value::String(s) => s.chars().take(120).collect::<String>(),
                other => other.to_string().chars().take(120).collect::<String>(),
            };
            Value::String(sanitize_log_message(&text))
        };
        out.insert(key.chars().take(48).collect(), redacted);
    }
    serde_json::to_string(&out).unwrap_or_else(|_| "{}".to_string())
}

fn normalize_content_type(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

fn payload_excerpt_from_json(body_json: &str) -> String {
    let compact = serde_json::from_str::<Value>(body_json)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| body_json.to_string());
    sanitize_log_message(&compact.chars().take(280).collect::<String>())
}

fn payload_hash(body_json: &str) -> String {
    format!("{:x}", Sha256::digest(body_json.as_bytes()))
}

fn validate_gmail_trigger_mode(input: &str) -> Result<String, String> {
    let v = input.trim().to_ascii_lowercase();
    match v.as_str() {
        "polling" | "gmail_pubsub" | "auto" => Ok(v),
        _ => Err("Gmail trigger mode must be Polling, Gmail PubSub, or Auto.".to_string()),
    }
}

fn validate_gmail_pubsub_callback_mode(input: &str) -> Result<String, String> {
    let v = input.trim().to_ascii_lowercase();
    match v.as_str() {
        "relay" | "local_debug" => Ok(v),
        _ => Err("Gmail PubSub callback mode must be Relay or Local Debug.".to_string()),
    }
}

fn sanitize_gmail_pubsub_resource_name(raw: &str, label: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 240 {
        return Err(format!("Gmail PubSub {label} is required."));
    }
    let bounded = trimmed
        .chars()
        .take(240)
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.' | ':'))
        .collect::<String>();
    if bounded.is_empty() || !bounded.contains('/') {
        return Err(format!("Gmail PubSub {label} format is invalid."));
    }
    Ok(bounded)
}

fn gmail_watch_register(access_token: &str, topic_name: &str) -> Result<(i64, String), String> {
    let client = HttpClient::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| "Could not initialize Gmail watch client.".to_string())?;
    let body = serde_json::json!({
        "topicName": topic_name,
        "labelIds": ["INBOX"],
        "labelFilterBehavior": "INCLUDE"
    });
    let json = client
        .post("https://gmail.googleapis.com/gmail/v1/users/me/watch")
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .map_err(|_| "Could not register Gmail PubSub watch. Check connection and try again.".to_string())?
        .error_for_status()
        .map_err(|e| {
            if e.status().map(|s| s.as_u16()) == Some(429) {
                "Gmail watch registration is rate-limited right now. Try again shortly.".to_string()
            } else {
                "Could not register Gmail PubSub watch. Check Gmail PubSub topic settings and try again.".to_string()
            }
        })?
        .json::<Value>()
        .map_err(|_| "Could not parse Gmail watch registration response.".to_string())?;
    let expiration_ms = json
        .get("expiration")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse::<i64>().ok())
                .or_else(|| v.as_i64())
        })
        .ok_or_else(|| "Gmail watch response is missing expiration.".to_string())?;
    let history_id = json
        .get("historyId")
        .and_then(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .or_else(|| v.as_i64().map(|n| n.to_string()))
        })
        .ok_or_else(|| "Gmail watch response is missing history id.".to_string())?;
    Ok((expiration_ms, history_id))
}

fn reserve_relay_gmail_pubsub_callback_event(
    connection: &rusqlite::Connection,
    request_id: &str,
    channel: Option<&str>,
) -> Result<(), String> {
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO relay_gmail_pubsub_callback_events
             (id, request_id, status, channel, created_at_ms)
             VALUES (?1, ?2, 'received', ?3, ?4)",
            rusqlite::params![
                make_main_id("relay_gp"),
                request_id.trim(),
                sanitize_approval_resolution_field(channel, 32),
                now_ms()
            ],
        )
        .map_err(|e| format!("Could not record relay Gmail PubSub callback event: {e}"))?;
    if inserted == 0 {
        return Err("Relay Gmail PubSub callback request was already processed.".to_string());
    }
    Ok(())
}

fn update_relay_gmail_pubsub_callback_event_status(
    connection: &rusqlite::Connection,
    request_id: &str,
    status: &str,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE relay_gmail_pubsub_callback_events SET status = ?1 WHERE request_id = ?2",
            rusqlite::params![status, request_id.trim()],
        )
        .map_err(|e| format!("Could not update relay Gmail PubSub callback status: {e}"))?;
    Ok(())
}

fn run_gmail_watcher_from_control(
    connection: &mut rusqlite::Connection,
) -> Result<inbox_watcher::InboxWatcherTickSummary, String> {
    let control = db::get_runner_control(connection)?;
    inbox_watcher::run_watcher_tick(
        connection,
        "gmail",
        &control.gmail_autopilot_id,
        control.watcher_max_items as usize,
    )
}

fn ingest_gmail_pubsub_event_internal<F>(
    connection: &mut rusqlite::Connection,
    relay_request_id: Option<String>,
    relay_channel: Option<&str>,
    body_json: &str,
    require_relay_callback_auth: bool,
    fetch_and_queue: F,
) -> Result<GmailPubSubIngestResult, String>
where
    F: FnOnce(&mut rusqlite::Connection) -> Result<inbox_watcher::InboxWatcherTickSummary, String>,
{
    let now = now_ms();
    if require_relay_callback_auth {
        let request_id = relay_request_id.as_deref().unwrap_or("");
        if let Err(err) =
            reserve_relay_gmail_pubsub_callback_event(connection, request_id, relay_channel)
        {
            if err.contains("already processed") {
                return Ok(GmailPubSubIngestResult {
                    status: "duplicate".to_string(),
                    event_dedupe_key: request_id.to_string(),
                    created_run_count: 0,
                    message: "Relay Gmail PubSub callback request was already processed."
                        .to_string(),
                });
            }
            return Err(err);
        }
    }

    let env = match gmail_pubsub::parse_pubsub_envelope(body_json) {
        Ok(v) => v,
        Err(err) => {
            let _ = gmail_pubsub::record_failure(connection, &err, now);
            if require_relay_callback_auth {
                let _ = update_relay_gmail_pubsub_callback_event_status(
                    connection,
                    relay_request_id.as_deref().unwrap_or(""),
                    "rejected",
                );
            }
            return Ok(GmailPubSubIngestResult {
                status: "rejected".to_string(),
                event_dedupe_key: "invalid".to_string(),
                created_run_count: 0,
                message: err,
            });
        }
    };

    let inserted = gmail_pubsub::insert_event(
        connection,
        &gmail_pubsub::GmailPubSubEventInsert {
            id: make_main_id("gpub_evt"),
            provider: "gmail".to_string(),
            message_id: Some(env.message_id.clone()),
            event_dedupe_key: env.dedupe_key.clone(),
            history_id: env.history_id.clone(),
            published_at_ms: env.published_at_ms,
            received_at_ms: now,
            status: "accepted".to_string(),
            failure_reason: None,
            created_run_count: 0,
            created_at_ms: now,
        },
    )?;
    if !inserted {
        if require_relay_callback_auth {
            let _ = update_relay_gmail_pubsub_callback_event_status(
                connection,
                relay_request_id.as_deref().unwrap_or(""),
                "duplicate",
            );
        }
        return Ok(GmailPubSubIngestResult {
            status: "duplicate".to_string(),
            event_dedupe_key: env.dedupe_key,
            created_run_count: 0,
            message: "Duplicate Gmail PubSub event ignored.".to_string(),
        });
    }

    gmail_pubsub::update_event_status(connection, &env.dedupe_key, "queued_fetch", None, None)?;
    match fetch_and_queue(connection) {
        Ok(summary) => {
            gmail_pubsub::update_event_status(
                connection,
                &env.dedupe_key,
                "accepted",
                None,
                Some(summary.started_runs as i64),
            )?;
            gmail_pubsub::touch_event_success(connection, now, env.history_id.as_deref())?;
            if require_relay_callback_auth {
                update_relay_gmail_pubsub_callback_event_status(
                    connection,
                    relay_request_id.as_deref().unwrap_or(""),
                    "applied",
                )?;
            }
            Ok(GmailPubSubIngestResult {
                status: "accepted".to_string(),
                event_dedupe_key: env.dedupe_key,
                created_run_count: summary.started_runs as i64,
                message: "Gmail PubSub event accepted and inbox fetch queued.".to_string(),
            })
        }
        Err(err) => {
            let msg = sanitize_log_message(&err);
            let _ = gmail_pubsub::update_event_status(
                connection,
                &env.dedupe_key,
                "fetch_failed",
                Some(&msg),
                Some(0),
            );
            let _ = gmail_pubsub::record_failure(connection, &msg, now);
            if require_relay_callback_auth {
                let _ = update_relay_gmail_pubsub_callback_event_status(
                    connection,
                    relay_request_id.as_deref().unwrap_or(""),
                    "fetch_failed",
                );
            }
            Ok(GmailPubSubIngestResult {
                status: "fetch_failed".to_string(),
                event_dedupe_key: env.dedupe_key,
                created_run_count: 0,
                message: "Gmail PubSub event received, but inbox fetch failed. Terminus will keep polling fallback available.".to_string(),
            })
        }
    }
}

fn validate_webhook_signature(
    secret: &str,
    body_json: &str,
    signature: &str,
    signature_ts_ms: i64,
) -> Result<(), String> {
    if signature_ts_ms <= 0 || (now_ms() - signature_ts_ms).abs() > 15 * 60 * 1000 {
        return Err(
            "Webhook signature timestamp is expired. Retry from the source system.".to_string(),
        );
    }
    let provided = signature
        .trim()
        .strip_prefix("sha256=")
        .unwrap_or(signature.trim())
        .to_ascii_lowercase();
    if provided.is_empty() || provided.len() > 256 {
        return Err("Webhook signature is invalid.".to_string());
    }
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|_| "Webhook signature key is invalid.".to_string())?;
    let signed = format!("{}.{}", signature_ts_ms, body_json);
    mac.update(signed.as_bytes());
    let expected = format!("{:x}", mac.finalize().into_bytes());
    if !constant_time_eq(&expected, &provided) {
        return Err("Webhook signature check failed.".to_string());
    }
    Ok(())
}

fn build_webhook_run_plan(
    route: &webhook_triggers::WebhookTriggerRouteConfig,
    body_json: &str,
    payload_hash_hex: &str,
    received_at_ms: i64,
) -> Result<AutopilotPlan, String> {
    let mut plan: AutopilotPlan = serde_json::from_str(&route.plan_json)
        .map_err(|e| format!("Webhook trigger plan snapshot is invalid: {e}"))?;
    if plan.recipe == RecipeKind::Custom {
        let provider_id = parse_provider(&route.provider_kind)?;
        plan = validate_custom_execution_plan(plan, provider_id)?;
    }
    let excerpt = payload_excerpt_from_json(body_json);
    let event_summary = format!(
        "Webhook event from trigger {} at {} (hash {}). Payload excerpt: {}",
        route.trigger_id,
        received_at_ms,
        &payload_hash_hex[..payload_hash_hex.len().min(12)],
        excerpt
    );
    if plan
        .inbox_source_text
        .as_ref()
        .is_none_or(|v| v.trim().is_empty())
    {
        plan.inbox_source_text = Some(event_summary.clone());
    } else if let Some(existing) = plan.inbox_source_text.clone() {
        let mut merged = existing;
        merged.push_str("\n\n");
        merged.push_str(&event_summary);
        plan.inbox_source_text = Some(merged.chars().take(4_000).collect());
    }
    plan.intent = format!(
        "{} [Webhook trigger {}]",
        plan.intent.trim(),
        route.trigger_id
    )
    .chars()
    .take(240)
    .collect();
    Ok(plan)
}

fn insert_webhook_run_activity(
    connection: &rusqlite::Connection,
    run_id: &str,
    trigger_id: &str,
    delivery_id: &str,
    channel: Option<&str>,
) {
    let message = format!(
        "Webhook event queued from {} (delivery {}).",
        trigger_id,
        delivery_id.chars().take(48).collect::<String>()
    );
    let _ = connection.execute(
        "INSERT INTO activities (id, run_id, activity_type, from_state, to_state, user_message, created_at)
         VALUES (?1, ?2, 'webhook_event_queued', NULL, NULL, ?3, ?4)",
        rusqlite::params![
            make_main_id("activity"),
            run_id,
            truncate_for_activity(&message),
            now_ms(),
        ],
    );
    let _ = connection.execute(
        "INSERT INTO activities (id, run_id, activity_type, from_state, to_state, user_message, created_at)
         VALUES (?1, ?2, 'webhook_origin', NULL, NULL, ?3, ?4)",
        rusqlite::params![
            make_main_id("activity"),
            run_id,
            truncate_for_activity(&format!(
                "Origin: webhook via {}",
                channel.unwrap_or("relay_webhook")
            )),
            now_ms(),
        ],
    );
}

fn ingest_webhook_event_internal(
    connection: &mut rusqlite::Connection,
    input: WebhookIngestInput,
) -> Result<WebhookIngestResult, String> {
    let trigger_id = input.trigger_id.trim().to_string();
    if trigger_id.is_empty() {
        return Err("Trigger ID is required.".to_string());
    }
    let delivery_id = input.delivery_id.trim().to_string();
    if delivery_id.is_empty() || delivery_id.len() > 200 {
        return Err("Webhook delivery ID is invalid.".to_string());
    }
    if input.require_relay_callback_auth {
        validate_relay_callback_auth_fields(
            input.relay_request_id.as_deref().unwrap_or(""),
            input.relay_callback_secret.as_deref().unwrap_or(""),
            input.relay_issued_at_ms.unwrap_or_default(),
            "Remote webhook delivery is not ready yet. Generate a callback secret first.",
        )?;
        if let Err(err) = reserve_relay_webhook_callback_event(
            connection,
            input.relay_request_id.as_deref().unwrap_or(""),
            &trigger_id,
            &delivery_id,
            input.relay_channel.as_deref(),
        ) {
            if err.contains("already processed") {
                return Ok(WebhookIngestResult {
                    status: "duplicate".to_string(),
                    trigger_id,
                    delivery_id,
                    run_id: None,
                    message: "Relay webhook callback request was already processed.".to_string(),
                });
            }
            return Err(err);
        }
    }

    let route = webhook_triggers::get_webhook_trigger_route_config(connection, &trigger_id)?
        .ok_or_else(|| "Webhook trigger not found.".to_string())?;
    let now = now_ms();
    let content_type = normalize_content_type(&input.content_type);
    let body_json = input.body_json.trim().to_string();
    let body_len = body_json.as_bytes().len() as i64;
    let hash = payload_hash(&body_json);
    let event_key = format!("{}:{}", delivery_id, &hash[..hash.len().min(16)]);
    let headers_redacted_json = redact_webhook_headers_json(input.headers_redacted_json.as_deref());
    let payload_excerpt = payload_excerpt_from_json(&body_json);

    let base_event = webhook_triggers::WebhookTriggerEventInsert {
        id: make_main_id("wh_event"),
        trigger_id: trigger_id.clone(),
        delivery_id: delivery_id.clone(),
        event_idempotency_key: event_key.clone(),
        received_at_ms: now,
        status: "accepted".to_string(),
        http_status: Some(202),
        headers_redacted_json,
        payload_excerpt,
        payload_hash: hash.clone(),
        failure_reason: None,
        run_id: None,
    };
    let inserted = webhook_triggers::insert_webhook_trigger_event(connection, &base_event)?;
    if !inserted {
        if input.require_relay_callback_auth {
            let _ = update_relay_webhook_callback_event_status(
                connection,
                input.relay_request_id.as_deref().unwrap_or(""),
                "duplicate",
            );
        }
        return Ok(WebhookIngestResult {
            status: "duplicate".to_string(),
            trigger_id,
            delivery_id,
            run_id: None,
            message: "Duplicate webhook delivery ignored.".to_string(),
        });
    }

    let fail = |status: &str,
                reason: &str,
                http_status: Option<i64>|
     -> Result<WebhookIngestResult, String> {
        let _ = webhook_triggers::update_webhook_trigger_event_status(
            connection,
            &trigger_id,
            &event_key,
            status,
            Some(reason),
            None,
        );
        let _ = webhook_triggers::touch_webhook_trigger_delivery(
            connection,
            &trigger_id,
            now,
            Some(reason),
        );
        if input.require_relay_callback_auth {
            let _ = update_relay_webhook_callback_event_status(
                connection,
                input.relay_request_id.as_deref().unwrap_or(""),
                status,
            );
        }
        if let Some(code) = http_status {
            let _ = connection.execute(
                "UPDATE webhook_trigger_events SET http_status = ?1 WHERE trigger_id = ?2 AND event_idempotency_key = ?3",
                rusqlite::params![code, &trigger_id, &event_key],
            );
        }
        Ok(WebhookIngestResult {
            status: status.to_string(),
            trigger_id: trigger_id.clone(),
            delivery_id: delivery_id.clone(),
            run_id: None,
            message: reason.to_string(),
        })
    };

    if route.status != "active" {
        return fail(
            "rejected",
            "Webhook trigger is paused. Enable it to accept events.",
            Some(409),
        );
    }
    if content_type != "application/json"
        || !route
            .allowed_content_types
            .iter()
            .any(|v| normalize_content_type(v) == content_type)
    {
        return fail(
            "failed_validation",
            "Unsupported webhook content type. Terminus currently accepts JSON only.",
            Some(415),
        );
    }
    if body_len <= 0 || body_len > route.max_payload_bytes {
        return fail(
            "failed_validation",
            "Webhook payload is too large for this trigger. Reduce payload size or raise the trigger limit.",
            Some(413),
        );
    }
    if serde_json::from_str::<Value>(&body_json).is_err() {
        return fail(
            "failed_validation",
            "Webhook payload must be valid JSON.",
            Some(400),
        );
    }
    if input.require_webhook_signature {
        let secret = providers::keychain::get_webhook_trigger_secret(&trigger_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| {
                "Webhook trigger signing secret is missing. Rotate the secret and retry."
                    .to_string()
            })?;
        if let Err(err) = validate_webhook_signature(
            &secret,
            &body_json,
            input.signature.as_deref().unwrap_or(""),
            input.signature_ts_ms.unwrap_or_default(),
        ) {
            return fail("rejected", &err, Some(401));
        }
    }

    let plan = build_webhook_run_plan(&route, &body_json, &hash, now)?;
    let run_idempotency_key = format!("webhook:{}:{}", trigger_id, event_key);
    let run = RunnerEngine::start_run(
        connection,
        &route.autopilot_id,
        plan,
        &run_idempotency_key,
        2,
    )
    .map_err(|e| e.to_string())?;
    insert_webhook_run_activity(
        connection,
        &run.id,
        &trigger_id,
        &delivery_id,
        input.relay_channel.as_deref(),
    );
    webhook_triggers::update_webhook_trigger_event_status(
        connection,
        &trigger_id,
        &event_key,
        "queued",
        None,
        Some(&run.id),
    )?;
    webhook_triggers::touch_webhook_trigger_delivery(connection, &trigger_id, now, None)?;
    if input.require_relay_callback_auth {
        update_relay_webhook_callback_event_status(
            connection,
            input.relay_request_id.as_deref().unwrap_or(""),
            "applied",
        )?;
    }
    Ok(WebhookIngestResult {
        status: "queued".to_string(),
        trigger_id,
        delivery_id,
        run_id: Some(run.id),
        message: "Webhook accepted and run queued.".to_string(),
    })
}

fn validate_voice_tone(input: &str) -> Result<String, String> {
    let value = input.trim().to_ascii_lowercase();
    match value.as_str() {
        "professional" | "neutral" | "warm" => Ok(value),
        _ => Err("Voice tone must be Professional, Neutral, or Warm.".to_string()),
    }
}

fn validate_voice_length(input: &str) -> Result<String, String> {
    let value = input.trim().to_ascii_lowercase();
    match value.as_str() {
        "concise" | "normal" | "detailed" => Ok(value),
        _ => Err("Voice length must be Concise, Normal, or Detailed.".to_string()),
    }
}

fn validate_voice_humor(input: &str) -> Result<String, String> {
    let value = input.trim().to_ascii_lowercase();
    match value.as_str() {
        "off" | "light" => Ok(value),
        _ => Err("Voice humor must be Off or Light.".to_string()),
    }
}

fn sanitize_voice_notes(input: &str) -> String {
    input
        .trim()
        .chars()
        .take(800)
        .collect::<String>()
        .replace('\u{0000}', "")
}

fn annotate_approval_resolution(
    connection: &rusqlite::Connection,
    approval_id: &str,
    channel: Option<&str>,
    actor_label: Option<&str>,
) -> Result<(), String> {
    let channel = sanitize_approval_resolution_field(channel, 32);
    let actor = sanitize_approval_resolution_field(actor_label, 80);
    if channel.is_none() && actor.is_none() {
        return Ok(());
    }
    connection
        .execute(
            "UPDATE approvals
             SET decided_channel = COALESCE(?1, decided_channel),
                 decided_by = COALESCE(?2, decided_by)
             WHERE id = ?3",
            rusqlite::params![channel, actor, approval_id],
        )
        .map_err(|e| format!("Could not record approval source metadata: {e}"))?;
    Ok(())
}

fn approve_run_approval_with_context(
    connection: &mut rusqlite::Connection,
    approval_id: &str,
    channel: Option<&str>,
    actor_label: Option<&str>,
) -> Result<RunRecord, String> {
    annotate_approval_resolution(connection, approval_id, channel, actor_label)?;
    RunnerEngine::approve(connection, approval_id).map_err(|e| e.to_string())
}

fn reject_run_approval_with_context(
    connection: &mut rusqlite::Connection,
    approval_id: &str,
    reason: Option<String>,
    channel: Option<&str>,
    actor_label: Option<&str>,
) -> Result<RunRecord, String> {
    annotate_approval_resolution(connection, approval_id, channel, actor_label)?;
    RunnerEngine::reject(connection, approval_id, reason).map_err(|e| e.to_string())
}

fn parse_recipe(value: &str) -> Result<RecipeKind, String> {
    match value {
        "website_monitor" => Ok(RecipeKind::WebsiteMonitor),
        "inbox_triage" => Ok(RecipeKind::InboxTriage),
        "daily_brief" => Ok(RecipeKind::DailyBrief),
        "custom" => Ok(RecipeKind::Custom),
        _ => Err(format!("Unknown recipe: {value}")),
    }
}

fn parse_provider(value: &str) -> Result<ProviderId, String> {
    match value {
        "openai" => Ok(ProviderId::OpenAi),
        "anthropic" => Ok(ProviderId::Anthropic),
        "gemini" => Ok(ProviderId::Gemini),
        _ => Err(format!("Unknown provider: {value}")),
    }
}

fn parse_intent_kind(value: &str) -> Result<IntentDraftKind, String> {
    match value {
        "one_off_run" => Ok(IntentDraftKind::OneOffRun),
        "draft_autopilot" => Ok(IntentDraftKind::DraftAutopilot),
        _ => Err(format!("Unknown intent kind: {value}")),
    }
}

fn classify_intent_kind(intent: &str) -> (IntentDraftKind, String) {
    let normalized = intent.to_ascii_lowercase();
    let recurring_hints = [
        "every",
        "daily",
        "weekly",
        "monitor",
        "watch",
        "always",
        "whenever",
        "keep an eye",
    ];
    let should_recur = recurring_hints.iter().any(|hint| normalized.contains(hint))
        || normalized.contains("inbox");

    if should_recur {
        (
            IntentDraftKind::DraftAutopilot,
            "Looks recurring, so Terminus prepared an Autopilot setup.".to_string(),
        )
    } else {
        (
            IntentDraftKind::OneOffRun,
            "Looks one-time, so Terminus prepared a one-off Run.".to_string(),
        )
    }
}

fn classify_recipe(intent: &str) -> RecipeKind {
    let normalized = intent.to_ascii_lowercase();
    if normalized.contains("inbox")
        || normalized.contains("email")
        || normalized.contains("reply")
        || normalized.contains("triage")
    {
        return RecipeKind::InboxTriage;
    }
    if normalized.contains("monitor")
        || normalized.contains("website")
        || normalized.contains("web page")
        || normalized.contains("url")
        || ((normalized.contains("http://") || normalized.contains("https://"))
            && !normalized.contains("email"))
    {
        return RecipeKind::WebsiteMonitor;
    }
    if normalized.contains("brief")
        || normalized.contains("summary")
        || normalized.contains("digest")
    {
        return RecipeKind::DailyBrief;
    }
    let custom_signals = [
        "chase",
        "follow up",
        "follow-up",
        "remind",
        "coordinate",
        "parse",
        "categorize",
        "extract",
        "prepare",
        "compile",
        "collect updates",
        "generate report",
        "proposal",
        "contract",
        "invoice",
        "receipt",
        "every friday",
        "every monday",
        "every week",
        "spreadsheet",
        "excel",
        "automate",
    ];
    if custom_signals
        .iter()
        .any(|signal| normalized.contains(signal))
    {
        return RecipeKind::Custom;
    }
    RecipeKind::DailyBrief
}

#[derive(Debug, Deserialize)]
struct GeneratedCustomPlan {
    steps: Vec<GeneratedCustomStep>,
    #[serde(default)]
    web_allowed_domains: Vec<String>,
    #[serde(default)]
    recipient_hints: Vec<String>,
    #[serde(default)]
    allowed_primitives: Vec<String>,
    #[serde(default)]
    api_call_request: Option<GeneratedApiCallRequest>,
}

#[derive(Debug, Deserialize)]
struct GeneratedCustomStep {
    id: String,
    label: String,
    primitive: String,
    requires_approval: bool,
    risk_tier: String,
}

#[derive(Debug, Deserialize)]
struct GeneratedApiCallRequest {
    url: String,
    #[serde(default)]
    method: Option<String>,
    header_key_ref: String,
    #[serde(default)]
    auth_header_name: Option<String>,
    #[serde(default)]
    auth_scheme: Option<String>,
    #[serde(default)]
    body_json: Option<String>,
}

fn provider_kind_for_schema(provider_id: ProviderId) -> ApiProviderKind {
    match provider_id {
        ProviderId::OpenAi => ApiProviderKind::OpenAi,
        ProviderId::Anthropic => ApiProviderKind::Anthropic,
        ProviderId::Gemini => ApiProviderKind::Gemini,
    }
}

fn provider_tier_for_schema(provider_id: ProviderId) -> ApiProviderTier {
    match provider_id {
        ProviderId::OpenAi | ProviderId::Anthropic => ApiProviderTier::Supported,
        ProviderId::Gemini => ApiProviderTier::Experimental,
    }
}

fn parse_generated_primitive_id(raw: &str) -> Result<PrimitiveId, String> {
    let normalized = raw.trim().to_ascii_lowercase().replace(['.', '-'], "_");
    match normalized.as_str() {
        "readweb" | "read_web" => Ok(PrimitiveId::ReadWeb),
        "readsources" | "read_sources" => Ok(PrimitiveId::ReadSources),
        "readforwardedemail" | "read_forwarded_email" => Ok(PrimitiveId::ReadForwardedEmail),
        "callapi" | "call_api" => Ok(PrimitiveId::CallApi),
        "triageemail" | "triage_email" => Ok(PrimitiveId::TriageEmail),
        "aggregatedailysummary" | "aggregate_daily_summary" => {
            Ok(PrimitiveId::AggregateDailySummary)
        }
        "readvaultfile" | "read_vault_file" => Ok(PrimitiveId::ReadVaultFile),
        "writeoutcomedraft" | "write_outcome_draft" => Ok(PrimitiveId::WriteOutcomeDraft),
        "writeemaildraft" | "write_email_draft" => Ok(PrimitiveId::WriteEmailDraft),
        "sendemail" | "send_email" => Ok(PrimitiveId::SendEmail),
        "schedulerun" | "schedule_run" => Ok(PrimitiveId::ScheduleRun),
        "notifyuser" | "notify_user" => Ok(PrimitiveId::NotifyUser),
        _ => Err(format!("Unknown primitive in generated plan: {raw}")),
    }
}

fn parse_generated_risk_tier(raw: &str) -> Result<RiskTier, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "low" => Ok(RiskTier::Low),
        "medium" => Ok(RiskTier::Medium),
        "high" => Ok(RiskTier::High),
        _ => Err(format!("Unknown risk tier in generated plan: {raw}")),
    }
}

fn sanitize_api_key_ref_name(raw: &str) -> Result<String, String> {
    let cleaned = raw
        .trim()
        .chars()
        .take(64)
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        .collect::<String>();
    if cleaned.is_empty() {
        return Err("API key reference name is required.".to_string());
    }
    Ok(cleaned)
}

fn normalize_api_call_method(raw: &str) -> Result<String, String> {
    let method = raw.trim().to_ascii_uppercase();
    match method.as_str() {
        "GET" | "POST" => Ok(method),
        _ => Err("CallApi method must be GET or POST in MVP.".to_string()),
    }
}

fn normalize_auth_header_name(raw: &str) -> Result<String, String> {
    let cleaned = raw
        .trim()
        .chars()
        .take(48)
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>();
    if cleaned.is_empty() {
        return Err("CallApi auth header name is invalid.".to_string());
    }
    Ok(cleaned)
}

fn normalize_auth_scheme(raw: &str) -> Result<String, String> {
    let value = raw.trim().to_ascii_lowercase();
    match value.as_str() {
        "bearer" | "raw" => Ok(value),
        _ => Err("CallApi auth scheme must be bearer or raw in MVP.".to_string()),
    }
}

fn validate_api_call_request_config(
    config: ApiCallRequest,
    allowlisted_domains: &mut Vec<String>,
) -> Result<ApiCallRequest, String> {
    let url = config.url.trim().to_string();
    let (scheme, host) = crate::web::parse_scheme_host(&url)
        .ok_or_else(|| "CallApi URL must be a valid HTTP/HTTPS URL.".to_string())?;
    if scheme != "http" && scheme != "https" {
        return Err("CallApi URL must use HTTP or HTTPS.".to_string());
    }
    if !allowlisted_domains
        .iter()
        .any(|d| d.eq_ignore_ascii_case(&host))
    {
        allowlisted_domains.push(host.to_ascii_lowercase());
    }
    let method = normalize_api_call_method(&config.method)?;
    let header_key_ref = sanitize_api_key_ref_name(&config.header_key_ref)?;
    let auth_header_name = normalize_auth_header_name(&config.auth_header_name)?;
    let auth_scheme = normalize_auth_scheme(&config.auth_scheme)?;
    let body_json = config
        .body_json
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.len() > 8_000 {
                return Err("CallApi request body is too large for MVP.".to_string());
            }
            serde_json::from_str::<serde_json::Value>(s)
                .map_err(|_| "CallApi request body must be valid JSON.".to_string())?;
            Ok(s.to_string())
        })
        .transpose()?;
    if method == "GET" && body_json.is_some() {
        return Err("CallApi GET requests cannot include a JSON body in MVP.".to_string());
    }
    Ok(ApiCallRequest {
        url,
        method,
        header_key_ref,
        auth_header_name,
        auth_scheme,
        body_json,
    })
}

fn generated_api_call_to_schema(
    generated: GeneratedApiCallRequest,
    allowlisted_domains: &mut Vec<String>,
) -> Result<ApiCallRequest, String> {
    validate_api_call_request_config(
        ApiCallRequest {
            url: generated.url,
            method: generated.method.unwrap_or_else(|| "GET".to_string()),
            header_key_ref: generated.header_key_ref,
            auth_header_name: generated
                .auth_header_name
                .unwrap_or_else(|| "Authorization".to_string()),
            auth_scheme: generated
                .auth_scheme
                .unwrap_or_else(|| "bearer".to_string()),
            body_json: generated.body_json,
        },
        allowlisted_domains,
    )
}

fn validate_custom_execution_plan(
    mut plan: AutopilotPlan,
    provider_id: ProviderId,
) -> Result<AutopilotPlan, String> {
    if plan.recipe != RecipeKind::Custom {
        return Err("Custom plan payload must use recipe=custom.".to_string());
    }
    if plan.steps.is_empty() {
        return Err("Custom plan must include at least one step.".to_string());
    }
    if plan.steps.len() > 10 {
        return Err("Custom plan exceeds the maximum of 10 steps.".to_string());
    }
    if plan
        .steps
        .iter()
        .any(|s| s.id.trim().is_empty() || s.label.trim().is_empty())
    {
        return Err("Every custom plan step needs an id and label.".to_string());
    }

    let mut used = Vec::<PrimitiveId>::new();
    for step in &mut plan.steps {
        match step.primitive {
            PrimitiveId::CallApi => {
                step.requires_approval = true;
                step.risk_tier = RiskTier::High;
            }
            PrimitiveId::SendEmail => {
                step.requires_approval = true;
                step.risk_tier = RiskTier::High;
            }
            PrimitiveId::WriteOutcomeDraft
            | PrimitiveId::WriteEmailDraft
            | PrimitiveId::TriageEmail => {
                step.requires_approval = true;
                if step.risk_tier == RiskTier::Low {
                    step.risk_tier = RiskTier::Medium;
                }
            }
            PrimitiveId::ScheduleRun | PrimitiveId::ReadVaultFile => {
                return Err(format!(
                    "This action isn't allowed in Terminus yet: {}.",
                    step.label
                ));
            }
            _ => {}
        }
        if !used.contains(&step.primitive) {
            used.push(step.primitive);
        }
    }
    plan.allowed_primitives = used;

    plan.provider = schema::ProviderMetadata::from_provider_id(provider_id);
    plan.web_allowed_domains = plan
        .web_allowed_domains
        .into_iter()
        .map(|d| d.trim().trim_matches('.').to_ascii_lowercase())
        .filter(|d| !d.is_empty())
        .collect();
    if let Some(url) = plan.web_source_url.clone() {
        if let Some((_, host)) = crate::web::parse_scheme_host(&url) {
            if !plan
                .web_allowed_domains
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&host))
            {
                plan.web_allowed_domains.push(host);
            }
        }
    }
    for source in &plan.daily_sources {
        if let Some((_, host)) = crate::web::parse_scheme_host(source) {
            if !plan
                .web_allowed_domains
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&host))
            {
                plan.web_allowed_domains.push(host);
            }
        }
    }
    if let Some(config) = plan.api_call_request.clone() {
        plan.api_call_request = Some(validate_api_call_request_config(
            config,
            &mut plan.web_allowed_domains,
        )?);
    }
    plan.recipient_hints = plan
        .recipient_hints
        .into_iter()
        .map(|e| e.trim().to_ascii_lowercase())
        .filter(|e| e.contains('@'))
        .collect();

    if plan
        .steps
        .iter()
        .any(|s| s.primitive == PrimitiveId::ReadWeb)
        && (plan.web_source_url.as_deref().unwrap_or("").is_empty()
            || plan.web_allowed_domains.is_empty())
    {
        return Err(
            "Custom plan reads a website but has no allowed domains. Add a website domain and retry."
                .to_string(),
        );
    }
    if plan
        .steps
        .iter()
        .any(|s| s.primitive == PrimitiveId::ReadSources)
        && plan.daily_sources.is_empty()
    {
        return Err(
            "Custom plan reads sources but no source URLs were detected. Add source URLs and retry."
                .to_string(),
        );
    }
    if plan
        .steps
        .iter()
        .any(|s| s.primitive == PrimitiveId::CallApi)
        && plan.api_call_request.is_none()
    {
        return Err(
            "Custom plan calls an API but has no API request configuration. Add URL and key ref and retry."
                .to_string(),
        );
    }
    if plan
        .steps
        .iter()
        .any(|s| s.primitive == PrimitiveId::SendEmail)
        && plan.recipient_hints.is_empty()
    {
        return Err(
            "Custom plan sends email but has no recipient hints. Add at least one email recipient and retry."
                .to_string(),
        );
    }
    Ok(plan)
}

fn validate_and_build_custom_plan(
    intent: &str,
    provider_id: ProviderId,
    generated: GeneratedCustomPlan,
) -> Result<AutopilotPlan, String> {
    if generated.steps.is_empty() {
        return Err("Generated plan had no steps. Try a more specific request.".to_string());
    }
    if generated.steps.len() > 10 {
        return Err("Generated plan exceeded the maximum of 10 steps.".to_string());
    }

    let mut used_primitives = Vec::<PrimitiveId>::new();
    let mut steps = Vec::<PlanStep>::new();
    for (index, generated_step) in generated.steps.iter().enumerate() {
        let primitive = parse_generated_primitive_id(&generated_step.primitive)?;
        let mut risk_tier = parse_generated_risk_tier(&generated_step.risk_tier)?;
        let mut requires_approval = generated_step.requires_approval;
        match primitive {
            PrimitiveId::CallApi => {
                requires_approval = true;
                risk_tier = RiskTier::High;
            }
            PrimitiveId::SendEmail => {
                requires_approval = true;
                risk_tier = RiskTier::High;
            }
            PrimitiveId::WriteOutcomeDraft
            | PrimitiveId::WriteEmailDraft
            | PrimitiveId::TriageEmail => {
                requires_approval = true;
                if risk_tier == RiskTier::Low {
                    risk_tier = RiskTier::Medium;
                }
            }
            PrimitiveId::ScheduleRun | PrimitiveId::ReadVaultFile => {
                return Err("This action isn't allowed in Terminus yet.".to_string())
            }
            _ => {}
        }
        if !used_primitives.contains(&primitive) {
            used_primitives.push(primitive);
        }
        steps.push(PlanStep {
            id: if generated_step.id.trim().is_empty() {
                format!("step_{}", index + 1)
            } else {
                generated_step.id.trim().to_string()
            },
            label: generated_step.label.trim().to_string(),
            primitive,
            requires_approval,
            risk_tier,
        });
    }

    let inferred_urls = intent
        .split_whitespace()
        .filter_map(|token| {
            let normalized = token.trim_matches(|c: char| ",.;:!?()[]{}<>\"'".contains(c));
            if normalized.starts_with("http://") || normalized.starts_with("https://") {
                Some(normalized.to_string())
            } else {
                None
            }
        })
        .collect::<Vec<String>>();
    let web_source_url = inferred_urls.first().cloned();
    let daily_sources = inferred_urls
        .iter()
        .take(5)
        .cloned()
        .collect::<Vec<String>>();

    let mut web_allowed_domains = generated.web_allowed_domains;
    let api_call_request = generated
        .api_call_request
        .map(|cfg| generated_api_call_to_schema(cfg, &mut web_allowed_domains))
        .transpose()?;

    let plan = AutopilotPlan {
        schema_version: "1.0".to_string(),
        recipe: RecipeKind::Custom,
        intent: intent.to_string(),
        provider: schema::ProviderMetadata::from_provider_id(provider_id),
        web_source_url,
        web_allowed_domains,
        inbox_source_text: None,
        daily_sources,
        api_call_request,
        recipient_hints: generated.recipient_hints,
        allowed_primitives: if generated.allowed_primitives.is_empty() {
            used_primitives
        } else {
            used_primitives
        },
        steps,
    };
    validate_custom_execution_plan(plan, provider_id)
}

fn generate_custom_plan(intent: &str, provider_id: ProviderId) -> Result<AutopilotPlan, String> {
    let prompt = format!(
        concat!(
            "Generate a Terminus execution plan as JSON only.\n",
            "Intent: {intent}\n\n",
            "Use only these primitive ids (snake_case): read_web, read_sources, read_forwarded_email, triage_email, aggregate_daily_summary, write_outcome_draft, write_email_draft, send_email, notify_user.\n",
            "You may also use: call_api (approval-gated, bounded HTTP GET/POST to allowlisted domain with Keychain ref).\n",
            "Do not use schedule_run or read_vault_file.\n",
            "Required JSON shape:\n",
            "{{\"steps\":[{{\"id\":\"step_1\",\"label\":\"...\",\"primitive\":\"read_web\",\"requires_approval\":false,\"risk_tier\":\"low\"}}],\"web_allowed_domains\":[\"example.com\"],\"recipient_hints\":[\"person@example.com\"],\"allowed_primitives\":[\"read_web\"],\"api_call_request\":null}}\n",
            "If using call_api include api_call_request: {{\"url\":\"https://api.example.com/v1/items\",\"method\":\"GET|POST\",\"header_key_ref\":\"crm_prod\",\"auth_header_name\":\"Authorization\",\"auth_scheme\":\"bearer|raw\",\"body_json\":\"{{...}}\"}}\n",
            "Rules:\n",
            "- call_api must be approval-gated and high risk\n",
            "- send_email must be high risk and approval-gated\n",
            "- write_outcome_draft and write_email_draft should be approval-gated\n",
            "- Keep step count between 1 and 10\n",
            "- Output JSON only, no markdown"
        ),
        intent = intent
    );
    let request = ProviderRequest {
        provider_kind: provider_kind_for_schema(provider_id),
        provider_tier: provider_tier_for_schema(provider_id),
        model: schema::ProviderMetadata::from_provider_id(provider_id).default_model,
        input: prompt,
        max_output_tokens: Some(900),
        correlation_id: Some(format!("plan_gen:{}", make_main_id("req"))),
    };
    let response = ProviderRuntime::default()
        .dispatch(&request)
        .map_err(|e| format!("Could not generate a custom plan yet: {e}"))?;
    let generated: GeneratedCustomPlan = serde_json::from_str(response.text.trim())
        .map_err(|e| format!("Plan generation returned invalid JSON: {e}"))?;
    validate_and_build_custom_plan(intent, provider_id, generated)
}

fn describe_primitive_read(primitive: PrimitiveId) -> Option<String> {
    match primitive {
        PrimitiveId::ReadWeb => Some("Read website content from allowlisted domains".to_string()),
        PrimitiveId::ReadSources => Some("Read configured sources for this brief".to_string()),
        PrimitiveId::ReadForwardedEmail => {
            Some("Read forwarded or pasted inbox content".to_string())
        }
        PrimitiveId::CallApi => Some("Read or write a bounded external API endpoint".to_string()),
        PrimitiveId::ReadVaultFile => Some("Read connected vault files".to_string()),
        _ => None,
    }
}

fn describe_primitive_write(primitive: PrimitiveId) -> Option<String> {
    match primitive {
        PrimitiveId::TriageEmail => Some("Apply inbox filing action".to_string()),
        PrimitiveId::WriteOutcomeDraft => Some("Prepare a completed outcome".to_string()),
        PrimitiveId::WriteEmailDraft => Some("Prepare an approval-ready message".to_string()),
        PrimitiveId::SendEmail => Some("Send an email".to_string()),
        PrimitiveId::ScheduleRun => Some("Schedule this autopilot".to_string()),
        PrimitiveId::NotifyUser => Some("Send a notification".to_string()),
        PrimitiveId::CallApi => Some("Call an external API (approval-gated)".to_string()),
        _ => None,
    }
}

fn preview_for_plan(kind: &IntentDraftKind, plan: &AutopilotPlan) -> IntentDraftPreview {
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    let mut approvals_required = Vec::new();

    for step in &plan.steps {
        if let Some(read) = describe_primitive_read(step.primitive) {
            if !reads.contains(&read) {
                reads.push(read);
            }
        }
        if let Some(write) = describe_primitive_write(step.primitive) {
            if !writes.contains(&write) {
                writes.push(write);
            }
        }
        if step.requires_approval {
            approvals_required.push(step.label.clone());
        }
    }

    IntentDraftPreview {
        reads,
        writes,
        approvals_required,
        estimated_spend: "About S$0.10–S$0.60 per run".to_string(),
        primary_cta: match kind {
            IntentDraftKind::OneOffRun => "Run now".to_string(),
            IntentDraftKind::DraftAutopilot => "Run test".to_string(),
        },
    }
}

#[tauri::command]
fn draft_intent(
    intent: String,
    provider: Option<String>,
    forced_kind: Option<String>,
) -> Result<IntentDraftResponse, String> {
    let cleaned = intent.trim();
    if cleaned.is_empty() {
        return Err("Add a one-line intent to continue.".to_string());
    }
    let provider_id = match provider {
        Some(raw) => parse_provider(&raw)?,
        None => ProviderId::OpenAi,
    };

    let (auto_kind, auto_reason) = classify_intent_kind(cleaned);
    let (kind, classification_reason) = match forced_kind {
        Some(raw) => {
            let forced = parse_intent_kind(raw.trim())?;
            let reason = match forced {
                IntentDraftKind::DraftAutopilot => {
                    "Switched to recurring. Terminus prepared an Autopilot setup.".to_string()
                }
                IntentDraftKind::OneOffRun => {
                    "Switched to one-time. Terminus prepared a one-off Run.".to_string()
                }
            };
            (forced, reason)
        }
        None => (auto_kind, auto_reason),
    };
    let recipe = classify_recipe(cleaned);
    let plan = if recipe == RecipeKind::Custom {
        generate_custom_plan(cleaned, provider_id)?
    } else {
        AutopilotPlan::from_intent(recipe, cleaned.to_string(), provider_id)
    };
    let preview = preview_for_plan(&kind, &plan);

    Ok(IntentDraftResponse {
        kind,
        classification_reason,
        plan,
        preview,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_recipe_preserves_existing_signals_and_detects_custom() {
        assert_eq!(
            classify_recipe("Monitor https://example.com for changes"),
            RecipeKind::WebsiteMonitor
        );
        assert_eq!(classify_recipe("Handle my inbox"), RecipeKind::InboxTriage);
        assert_eq!(
            classify_recipe("Prepare a daily digest from these links"),
            RecipeKind::DailyBrief
        );
        assert_eq!(
            classify_recipe("Parse this invoice and categorize expenses every Friday"),
            RecipeKind::Custom
        );
    }

    #[test]
    fn validate_and_build_custom_plan_forces_send_approval_and_rejects_disallowed_primitives() {
        let generated = GeneratedCustomPlan {
            steps: vec![
                GeneratedCustomStep {
                    id: "step_1".to_string(),
                    label: "Read page".to_string(),
                    primitive: "read_web".to_string(),
                    requires_approval: false,
                    risk_tier: "low".to_string(),
                },
                GeneratedCustomStep {
                    id: "step_2".to_string(),
                    label: "Send update".to_string(),
                    primitive: "SendEmail".to_string(),
                    requires_approval: false,
                    risk_tier: "low".to_string(),
                },
            ],
            web_allowed_domains: vec!["Example.com".to_string()],
            recipient_hints: vec!["team@example.com".to_string()],
            allowed_primitives: vec!["send_email".to_string()],
            api_call_request: None,
        };
        let plan = validate_and_build_custom_plan(
            "Send updates for https://example.com",
            ProviderId::OpenAi,
            generated,
        )
        .expect("valid custom plan");
        let send_step = plan
            .steps
            .iter()
            .find(|s| s.primitive == PrimitiveId::SendEmail)
            .expect("send step");
        assert!(send_step.requires_approval);
        assert_eq!(send_step.risk_tier, RiskTier::High);
        assert!(plan.allowed_primitives.contains(&PrimitiveId::SendEmail));

        let disallowed = GeneratedCustomPlan {
            steps: vec![GeneratedCustomStep {
                id: "step_1".to_string(),
                label: "Schedule".to_string(),
                primitive: "schedule_run".to_string(),
                requires_approval: false,
                risk_tier: "low".to_string(),
            }],
            web_allowed_domains: vec![],
            recipient_hints: vec![],
            allowed_primitives: vec![],
            api_call_request: None,
        };
        let err = validate_and_build_custom_plan("Schedule this", ProviderId::OpenAi, disallowed)
            .expect_err("schedule_run must be rejected");
        assert!(err.contains("isn't allowed"));
    }

    #[test]
    fn validate_custom_execution_plan_enforces_bounds_and_required_metadata() {
        let mut plan =
            AutopilotPlan::from_intent(RecipeKind::Custom, "x".to_string(), ProviderId::OpenAi);
        assert!(validate_custom_execution_plan(plan.clone(), ProviderId::OpenAi).is_err());

        plan.steps = vec![PlanStep {
            id: "step_1".to_string(),
            label: "Read".to_string(),
            primitive: PrimitiveId::ReadWeb,
            requires_approval: false,
            risk_tier: RiskTier::Low,
        }];
        let err = validate_custom_execution_plan(plan.clone(), ProviderId::OpenAi)
            .expect_err("read_web requires allowlist");
        assert!(err.contains("allowed domains"));

        plan.web_source_url = Some("https://example.com".to_string());
        plan.web_allowed_domains = vec!["example.com".to_string()];
        let ok = validate_custom_execution_plan(plan, ProviderId::OpenAi).expect("valid");
        assert_eq!(ok.provider.id, ProviderId::OpenAi);
    }

    #[test]
    fn validate_custom_plan_call_api_requires_config_and_forces_approval() {
        let generated = GeneratedCustomPlan {
            steps: vec![GeneratedCustomStep {
                id: "step_1".to_string(),
                label: "Call CRM".to_string(),
                primitive: "call_api".to_string(),
                requires_approval: false,
                risk_tier: "low".to_string(),
            }],
            web_allowed_domains: vec![],
            recipient_hints: vec![],
            allowed_primitives: vec![],
            api_call_request: Some(GeneratedApiCallRequest {
                url: "https://api.example.com/v1/items".to_string(),
                method: Some("get".to_string()),
                header_key_ref: "crm_prod".to_string(),
                auth_header_name: Some("Authorization".to_string()),
                auth_scheme: Some("bearer".to_string()),
                body_json: None,
            }),
        };
        let plan = validate_and_build_custom_plan(
            "Call the CRM API and summarize results",
            ProviderId::OpenAi,
            generated,
        )
        .expect("valid call api custom plan");
        assert!(plan.api_call_request.is_some());
        let step = &plan.steps[0];
        assert_eq!(step.primitive, PrimitiveId::CallApi);
        assert!(step.requires_approval);
        assert_eq!(step.risk_tier, RiskTier::High);
        assert!(plan
            .web_allowed_domains
            .iter()
            .any(|d| d == "api.example.com"));

        let mut missing_cfg = plan.clone();
        missing_cfg.api_call_request = None;
        let err = validate_custom_execution_plan(missing_cfg, ProviderId::OpenAi)
            .expect_err("call_api should require config");
        assert!(err.contains("API request configuration"));
    }

    #[test]
    fn webhook_signature_validation_accepts_valid_and_rejects_invalid_signature() {
        let secret = "whsec_test";
        let body = "{\"event\":\"ok\"}";
        let ts = now_ms();
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac");
        mac.update(format!("{}.{}", ts, body).as_bytes());
        let sig = format!("sha256={:x}", mac.finalize().into_bytes());
        validate_webhook_signature(secret, body, &sig, ts).expect("valid signature");
        let err =
            validate_webhook_signature(secret, body, "sha256=deadbeef", ts).expect_err("invalid");
        assert!(err.to_ascii_lowercase().contains("signature"));
    }

    #[test]
    fn webhook_content_type_normalization_strips_charset() {
        assert_eq!(
            normalize_content_type("application/json; charset=utf-8"),
            "application/json"
        );
        assert_eq!(
            normalize_content_type(" APPLICATION/JSON "),
            "application/json"
        );
    }

    #[test]
    fn gmail_pubsub_ingest_dedupes_duplicate_event() {
        let mut conn = rusqlite::Connection::open_in_memory().expect("db");
        db::bootstrap_schema(&mut conn).expect("bootstrap");
        gmail_pubsub::upsert_state(
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
            now_ms(),
        )
        .expect("state");
        let body = r#"{"message":{"messageId":"m1","publishTime":"2026-02-25T12:00:00Z","data":"eyJoaXN0b3J5SWQiOiIxIn0="}}"#;

        let first = ingest_gmail_pubsub_event_internal(
            &mut conn,
            Some("req_1".to_string()),
            Some("local_debug"),
            body,
            false,
            |_conn| {
                Ok(inbox_watcher::InboxWatcherTickSummary {
                    provider: "gmail".to_string(),
                    autopilot_id: "auto_inbox_watch_gmail".to_string(),
                    fetched: 1,
                    deduped: 0,
                    started_runs: 1,
                    failed: 0,
                })
            },
        )
        .expect("first");
        assert_eq!(first.status, "accepted");
        assert_eq!(first.created_run_count, 1);

        let second = ingest_gmail_pubsub_event_internal(
            &mut conn,
            Some("req_2".to_string()),
            Some("local_debug"),
            body,
            false,
            |_conn| unreachable!("duplicate should not call fetch path"),
        )
        .expect("second");
        assert_eq!(second.status, "duplicate");
        assert_eq!(second.created_run_count, 0);
    }

    #[test]
    fn gmail_pubsub_ingest_records_fetch_failure_without_crashing() {
        let mut conn = rusqlite::Connection::open_in_memory().expect("db");
        db::bootstrap_schema(&mut conn).expect("bootstrap");
        gmail_pubsub::upsert_state(
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
            now_ms(),
        )
        .expect("state");
        let body = r#"{"message":{"messageId":"m2","publishTime":"2026-02-25T12:00:00Z","data":"eyJoaXN0b3J5SWQiOiIyIn0="}}"#;
        let result = ingest_gmail_pubsub_event_internal(
            &mut conn,
            Some("req_3".to_string()),
            Some("local_debug"),
            body,
            false,
            |_conn| Err("Gmail inbox is rate-limited right now.".to_string()),
        )
        .expect("result");
        assert_eq!(result.status, "fetch_failed");

        let events = gmail_pubsub::list_events(&conn, 5).expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].status, "fetch_failed");
        assert!(events[0]
            .failure_reason
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase()
            .contains("gmail"));
    }

    #[test]
    fn relay_routing_blocks_standby_when_preferred_active() {
        let mut conn = rusqlite::Connection::open_in_memory().expect("db");
        db::bootstrap_schema(&mut conn).expect("bootstrap");
        let now = now_ms();
        conn.execute(
            "INSERT INTO relay_devices (device_id, device_label, status, last_seen_at_ms, capabilities_json, is_preferred_target, updated_at_ms)
             VALUES ('dev_a','Mac A','active',?1,'{}',1,?1), ('dev_b','Mac B','standby',?1,'{}',0,?1)",
            rusqlite::params![now],
        )
        .expect("insert devices");

        let reason = relay_local_execution_allowed(&conn, "dev_b", RelayDecisionSyncChannel::Poll)
            .expect("routing check")
            .expect("should block standby");
        assert!(reason.contains("preferred device"));
    }

    #[test]
    fn relay_routing_blocks_manual_target_mode() {
        let mut conn = rusqlite::Connection::open_in_memory().expect("db");
        db::bootstrap_schema(&mut conn).expect("bootstrap");
        let now = now_ms();
        conn.execute(
            "INSERT INTO relay_devices (device_id, device_label, status, last_seen_at_ms, capabilities_json, is_preferred_target, updated_at_ms)
             VALUES ('dev_a','Mac A','active',?1,'{}',1,?1)",
            rusqlite::params![now],
        )
        .expect("insert device");
        conn.execute(
            "UPDATE relay_routing_policy SET approval_target_mode = 'manual_target_only', updated_at_ms = ?1 WHERE singleton_id = 1",
            rusqlite::params![now],
        )
        .expect("update policy");

        let reason = relay_local_execution_allowed(&conn, "dev_a", RelayDecisionSyncChannel::Push)
            .expect("routing check")
            .expect("manual target should block");
        assert!(reason.to_ascii_lowercase().contains("manual target"));
    }
}

fn main() {
    tauri::Builder::default()
        .manage(AppState::default())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let db_path = db::bootstrap_sqlite(app.handle())?;
            let state = app.state::<AppState>();
            if let Ok(mut guard) = state.db_path.lock() {
                *guard = Some(db_path.clone());
            }
            install_tray(app.handle())?;
            spawn_background_cycle_thread(app.handle(), db_path);
            if let Ok(guard) = state.db_path.lock() {
                if let Some(path) = guard.clone() {
                    spawn_background_relay_push_thread(app.handle(), path);
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app_state = window.state::<AppState>();
                let db_path = app_state.db_path.lock().ok().and_then(|g| g.clone());
                if let Some(path) = db_path {
                    if let Ok(connection) = open_connection_from_path(&path) {
                        if let Ok(control) = db::get_runner_control(&connection) {
                            if control.background_enabled {
                                api.prevent_close();
                                let _ = window.hide();
                            }
                        }
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_home_snapshot,
            list_primary_outcomes,
            get_transport_status,
            get_remote_approval_readiness,
            list_relay_devices,
            get_relay_routing_policy,
            set_relay_device_status,
            set_preferred_relay_device,
            update_relay_routing_policy,
            get_relay_sync_status,
            get_relay_push_status,
            tick_relay_approval_sync,
            tick_relay_approval_push,
            issue_relay_callback_secret,
            clear_relay_callback_secret,
            set_subscriber_token,
            remove_subscriber_token,
            set_api_key_ref,
            remove_api_key_ref,
            get_api_key_ref_status,
            probe_vault_extraction,
            get_codex_oauth_status,
            import_codex_oauth_from_local_auth,
            remove_codex_oauth,
            get_gmail_pubsub_status,
            enable_gmail_pubsub,
            disable_gmail_pubsub,
            renew_gmail_pubsub_watch,
            list_gmail_pubsub_events,
            ingest_gmail_pubsub_local_debug,
            resolve_relay_gmail_pubsub_callback,
            list_webhook_triggers,
            create_webhook_trigger,
            rotate_webhook_trigger_secret,
            disable_webhook_trigger,
            enable_webhook_trigger,
            get_webhook_trigger_events,
            ingest_webhook_event_local_debug,
            resolve_relay_webhook_callback,
            draft_intent,
            start_recipe_run,
            run_tick,
            resume_due_runs,
            create_mission_draft,
            start_mission,
            get_mission,
            list_missions,
            run_mission_tick,
            approve_run_approval,
            approve_run_approval_remote,
            reject_run_approval,
            reject_run_approval_remote,
            resolve_relay_approval_callback,
            list_pending_approvals,
            list_pending_clarifications,
            list_run_diagnostics,
            apply_intervention,
            submit_clarification_answer,
            get_run,
            get_terminal_receipt,
            list_email_connections,
            save_email_oauth_config,
            start_email_oauth,
            complete_email_oauth,
            disconnect_email_provider,
            run_inbox_watcher_tick,
            get_runner_control,
            update_runner_control,
            get_onboarding_state,
            save_onboarding_state,
            dismiss_onboarding,
            get_global_voice_config,
            update_global_voice_config,
            get_autopilot_voice_config,
            update_autopilot_voice_config,
            clear_autopilot_voice_config,
            tick_runner_cycle,
            get_autopilot_send_policy,
            update_autopilot_send_policy,
            submit_guidance,
            record_decision_event,
            compact_learning_data
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Terminus app");
}
