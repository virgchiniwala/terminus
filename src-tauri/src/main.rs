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

use guidance_utils::{
    classify_guidance, compute_missed_cycles, normalize_guidance_instruction, sanitize_log_message,
    GuidanceMode,
};
use runner::{ApprovalRecord, ClarificationRecord, RunReceipt, RunRecord, RunnerEngine};
use rusqlite::OptionalExtension;
use schema::{AutopilotPlan, PrimitiveId, ProviderId, RecipeKind};
use serde::Serialize;
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
) -> Result<RunRecord, String> {
    let mut connection = open_connection(&state)?;
    let recipe_kind = parse_recipe(&recipe)?;
    let provider_id = parse_provider(&provider)?;
    let mut plan = AutopilotPlan::from_intent(recipe_kind, intent, provider_id);
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
    RunnerEngine::approve(&mut connection, &approval_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn reject_run_approval(
    state: tauri::State<AppState>,
    approval_id: String,
    reason: Option<String>,
) -> Result<RunRecord, String> {
    let mut connection = open_connection(&state)?;
    RunnerEngine::reject(&mut connection, &approval_id, reason).map_err(|e| e.to_string())
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

fn parse_recipe(value: &str) -> Result<RecipeKind, String> {
    match value {
        "website_monitor" => Ok(RecipeKind::WebsiteMonitor),
        "inbox_triage" => Ok(RecipeKind::InboxTriage),
        "daily_brief" => Ok(RecipeKind::DailyBrief),
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
        || normalized.contains("http://")
        || normalized.contains("https://")
        || normalized.contains("url")
    {
        return RecipeKind::WebsiteMonitor;
    }
    RecipeKind::DailyBrief
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
    let plan = AutopilotPlan::from_intent(recipe, cleaned.to_string(), provider_id);
    let preview = preview_for_plan(&kind, &plan);

    Ok(IntentDraftResponse {
        kind,
        classification_reason,
        plan,
        preview,
    })
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
            reject_run_approval,
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
