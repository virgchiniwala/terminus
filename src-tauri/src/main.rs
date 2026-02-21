mod db;
mod email_connections;
mod inbox_watcher;
mod learning;
mod primitives;
mod providers;
mod runner;
mod schema;
mod transport;
mod web;

use runner::{ApprovalRecord, RunReceipt, RunRecord, RunnerEngine};
use schema::{AutopilotPlan, PrimitiveId, ProviderId, RecipeKind};
use serde::Serialize;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;

#[derive(Default)]
struct AppState {
    db_path: std::sync::Mutex<Option<PathBuf>>,
}

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
    let mut control = db::get_runner_control(&connection)?;
    let now = now_ms();

    let mut summary = RunnerCycleSummary {
        watcher_status: "idle".to_string(),
        providers_polled: 0,
        fetched: 0,
        deduped: 0,
        started_runs: 0,
        failed: 0,
        resumed_due_runs: 0,
    };

    if !control.watcher_enabled {
        summary.watcher_status = "paused".to_string();
    } else if let Some(last_tick) = control.watcher_last_tick_ms {
        if now - last_tick < control.watcher_poll_seconds.saturating_mul(1000) {
            summary.watcher_status = "throttled".to_string();
        } else {
            run_watchers(&mut connection, &control, &mut summary)?;
            control.watcher_last_tick_ms = Some(now);
            db::upsert_runner_control(&connection, &control)?;
            summary.watcher_status = "ran".to_string();
        }
    } else {
        run_watchers(&mut connection, &control, &mut summary)?;
        control.watcher_last_tick_ms = Some(now);
        db::upsert_runner_control(&connection, &control)?;
        summary.watcher_status = "ran".to_string();
    }

    let resumed = RunnerEngine::resume_due_runs(&mut connection, 20).map_err(|e| e.to_string())?;
    summary.resumed_due_runs = resumed.len();
    Ok(summary)
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
        let result = inbox_watcher::run_watcher_tick(
            connection,
            &provider.provider,
            autopilot_id,
            control.watcher_max_items as usize,
        )?;
        summary.providers_polled += 1;
        summary.fetched += result.fetched;
        summary.deduped += result.deduped;
        summary.started_runs += result.started_runs;
        summary.failed += result.failed;
    }
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
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
            "Looks recurring, so Terminus prepared a Draft Autopilot.".to_string(),
        )
    } else {
        (
            IntentDraftKind::OneOffRun,
            "Looks one-time, so Terminus prepared a one-off Run draft.".to_string(),
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
        PrimitiveId::WriteOutcomeDraft => Some("Create an outcome draft".to_string()),
        PrimitiveId::WriteEmailDraft => Some("Create an email draft".to_string()),
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
fn draft_intent(intent: String, provider: Option<String>) -> Result<IntentDraftResponse, String> {
    let cleaned = intent.trim();
    if cleaned.is_empty() {
        return Err("Add a one-line intent to continue.".to_string());
    }
    let provider_id = match provider {
        Some(raw) => parse_provider(&raw)?,
        None => ProviderId::OpenAi,
    };

    let (kind, classification_reason) = classify_intent_kind(cleaned);
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
                *guard = Some(db_path);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_home_snapshot,
            draft_intent,
            start_recipe_run,
            run_tick,
            resume_due_runs,
            approve_run_approval,
            reject_run_approval,
            list_pending_approvals,
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
            record_decision_event,
            compact_learning_data
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Terminus app");
}
