mod db;
mod learning;
mod primitives;
mod providers;
mod runner;
mod schema;
mod transport;
mod web;

use runner::{ApprovalRecord, RunReceipt, RunRecord, RunnerEngine};
use schema::{AutopilotPlan, ProviderId, RecipeKind};
use std::path::PathBuf;
use tauri::Manager;

#[derive(Default)]
struct AppState {
    db_path: std::sync::Mutex<Option<PathBuf>>,
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

fn main() {
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            let db_path = db::bootstrap_sqlite(app.handle())?;
            let state = app.state::<AppState>();
            if let Ok(mut guard) = state.db_path.lock() {
                *guard = Some(db_path);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_home_snapshot,
            start_recipe_run,
            run_tick,
            resume_due_runs,
            approve_run_approval,
            reject_run_approval,
            list_pending_approvals,
            get_run,
            get_terminal_receipt,
            record_decision_event,
            compact_learning_data
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Terminus app");
}
