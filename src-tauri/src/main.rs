mod db;
mod diagnostics;
mod email_connections;
mod guidance_utils;
mod inbox_watcher;
mod learning;
mod missions;
mod primitives;
mod providers;
mod runner;
mod schema;
mod transport;
mod web;

use base64::Engine as _;
use guidance_utils::{
    classify_guidance, compute_missed_cycles, normalize_guidance_instruction, sanitize_log_message,
    GuidanceMode,
};
use providers::runtime::{ProviderRuntime, TransportStatus};
use providers::types::{
    ProviderKind as ApiProviderKind, ProviderRequest, ProviderTier as ApiProviderTier,
};
use runner::{ApprovalRecord, ClarificationRecord, RunReceipt, RunRecord, RunnerEngine};
use rusqlite::OptionalExtension;
use schema::{AutopilotPlan, PlanStep, PrimitiveId, ProviderId, RecipeKind, RiskTier};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::menu::{MenuBuilder, MenuEvent, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;

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
    watcher_poll_seconds: i64,
    watcher_max_items: i64,
    gmail_autopilot_id: String,
    microsoft_autopilot_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunnerCycleSummary {
    watcher_status: String,
    providers_polled: usize,
    fetched: usize,
    deduped: usize,
    started_runs: usize,
    failed: usize,
    resumed_due_runs: usize,
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
    let device_id = ensure_relay_device_id().map_err(|e| e.to_string())?;
    let connection = open_connection(&state)?;
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
    validate_relay_callback_auth(&input)?;
    if let Some(existing) = get_relay_callback_existing_run(&connection, &input.request_id)? {
        return Ok(existing);
    }
    let channel = input.channel.as_deref().or(Some("relay_callback"));
    let actor = input.actor_label.as_deref();
    if let Err(err) = reserve_relay_callback_event(
        &connection,
        &input.request_id,
        &input.approval_id,
        &input.decision,
        channel,
        actor,
    ) {
        if err.contains("already processed") {
            if let Some(existing) = get_relay_callback_existing_run(&connection, &input.request_id)?
            {
                return Ok(existing);
            }
        }
        return Err(err);
    }
    let run = match input.decision.trim().to_ascii_lowercase().as_str() {
        "approve" | "approved" => {
            approve_run_approval_with_context(&mut connection, &input.approval_id, channel, actor)?
        }
        "reject" | "rejected" => reject_run_approval_with_context(
            &mut connection,
            &input.approval_id,
            input.reason.clone(),
            channel,
            actor,
        )?,
        _ => return Err("Unknown approval decision. Use approve or reject.".to_string()),
    };
    update_relay_callback_event_status(&connection, &input.request_id, "applied")?;
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
            providers_polled: 0,
            fetched: 0,
            deduped: 0,
            started_runs: 0,
            failed: 0,
            resumed_due_runs: 0,
            missed_runs_detected: 0,
            catch_up_cycles_run: 0,
        });
    }
    let now = now_ms();
    let poll_ms = control.watcher_poll_seconds.saturating_mul(1000);

    let mut summary = RunnerCycleSummary {
        watcher_status: "idle".to_string(),
        providers_polled: 0,
        fetched: 0,
        deduped: 0,
        started_runs: 0,
        failed: 0,
        resumed_due_runs: 0,
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
    let request_id = input.request_id.trim();
    if request_id.is_empty() || request_id.len() > 120 {
        return Err("Relay callback request id is invalid.".to_string());
    }
    let expected = providers::keychain::get_relay_callback_secret()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "Remote approvals are not ready yet. Generate a callback secret first.".to_string()
        })?;
    if !constant_time_eq(expected.trim(), input.callback_secret.trim()) {
        return Err("Relay callback authentication failed.".to_string());
    }
    let now = now_ms();
    if input.issued_at_ms <= 0 || (now - input.issued_at_ms).abs() > 15 * 60 * 1000 {
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
}

#[derive(Debug, Deserialize)]
struct GeneratedCustomStep {
    id: String,
    label: String,
    primitive: String,
    requires_approval: bool,
    risk_tier: String,
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

    let plan = AutopilotPlan {
        schema_version: "1.0".to_string(),
        recipe: RecipeKind::Custom,
        intent: intent.to_string(),
        provider: schema::ProviderMetadata::from_provider_id(provider_id),
        web_source_url,
        web_allowed_domains: generated.web_allowed_domains,
        inbox_source_text: None,
        daily_sources,
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
            "Do not use schedule_run or read_vault_file.\n",
            "Required JSON shape:\n",
            "{{\"steps\":[{{\"id\":\"step_1\",\"label\":\"...\",\"primitive\":\"read_web\",\"requires_approval\":false,\"risk_tier\":\"low\"}}],\"web_allowed_domains\":[\"example.com\"],\"recipient_hints\":[\"person@example.com\"],\"allowed_primitives\":[\"read_web\"]}}\n",
            "Rules:\n",
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
}

fn main() {
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            let db_path = db::bootstrap_sqlite(app.handle())?;
            let state = app.state::<AppState>();
            if let Ok(mut guard) = state.db_path.lock() {
                *guard = Some(db_path.clone());
            }
            install_tray(app.handle())?;
            spawn_background_cycle_thread(app.handle(), db_path);
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
            issue_relay_callback_secret,
            clear_relay_callback_secret,
            set_subscriber_token,
            remove_subscriber_token,
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
