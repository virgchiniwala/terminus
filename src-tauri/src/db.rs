use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tauri::Manager;

#[derive(Debug, Clone, Serialize)]
pub struct HomeSurface {
    pub title: String,
    pub subtitle: String,
    pub count: i64,
    pub cta: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunnerStatus {
    pub mode: String,
    pub status_line: String,
    pub backlog_count: i64,
    pub watcher_enabled: bool,
    pub watcher_last_tick_ms: Option<i64>,
    pub missed_runs_count: i64,
    pub suppressed_autopilots_count: i64,
    pub suppressed_autopilots: Vec<SuppressedAutopilotNotice>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SuppressedAutopilotNotice {
    pub autopilot_id: String,
    pub name: String,
    pub suppress_until_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HomeSnapshot {
    pub surfaces: Vec<HomeSurface>,
    pub runner: RunnerStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrimaryOutcomeRecord {
    pub run_id: String,
    pub autopilot_id: String,
    pub status: String, // executed | pending_approval | blocked_clarification | blocked
    pub summary: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerControlRecord {
    pub background_enabled: bool,
    pub watcher_enabled: bool,
    pub watcher_poll_seconds: i64,
    pub watcher_max_items: i64,
    pub gmail_autopilot_id: String,
    pub microsoft_autopilot_id: String,
    pub watcher_last_tick_ms: Option<i64>,
    pub missed_runs_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutopilotSendPolicyRecord {
    pub autopilot_id: String,
    pub allow_sending: bool,
    pub recipient_allowlist: Vec<String>,
    pub max_sends_per_day: i64,
    pub quiet_hours_start_local: i64,
    pub quiet_hours_end_local: i64,
    pub allow_outside_quiet_hours: bool,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionEventInsert {
    pub event_id: String,
    pub client_event_id: Option<String>,
    pub autopilot_id: String,
    pub run_id: String,
    pub step_id: Option<String>,
    pub event_type: String,
    pub metadata_json: String,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEvaluationInsert {
    pub run_id: String,
    pub autopilot_id: String,
    pub quality_score: i64,
    pub noise_score: i64,
    pub cost_score: i64,
    pub signals_json: String,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutopilotProfileUpsert {
    pub autopilot_id: String,
    pub learning_enabled: bool,
    pub mode: String,
    pub knobs_json: String,
    pub suppression_json: String,
    pub updated_at_ms: i64,
    pub version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptationLogInsert {
    pub id: String,
    pub autopilot_id: String,
    pub run_id: String,
    pub adaptation_hash: String,
    pub changes_json: String,
    pub rationale_codes_json: String,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCardUpsert {
    pub card_id: String,
    pub autopilot_id: String,
    pub card_type: String,
    pub title: String,
    pub content_json: String,
    pub confidence: i64,
    pub created_from_run_id: Option<String>,
    pub updated_at_ms: i64,
    pub version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidanceEventInsert {
    pub id: String,
    pub scope_type: String,
    pub scope_id: String,
    pub autopilot_id: Option<String>,
    pub run_id: Option<String>,
    pub approval_id: Option<String>,
    pub outcome_id: Option<String>,
    pub mode: String,
    pub instruction: String,
    pub result_json: String,
    pub created_at_ms: i64,
}

pub fn bootstrap_sqlite(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let app_data = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {e}"))?;
    fs::create_dir_all(&app_data).map_err(|e| format!("Failed to create app data dir: {e}"))?;

    let db_path = app_data.join("terminus.sqlite");
    let mut connection =
        Connection::open(&db_path).map_err(|e| format!("Failed to open sqlite db: {e}"))?;
    configure_connection(&connection)?;
    bootstrap_schema(&mut connection)?;
    Ok(db_path)
}

pub fn configure_connection(connection: &Connection) -> Result<(), String> {
    connection
        .busy_timeout(std::time::Duration::from_millis(5_000))
        .map_err(|e| format!("Failed to configure SQLite busy timeout: {e}"))?;
    connection
        .execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
        .map_err(|e| format!("Failed to configure SQLite pragmas: {e}"))?;
    Ok(())
}

pub fn bootstrap_schema(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            PRAGMA busy_timeout = 5000;

            CREATE TABLE IF NOT EXISTS schema_meta (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS autopilots (
              id TEXT PRIMARY KEY,
              name TEXT NOT NULL,
              created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS runs (
              id TEXT PRIMARY KEY,
              autopilot_id TEXT NOT NULL,
              idempotency_key TEXT NOT NULL UNIQUE,
              plan_json TEXT NOT NULL,
              provider_kind TEXT NOT NULL DEFAULT 'openai',
              provider_tier TEXT NOT NULL DEFAULT 'supported',
              state TEXT NOT NULL,
              current_step_index INTEGER NOT NULL DEFAULT 0,
              retry_count INTEGER NOT NULL DEFAULT 0,
              max_retries INTEGER NOT NULL DEFAULT 2,
              next_retry_backoff_ms INTEGER,
              next_retry_at_ms INTEGER,
              soft_cap_approved INTEGER NOT NULL DEFAULT 0,
              spend_usd_estimate REAL NOT NULL DEFAULT 0.0,
              spend_usd_actual REAL NOT NULL DEFAULT 0.0,
              usd_cents_estimate INTEGER NOT NULL DEFAULT 0,
              usd_cents_actual INTEGER NOT NULL DEFAULT 0,
              failure_reason TEXT,
              created_at INTEGER NOT NULL,
              updated_at INTEGER NOT NULL,
              FOREIGN KEY (autopilot_id) REFERENCES autopilots(id)
            );

            CREATE TABLE IF NOT EXISTS approvals (
              id TEXT PRIMARY KEY,
              run_id TEXT NOT NULL,
              step_id TEXT NOT NULL,
              action_id TEXT,
              status TEXT NOT NULL,
              preview TEXT NOT NULL,
              payload_type TEXT NOT NULL DEFAULT 'generic',
              payload_json TEXT NOT NULL DEFAULT '{}',
              reason TEXT,
              created_at INTEGER NOT NULL,
              updated_at INTEGER NOT NULL,
              decided_at INTEGER,
              UNIQUE (run_id, step_id),
              FOREIGN KEY (run_id) REFERENCES runs(id)
            );

            CREATE TABLE IF NOT EXISTS actions (
              id TEXT PRIMARY KEY,
              run_id TEXT NOT NULL,
              step_id TEXT NOT NULL,
              action_type TEXT NOT NULL,
              payload_json TEXT NOT NULL,
              requires_approval INTEGER NOT NULL,
              status TEXT NOT NULL,
              idempotency_key TEXT NOT NULL UNIQUE,
              created_at_ms INTEGER NOT NULL,
              updated_at_ms INTEGER NOT NULL,
              FOREIGN KEY (run_id) REFERENCES runs(id)
            );

            CREATE TABLE IF NOT EXISTS action_executions (
              id TEXT PRIMARY KEY,
              action_id TEXT NOT NULL,
              attempt INTEGER NOT NULL,
              executed_at_ms INTEGER NOT NULL,
              result_status TEXT NOT NULL,
              result_json TEXT NOT NULL,
              latency_ms INTEGER,
              retry_at_ms INTEGER,
              UNIQUE(action_id, attempt),
              FOREIGN KEY (action_id) REFERENCES actions(id)
            );

            CREATE TABLE IF NOT EXISTS clarifications (
              id TEXT PRIMARY KEY,
              run_id TEXT NOT NULL,
              step_id TEXT NOT NULL,
              field_key TEXT NOT NULL,
              question TEXT NOT NULL,
              options_json TEXT,
              answer_json TEXT,
              status TEXT NOT NULL,
              created_at_ms INTEGER NOT NULL,
              updated_at_ms INTEGER NOT NULL,
              FOREIGN KEY (run_id) REFERENCES runs(id)
            );

            CREATE TABLE IF NOT EXISTS provider_calls (
              id TEXT PRIMARY KEY,
              run_id TEXT,
              step_id TEXT,
              provider TEXT NOT NULL,
              model TEXT NOT NULL,
              request_kind TEXT NOT NULL,
              input_chars INTEGER,
              output_chars INTEGER,
              input_tokens_est INTEGER,
              output_tokens_est INTEGER,
              cache_hit INTEGER,
              latency_ms INTEGER,
              cost_cents_est INTEGER,
              created_at_ms INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS outcomes (
              id TEXT PRIMARY KEY,
              run_id TEXT NOT NULL,
              step_id TEXT NOT NULL,
              kind TEXT NOT NULL,
              status TEXT NOT NULL,
              content TEXT NOT NULL,
              failure_reason TEXT,
              created_at INTEGER NOT NULL,
              updated_at INTEGER NOT NULL,
              UNIQUE (run_id, step_id, kind),
              FOREIGN KEY (run_id) REFERENCES runs(id)
            );

            CREATE TABLE IF NOT EXISTS activities (
              id TEXT PRIMARY KEY,
              run_id TEXT NOT NULL,
              activity_type TEXT NOT NULL,
              from_state TEXT,
              to_state TEXT,
              user_message TEXT NOT NULL,
              created_at INTEGER NOT NULL,
              FOREIGN KEY (run_id) REFERENCES runs(id)
            );

            CREATE TABLE IF NOT EXISTS spend_ledger (
              id TEXT PRIMARY KEY,
              run_id TEXT NOT NULL,
              step_id TEXT NOT NULL DEFAULT '',
              entry_kind TEXT NOT NULL DEFAULT 'actual',
              amount_usd REAL NOT NULL,
              amount_usd_cents INTEGER NOT NULL DEFAULT 0,
              reason TEXT NOT NULL,
              day_bucket INTEGER NOT NULL,
              created_at INTEGER NOT NULL,
              FOREIGN KEY (run_id) REFERENCES runs(id)
            );

            CREATE TABLE IF NOT EXISTS web_snapshots (
              autopilot_id TEXT NOT NULL,
              url TEXT NOT NULL,
              last_hash TEXT NOT NULL,
              last_fetched_at_ms INTEGER NOT NULL,
              last_text_excerpt TEXT NOT NULL DEFAULT '',
              updated_at INTEGER NOT NULL,
              PRIMARY KEY (autopilot_id, url),
              FOREIGN KEY (autopilot_id) REFERENCES autopilots(id)
            );

            CREATE TABLE IF NOT EXISTS inbox_items (
              id TEXT PRIMARY KEY,
              autopilot_id TEXT NOT NULL,
              content_hash TEXT NOT NULL UNIQUE,
              raw_text TEXT NOT NULL,
              created_at_ms INTEGER NOT NULL,
              processed_at_ms INTEGER,
              FOREIGN KEY (autopilot_id) REFERENCES autopilots(id)
            );

            CREATE TABLE IF NOT EXISTS daily_brief_sources (
              autopilot_id TEXT PRIMARY KEY,
              sources_json TEXT NOT NULL,
              sources_hash TEXT NOT NULL,
              updated_at_ms INTEGER NOT NULL,
              FOREIGN KEY (autopilot_id) REFERENCES autopilots(id)
            );

            CREATE TABLE IF NOT EXISTS daily_brief_history (
              id TEXT PRIMARY KEY,
              autopilot_id TEXT NOT NULL,
              run_id TEXT NOT NULL,
              sources_hash TEXT NOT NULL,
              content_hash TEXT NOT NULL,
              summary_json TEXT NOT NULL,
              created_at_ms INTEGER NOT NULL,
              UNIQUE(autopilot_id, sources_hash, content_hash),
              FOREIGN KEY (autopilot_id) REFERENCES autopilots(id),
              FOREIGN KEY (run_id) REFERENCES runs(id)
            );

            CREATE TABLE IF NOT EXISTS decision_events (
              event_id TEXT PRIMARY KEY,
              client_event_id TEXT,
              autopilot_id TEXT NOT NULL,
              run_id TEXT NOT NULL,
              step_id TEXT,
              event_type TEXT NOT NULL,
              metadata_json TEXT NOT NULL DEFAULT '{}',
              created_at_ms INTEGER NOT NULL,
              FOREIGN KEY (autopilot_id) REFERENCES autopilots(id),
              FOREIGN KEY (run_id) REFERENCES runs(id)
            );

            CREATE TABLE IF NOT EXISTS run_evaluations (
              run_id TEXT PRIMARY KEY,
              autopilot_id TEXT NOT NULL,
              quality_score INTEGER NOT NULL,
              noise_score INTEGER NOT NULL,
              cost_score INTEGER NOT NULL,
              signals_json TEXT NOT NULL DEFAULT '{}',
              created_at_ms INTEGER NOT NULL,
              FOREIGN KEY (autopilot_id) REFERENCES autopilots(id),
              FOREIGN KEY (run_id) REFERENCES runs(id)
            );

            CREATE TABLE IF NOT EXISTS adaptation_log (
              id TEXT PRIMARY KEY,
              autopilot_id TEXT NOT NULL,
              run_id TEXT NOT NULL,
              adaptation_hash TEXT NOT NULL DEFAULT '',
              changes_json TEXT NOT NULL,
              rationale_codes_json TEXT NOT NULL,
              created_at_ms INTEGER NOT NULL,
              FOREIGN KEY (autopilot_id) REFERENCES autopilots(id),
              FOREIGN KEY (run_id) REFERENCES runs(id)
            );

            CREATE TABLE IF NOT EXISTS autopilot_profile (
              autopilot_id TEXT PRIMARY KEY,
              learning_enabled INTEGER NOT NULL DEFAULT 1,
              mode TEXT NOT NULL DEFAULT 'balanced',
              knobs_json TEXT NOT NULL DEFAULT '{}',
              suppression_json TEXT NOT NULL DEFAULT '{}',
              updated_at_ms INTEGER NOT NULL,
              version INTEGER NOT NULL DEFAULT 1,
              FOREIGN KEY (autopilot_id) REFERENCES autopilots(id)
            );

            CREATE TABLE IF NOT EXISTS memory_cards (
              card_id TEXT PRIMARY KEY,
              autopilot_id TEXT NOT NULL,
              card_type TEXT NOT NULL,
              title TEXT NOT NULL,
              content_json TEXT NOT NULL,
              confidence INTEGER NOT NULL DEFAULT 50,
              created_from_run_id TEXT,
              updated_at_ms INTEGER NOT NULL,
              version INTEGER NOT NULL DEFAULT 1,
              FOREIGN KEY (autopilot_id) REFERENCES autopilots(id),
              FOREIGN KEY (created_from_run_id) REFERENCES runs(id)
            );

            CREATE TABLE IF NOT EXISTS guidance_events (
              id TEXT PRIMARY KEY,
              scope_type TEXT NOT NULL,
              scope_id TEXT NOT NULL,
              autopilot_id TEXT,
              run_id TEXT,
              approval_id TEXT,
              outcome_id TEXT,
              mode TEXT NOT NULL,
              instruction TEXT NOT NULL,
              result_json TEXT NOT NULL DEFAULT '{}',
              created_at_ms INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS email_oauth_config (
              provider TEXT PRIMARY KEY,
              client_id TEXT NOT NULL,
              redirect_uri TEXT NOT NULL,
              updated_at_ms INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS email_oauth_sessions (
              provider TEXT NOT NULL,
              state TEXT NOT NULL,
              code_verifier TEXT NOT NULL,
              created_at_ms INTEGER NOT NULL,
              expires_at_ms INTEGER NOT NULL,
              PRIMARY KEY(provider, state)
            );

            CREATE TABLE IF NOT EXISTS email_connections (
              provider TEXT PRIMARY KEY,
              status TEXT NOT NULL,
              account_email TEXT,
              scopes_json TEXT NOT NULL DEFAULT '[]',
              connected_at_ms INTEGER,
              updated_at_ms INTEGER NOT NULL,
              last_error TEXT
            );

            CREATE TABLE IF NOT EXISTS email_ingest_events (
              id TEXT PRIMARY KEY,
              provider TEXT NOT NULL,
              provider_message_id TEXT NOT NULL,
              provider_thread_id TEXT,
              sender_email TEXT,
              dedupe_key TEXT NOT NULL UNIQUE,
              autopilot_id TEXT NOT NULL,
              subject TEXT NOT NULL DEFAULT '',
              received_at_ms INTEGER NOT NULL,
              run_id TEXT,
              status TEXT NOT NULL,
              created_at_ms INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS runner_control (
              singleton_id INTEGER PRIMARY KEY CHECK(singleton_id = 1),
              background_enabled INTEGER NOT NULL DEFAULT 0,
              watcher_enabled INTEGER NOT NULL DEFAULT 1,
              watcher_poll_seconds INTEGER NOT NULL DEFAULT 60,
              watcher_max_items INTEGER NOT NULL DEFAULT 10,
              gmail_autopilot_id TEXT NOT NULL DEFAULT 'auto_inbox_watch_gmail',
              microsoft_autopilot_id TEXT NOT NULL DEFAULT 'auto_inbox_watch_microsoft365',
              watcher_last_tick_ms INTEGER,
              missed_runs_count INTEGER NOT NULL DEFAULT 0,
              updated_at_ms INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS autopilot_send_policy (
              autopilot_id TEXT PRIMARY KEY,
              allow_sending INTEGER NOT NULL DEFAULT 0,
              recipient_allowlist_json TEXT NOT NULL DEFAULT '[]',
              max_sends_per_day INTEGER NOT NULL DEFAULT 10,
              quiet_hours_start_local INTEGER NOT NULL DEFAULT 18,
              quiet_hours_end_local INTEGER NOT NULL DEFAULT 9,
              allow_outside_quiet_hours INTEGER NOT NULL DEFAULT 0,
              updated_at_ms INTEGER NOT NULL,
              FOREIGN KEY (autopilot_id) REFERENCES autopilots(id)
            );

            -- Legacy compatibility from earlier bootstrap versions.
            CREATE TABLE IF NOT EXISTS activity (
              id TEXT PRIMARY KEY,
              autopilot_id TEXT,
              event TEXT,
              created_at INTEGER
            );
            ",
        )
        .map_err(|e| format!("Failed to bootstrap schema: {e}"))?;
    connection
        .execute(
            "INSERT INTO schema_meta (key, value) VALUES ('schema_version', '2026-02-22-hardening')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )
        .map_err(|e| format!("Failed to update schema version: {e}"))?;

    ensure_column(connection, "runs", "next_retry_at_ms", "INTEGER")?;
    ensure_column(
        connection,
        "runs",
        "provider_kind",
        "TEXT NOT NULL DEFAULT 'openai'",
    )?;
    ensure_column(
        connection,
        "runs",
        "provider_tier",
        "TEXT NOT NULL DEFAULT 'supported'",
    )?;
    ensure_column(
        connection,
        "runs",
        "soft_cap_approved",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        connection,
        "runs",
        "spend_usd_estimate",
        "REAL NOT NULL DEFAULT 0.0",
    )?;
    ensure_column(
        connection,
        "runs",
        "spend_usd_actual",
        "REAL NOT NULL DEFAULT 0.0",
    )?;
    ensure_column(
        connection,
        "runs",
        "usd_cents_estimate",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        connection,
        "runs",
        "usd_cents_actual",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        connection,
        "spend_ledger",
        "step_id",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        connection,
        "spend_ledger",
        "entry_kind",
        "TEXT NOT NULL DEFAULT 'actual'",
    )?;
    ensure_column(
        connection,
        "spend_ledger",
        "amount_usd_cents",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        connection,
        "web_snapshots",
        "last_text_excerpt",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(
        connection,
        "email_ingest_events",
        "provider_thread_id",
        "TEXT",
    )?;
    ensure_column(connection, "email_ingest_events", "sender_email", "TEXT")?;
    ensure_column(
        connection,
        "runner_control",
        "missed_runs_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        connection,
        "web_snapshots",
        "updated_at",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        connection,
        "approvals",
        "payload_type",
        "TEXT NOT NULL DEFAULT 'generic'",
    )?;
    ensure_column(
        connection,
        "approvals",
        "payload_json",
        "TEXT NOT NULL DEFAULT '{}'",
    )?;
    ensure_column(connection, "approvals", "action_id", "TEXT")?;
    ensure_column(connection, "decision_events", "client_event_id", "TEXT")?;
    ensure_column(
        connection,
        "adaptation_log",
        "adaptation_hash",
        "TEXT NOT NULL DEFAULT ''",
    )?;

    // Best-effort backfill from legacy float columns for existing vaults.
    connection
        .execute(
            "UPDATE runs
             SET usd_cents_actual = CAST(ROUND(spend_usd_actual * 100.0) AS INTEGER)
             WHERE usd_cents_actual = 0 AND spend_usd_actual > 0.0",
            [],
        )
        .map_err(|e| format!("Failed to backfill usd_cents_actual: {e}"))?;
    connection
        .execute(
            "UPDATE runs
             SET usd_cents_estimate = CAST(ROUND(spend_usd_estimate * 100.0) AS INTEGER)
             WHERE usd_cents_estimate = 0 AND spend_usd_estimate > 0.0",
            [],
        )
        .map_err(|e| format!("Failed to backfill usd_cents_estimate: {e}"))?;
    connection
        .execute(
            "UPDATE spend_ledger
             SET amount_usd_cents = CAST(ROUND(amount_usd * 100.0) AS INTEGER)
             WHERE amount_usd_cents = 0 AND amount_usd > 0.0",
            [],
        )
        .map_err(|e| format!("Failed to backfill spend_ledger cents: {e}"))?;

    // Replace legacy uniqueness (run_id, step_id) with (run_id, step_id, entry_kind).
    connection
        .execute("DROP INDEX IF EXISTS idx_spend_ledger_run_step", [])
        .map_err(|e| format!("Failed to drop legacy spend ledger index: {e}"))?;
    connection
        .execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_spend_ledger_run_step_kind ON spend_ledger(run_id, step_id, entry_kind)",
            [],
        )
        .map_err(|e| format!("Failed to create spend ledger unique index: {e}"))?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_decision_events_autopilot_created_at ON decision_events(autopilot_id, created_at_ms DESC)",
            [],
        )
        .map_err(|e| format!("Failed to create decision events index: {e}"))?;
    connection
        .execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_decision_events_client_event_id ON decision_events(client_event_id)",
            [],
        )
        .map_err(|e| format!("Failed to create decision client_event_id index: {e}"))?;
    connection
        .execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_adaptation_log_autopilot_run ON adaptation_log(autopilot_id, run_id)",
            [],
        )
        .map_err(|e| format!("Failed to create adaptation log index: {e}"))?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_adaptation_log_autopilot_hash_created ON adaptation_log(autopilot_id, adaptation_hash, created_at_ms DESC)",
            [],
        )
        .map_err(|e| format!("Failed to create adaptation hash index: {e}"))?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_cards_autopilot_type ON memory_cards(autopilot_id, card_type)",
            [],
        )
        .map_err(|e| format!("Failed to create memory card index: {e}"))?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_guidance_events_scope_created ON guidance_events(scope_type, scope_id, created_at_ms DESC)",
            [],
        )
        .map_err(|e| format!("Failed to create guidance events index: {e}"))?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_email_oauth_sessions_provider_expiry ON email_oauth_sessions(provider, expires_at_ms DESC)",
            [],
        )
        .map_err(|e| format!("Failed to create oauth sessions index: {e}"))?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_email_ingest_events_provider_created ON email_ingest_events(provider, created_at_ms DESC)",
            [],
        )
        .map_err(|e| format!("Failed to create email ingest events index: {e}"))?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_runs_state_updated ON runs(state, updated_at DESC)",
            [],
        )
        .map_err(|e| format!("Failed to create runs state index: {e}"))?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_runs_autopilot_updated ON runs(autopilot_id, updated_at DESC)",
            [],
        )
        .map_err(|e| format!("Failed to create runs autopilot index: {e}"))?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_actions_run_step ON actions(run_id, step_id)",
            [],
        )
        .map_err(|e| format!("Failed to create actions run-step index: {e}"))?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_actions_status_updated ON actions(status, updated_at_ms DESC)",
            [],
        )
        .map_err(|e| format!("Failed to create actions status index: {e}"))?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_action_executions_action_attempt ON action_executions(action_id, attempt DESC)",
            [],
        )
        .map_err(|e| format!("Failed to create action executions index: {e}"))?;
    connection
        .execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_clarifications_single_pending ON clarifications(run_id, step_id, status) WHERE status = 'pending'",
            [],
        )
        .map_err(|e| format!("Failed to create clarifications pending index: {e}"))?;
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_provider_calls_run_step ON provider_calls(run_id, step_id, created_at_ms DESC)",
            [],
        )
        .map_err(|e| format!("Failed to create provider calls index: {e}"))?;
    connection
        .execute(
            "INSERT OR IGNORE INTO runner_control (
               singleton_id, background_enabled, watcher_enabled, watcher_poll_seconds, watcher_max_items,
               gmail_autopilot_id, microsoft_autopilot_id, missed_runs_count, updated_at_ms
             ) VALUES (1, 0, 1, 60, 10, 'auto_inbox_watch_gmail', 'auto_inbox_watch_microsoft365', 0, strftime('%s','now') * 1000)",
            [],
        )
        .map_err(|e| format!("Failed to seed runner control: {e}"))?;

    Ok(())
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    column_type: &str,
) -> Result<(), String> {
    let mut stmt = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| format!("Failed to inspect table {table}: {e}"))?;
    let mut rows = stmt
        .query([])
        .map_err(|e| format!("Failed to query table info for {table}: {e}"))?;

    while let Some(row) = rows
        .next()
        .map_err(|e| format!("Failed reading table info for {table}: {e}"))?
    {
        let name: String = row
            .get(1)
            .map_err(|e| format!("Failed parsing table info for {table}: {e}"))?;
        if name == column {
            return Ok(());
        }
    }

    connection
        .execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {column_type}"),
            [],
        )
        .map_err(|e| format!("Failed adding column {column} to {table}: {e}"))?;
    Ok(())
}

pub fn insert_decision_event(
    connection: &Connection,
    payload: &DecisionEventInsert,
) -> Result<bool, String> {
    let changed = connection
        .execute(
            "
            INSERT INTO decision_events (
              event_id, client_event_id, autopilot_id, run_id, step_id, event_type, metadata_json, created_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT DO NOTHING
            ",
            params![
                &payload.event_id,
                &payload.client_event_id,
                &payload.autopilot_id,
                &payload.run_id,
                &payload.step_id,
                &payload.event_type,
                &payload.metadata_json,
                payload.created_at_ms
            ],
        )
        .map_err(|e| format!("Failed to insert decision event: {e}"))?;
    Ok(changed > 0)
}

pub fn insert_run_evaluation_if_missing(
    connection: &Connection,
    payload: &RunEvaluationInsert,
) -> Result<bool, String> {
    let changed = connection
        .execute(
            "
            INSERT INTO run_evaluations (
              run_id, autopilot_id, quality_score, noise_score, cost_score, signals_json, created_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(run_id) DO NOTHING
            ",
            params![
                &payload.run_id,
                &payload.autopilot_id,
                payload.quality_score,
                payload.noise_score,
                payload.cost_score,
                &payload.signals_json,
                payload.created_at_ms
            ],
        )
        .map_err(|e| format!("Failed to insert run evaluation: {e}"))?;
    Ok(changed > 0)
}

pub fn upsert_autopilot_profile(
    connection: &Connection,
    payload: &AutopilotProfileUpsert,
) -> Result<(), String> {
    connection
        .execute(
            "
            INSERT INTO autopilot_profile (
              autopilot_id, learning_enabled, mode, knobs_json, suppression_json, updated_at_ms, version
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(autopilot_id) DO UPDATE SET
              learning_enabled = excluded.learning_enabled,
              mode = excluded.mode,
              knobs_json = excluded.knobs_json,
              suppression_json = excluded.suppression_json,
              updated_at_ms = excluded.updated_at_ms,
              version = excluded.version
            ",
            params![
                &payload.autopilot_id,
                if payload.learning_enabled { 1 } else { 0 },
                &payload.mode,
                &payload.knobs_json,
                &payload.suppression_json,
                payload.updated_at_ms,
                payload.version
            ],
        )
        .map_err(|e| format!("Failed to upsert autopilot profile: {e}"))?;
    Ok(())
}

pub fn insert_adaptation_log(
    connection: &Connection,
    payload: &AdaptationLogInsert,
) -> Result<bool, String> {
    let changed = connection
        .execute(
            "
            INSERT INTO adaptation_log (
              id, autopilot_id, run_id, adaptation_hash, changes_json, rationale_codes_json, created_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(autopilot_id, run_id) DO NOTHING
            ",
            params![
                &payload.id,
                &payload.autopilot_id,
                &payload.run_id,
                &payload.adaptation_hash,
                &payload.changes_json,
                &payload.rationale_codes_json,
                payload.created_at_ms
            ],
        )
        .map_err(|e| format!("Failed to insert adaptation log: {e}"))?;
    Ok(changed > 0)
}

pub fn upsert_memory_card(
    connection: &Connection,
    payload: &MemoryCardUpsert,
) -> Result<(), String> {
    connection
        .execute(
            "
            INSERT INTO memory_cards (
              card_id, autopilot_id, card_type, title, content_json, confidence, created_from_run_id, updated_at_ms, version
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(card_id) DO UPDATE SET
              title = excluded.title,
              content_json = excluded.content_json,
              confidence = excluded.confidence,
              created_from_run_id = excluded.created_from_run_id,
              updated_at_ms = excluded.updated_at_ms,
              version = excluded.version
            ",
            params![
                &payload.card_id,
                &payload.autopilot_id,
                &payload.card_type,
                &payload.title,
                &payload.content_json,
                payload.confidence,
                &payload.created_from_run_id,
                payload.updated_at_ms,
                payload.version
            ],
        )
        .map_err(|e| format!("Failed to upsert memory card: {e}"))?;
    Ok(())
}

pub fn insert_guidance_event(
    connection: &Connection,
    payload: &GuidanceEventInsert,
) -> Result<(), String> {
    connection
        .execute(
            "
            INSERT INTO guidance_events (
              id, scope_type, scope_id, autopilot_id, run_id, approval_id, outcome_id,
              mode, instruction, result_json, created_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            ",
            params![
                &payload.id,
                &payload.scope_type,
                &payload.scope_id,
                &payload.autopilot_id,
                &payload.run_id,
                &payload.approval_id,
                &payload.outcome_id,
                &payload.mode,
                &payload.instruction,
                &payload.result_json,
                payload.created_at_ms
            ],
        )
        .map_err(|e| format!("Failed to insert guidance event: {e}"))?;
    Ok(())
}

pub fn get_home_snapshot(db_path: PathBuf) -> Result<HomeSnapshot, String> {
    let connection =
        Connection::open(db_path).map_err(|e| format!("Failed to open sqlite db: {e}"))?;
    configure_connection(&connection)?;

    let count = |table: &str| -> Result<i64, String> {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        connection
            .query_row(&sql, [], |row| row.get(0))
            .map_err(|e| format!("Failed to count {table}: {e}"))
    };

    let runner_control = get_runner_control(&connection)?;
    let now_ms = current_time_ms();
    let backlog_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM runs WHERE state IN ('ready', 'retrying', 'needs_approval', 'needs_clarification')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to count run backlog: {e}"))?;

    let base_line = if runner_control.watcher_enabled {
        if runner_control.background_enabled {
            "Autopilots run while your Mac is awake. Inbox watcher is active."
        } else {
            "Autopilots run while the app is open. Inbox watcher is active."
        }
    } else if runner_control.background_enabled {
        "Autopilots run while your Mac is awake. Inbox watcher is paused."
    } else {
        "Autopilots run only while the app is open. Inbox watcher is paused."
    };
    let status_line = if runner_control.missed_runs_count > 0 {
        format!(
            "{} {} runs were missed while your Mac was asleep/offline. Catch-up is in progress.",
            base_line, runner_control.missed_runs_count
        )
    } else {
        base_line.to_string()
    };
    let suppressed_autopilots_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM autopilot_profile
             WHERE learning_enabled = 1
               AND CAST(json_extract(suppression_json, '$.suppress_until_ms') AS INTEGER) > ?1",
            params![now_ms],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let mut suppressed_stmt = connection
        .prepare(
            "SELECT ap.autopilot_id,
                    COALESCE(a.name, ap.autopilot_id) AS name,
                    CAST(json_extract(ap.suppression_json, '$.suppress_until_ms') AS INTEGER) AS suppress_until_ms
             FROM autopilot_profile ap
             LEFT JOIN autopilots a ON a.id = ap.autopilot_id
             WHERE ap.learning_enabled = 1
               AND CAST(json_extract(ap.suppression_json, '$.suppress_until_ms') AS INTEGER) > ?1
             ORDER BY suppress_until_ms ASC
             LIMIT 5",
        )
        .map_err(|e| format!("Failed to prepare suppressed Autopilot query: {e}"))?;
    let suppressed_rows = suppressed_stmt
        .query_map(params![now_ms], |row| {
            Ok(SuppressedAutopilotNotice {
                autopilot_id: row.get(0)?,
                name: row.get(1)?,
                suppress_until_ms: row.get(2)?,
            })
        })
        .map_err(|e| format!("Failed to query suppressed Autopilots: {e}"))?;
    let mut suppressed_autopilots = Vec::new();
    for row in suppressed_rows {
        suppressed_autopilots
            .push(row.map_err(|e| format!("Failed to parse suppressed Autopilot row: {e}"))?);
    }
    let status_line = if suppressed_autopilots_count > 0 {
        format!(
            "{} {} Autopilot{} currently suppressed by learning rules.",
            status_line,
            suppressed_autopilots_count,
            if suppressed_autopilots_count == 1 {
                " is"
            } else {
                "s are"
            }
        )
    } else {
        status_line
    };

    let primary_outcome_count = count_primary_outcomes(&connection)?;

    Ok(HomeSnapshot {
        surfaces: vec![
            HomeSurface {
                title: "Autopilots".into(),
                subtitle: "Create repeatable follow-through".into(),
                count: count("autopilots")?,
                cta: "Create Autopilot".into(),
            },
            HomeSurface {
                title: "Outcomes".into(),
                subtitle: "Results from completed runs".into(),
                count: primary_outcome_count,
                cta: "View Outcomes".into(),
            },
            HomeSurface {
                title: "Approvals".into(),
                subtitle: "Actions waiting for your go-ahead".into(),
                count: count("approvals")?,
                cta: "Open Queue".into(),
            },
            HomeSurface {
                title: "Activity".into(),
                subtitle: "What happened and why".into(),
                count: count("activities")?,
                cta: "Open Activity".into(),
            },
        ],
        runner: RunnerStatus {
            mode: if runner_control.background_enabled {
                "background".into()
            } else {
                "app_open".into()
            },
            status_line,
            backlog_count,
            watcher_enabled: runner_control.watcher_enabled,
            watcher_last_tick_ms: runner_control.watcher_last_tick_ms,
            missed_runs_count: runner_control.missed_runs_count,
            suppressed_autopilots_count,
            suppressed_autopilots,
        },
    })
}

fn current_time_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub fn count_primary_outcomes(connection: &Connection) -> Result<i64, String> {
    connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM runs r
            WHERE r.state IN ('succeeded', 'failed', 'canceled')
               OR r.state = 'needs_approval'
               OR r.state = 'needs_clarification'
               OR (
                    r.state = 'blocked'
                    AND EXISTS (
                      SELECT 1 FROM clarifications c
                      WHERE c.run_id = r.id AND c.status = 'pending'
                    )
                  )
            ",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to count primary outcomes: {e}"))
}

pub fn list_primary_outcomes(
    connection: &Connection,
    limit: usize,
) -> Result<Vec<PrimaryOutcomeRecord>, String> {
    let mut stmt = connection
        .prepare(
            "
            SELECT
              r.id,
              r.autopilot_id,
              r.state,
              r.failure_reason,
              r.created_at,
              r.updated_at,
              (
                SELECT a.preview FROM approvals a
                WHERE a.run_id = r.id AND a.status = 'pending'
                ORDER BY a.created_at ASC LIMIT 1
              ) AS pending_approval_preview,
              (
                SELECT c.question FROM clarifications c
                WHERE c.run_id = r.id AND c.status = 'pending'
                ORDER BY c.created_at_ms ASC LIMIT 1
              ) AS pending_clarification_question,
              (
                SELECT o.content FROM outcomes o
                WHERE o.run_id = r.id AND o.kind = 'receipt'
                ORDER BY o.updated_at DESC LIMIT 1
              ) AS receipt_content
            FROM runs r
            WHERE r.state IN ('succeeded', 'failed', 'canceled', 'needs_approval', 'needs_clarification', 'blocked')
            ORDER BY r.updated_at DESC
            LIMIT ?1
            ",
        )
        .map_err(|e| format!("Failed to prepare primary outcomes query: {e}"))?;
    let rows = stmt
        .query_map(params![limit as i64], |row| {
            let state: String = row.get(2)?;
            let failure_reason: Option<String> = row.get(3)?;
            let pending_preview: Option<String> = row.get(6)?;
            let clarification_q: Option<String> = row.get(7)?;
            let receipt_content: Option<String> = row.get(8)?;
            let (status, summary) = match state.as_str() {
                "needs_approval" => (
                    "pending_approval".to_string(),
                    pending_preview.unwrap_or_else(|| "Action waiting for approval.".to_string()),
                ),
                "needs_clarification" => (
                    "blocked_clarification".to_string(),
                    clarification_q
                        .unwrap_or_else(|| "One thing is needed to proceed.".to_string()),
                ),
                "blocked" if clarification_q.is_some() => (
                    "blocked_clarification".to_string(),
                    clarification_q
                        .unwrap_or_else(|| "One thing is needed to proceed.".to_string()),
                ),
                "blocked" => (
                    "blocked".to_string(),
                    failure_reason
                        .clone()
                        .unwrap_or_else(|| "Run was blocked.".to_string()),
                ),
                "succeeded" | "failed" | "canceled" => {
                    let summary = receipt_content
                        .as_deref()
                        .and_then(|payload| {
                            serde_json::from_str::<serde_json::Value>(payload)
                                .ok()
                                .and_then(|v| {
                                    v.get("summary")
                                        .and_then(|s| s.as_str())
                                        .map(ToString::to_string)
                                })
                        })
                        .or(failure_reason.clone())
                        .unwrap_or_else(|| "Run completed.".to_string());
                    ("executed".to_string(), summary)
                }
                _ => ("executed".to_string(), "Run completed.".to_string()),
            };
            Ok(PrimaryOutcomeRecord {
                run_id: row.get(0)?,
                autopilot_id: row.get(1)?,
                status,
                summary,
                created_at_ms: row.get(4)?,
                updated_at_ms: row.get(5)?,
            })
        })
        .map_err(|e| format!("Failed to query primary outcomes: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("Failed to parse primary outcomes row: {e}"))?);
    }
    Ok(out)
}

pub fn get_runner_control(connection: &Connection) -> Result<RunnerControlRecord, String> {
    connection
        .query_row(
            "SELECT background_enabled, watcher_enabled, watcher_poll_seconds, watcher_max_items, gmail_autopilot_id, microsoft_autopilot_id, watcher_last_tick_ms, missed_runs_count
             FROM runner_control WHERE singleton_id = 1",
            [],
            |row| {
                Ok(RunnerControlRecord {
                    background_enabled: row.get::<_, i64>(0)? == 1,
                    watcher_enabled: row.get::<_, i64>(1)? == 1,
                    watcher_poll_seconds: row.get(2)?,
                    watcher_max_items: row.get(3)?,
                    gmail_autopilot_id: row.get(4)?,
                    microsoft_autopilot_id: row.get(5)?,
                    watcher_last_tick_ms: row.get(6)?,
                    missed_runs_count: row.get(7)?,
                })
            },
        )
        .map_err(|e| format!("Failed to read runner control: {e}"))
}

pub fn upsert_runner_control(
    connection: &Connection,
    payload: &RunnerControlRecord,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE runner_control
             SET background_enabled = ?1,
                 watcher_enabled = ?2,
                 watcher_poll_seconds = ?3,
                 watcher_max_items = ?4,
                 gmail_autopilot_id = ?5,
                 microsoft_autopilot_id = ?6,
                 watcher_last_tick_ms = ?7,
                 missed_runs_count = ?8,
                 updated_at_ms = strftime('%s','now') * 1000
             WHERE singleton_id = 1",
            params![
                if payload.background_enabled { 1 } else { 0 },
                if payload.watcher_enabled { 1 } else { 0 },
                payload.watcher_poll_seconds,
                payload.watcher_max_items,
                payload.gmail_autopilot_id,
                payload.microsoft_autopilot_id,
                payload.watcher_last_tick_ms,
                payload.missed_runs_count
            ],
        )
        .map_err(|e| format!("Failed to update runner control: {e}"))?;
    Ok(())
}

pub fn get_autopilot_send_policy(
    connection: &Connection,
    autopilot_id: &str,
) -> Result<AutopilotSendPolicyRecord, String> {
    let row: Option<(i64, String, i64, i64, i64, i64, i64)> = connection
        .query_row(
            "SELECT allow_sending, recipient_allowlist_json, max_sends_per_day,
                    quiet_hours_start_local, quiet_hours_end_local, allow_outside_quiet_hours, updated_at_ms
             FROM autopilot_send_policy
             WHERE autopilot_id = ?1",
            params![autopilot_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()
        .map_err(|e| format!("Failed to read send policy: {e}"))?;

    let Some((
        allow_sending,
        allowlist_json,
        max_sends_per_day,
        start,
        end,
        allow_outside,
        updated_at_ms,
    )) = row
    else {
        return Ok(AutopilotSendPolicyRecord {
            autopilot_id: autopilot_id.to_string(),
            allow_sending: false,
            recipient_allowlist: Vec::new(),
            max_sends_per_day: 10,
            quiet_hours_start_local: 18,
            quiet_hours_end_local: 9,
            allow_outside_quiet_hours: false,
            updated_at_ms: 0,
        });
    };

    let recipient_allowlist =
        serde_json::from_str::<Vec<String>>(&allowlist_json).unwrap_or_default();
    Ok(AutopilotSendPolicyRecord {
        autopilot_id: autopilot_id.to_string(),
        allow_sending: allow_sending == 1,
        recipient_allowlist,
        max_sends_per_day,
        quiet_hours_start_local: start,
        quiet_hours_end_local: end,
        allow_outside_quiet_hours: allow_outside == 1,
        updated_at_ms,
    })
}

pub fn upsert_autopilot_send_policy(
    connection: &Connection,
    payload: &AutopilotSendPolicyRecord,
) -> Result<(), String> {
    let allowlist_json = serde_json::to_string(&payload.recipient_allowlist)
        .map_err(|e| format!("Failed to serialize recipient allowlist: {e}"))?;
    connection
        .execute(
            "INSERT INTO autopilot_send_policy (
               autopilot_id, allow_sending, recipient_allowlist_json, max_sends_per_day,
               quiet_hours_start_local, quiet_hours_end_local, allow_outside_quiet_hours, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(autopilot_id) DO UPDATE SET
               allow_sending = excluded.allow_sending,
               recipient_allowlist_json = excluded.recipient_allowlist_json,
               max_sends_per_day = excluded.max_sends_per_day,
               quiet_hours_start_local = excluded.quiet_hours_start_local,
               quiet_hours_end_local = excluded.quiet_hours_end_local,
               allow_outside_quiet_hours = excluded.allow_outside_quiet_hours,
               updated_at_ms = excluded.updated_at_ms",
            params![
                payload.autopilot_id,
                if payload.allow_sending { 1 } else { 0 },
                allowlist_json,
                payload.max_sends_per_day,
                payload.quiet_hours_start_local,
                payload.quiet_hours_end_local,
                if payload.allow_outside_quiet_hours { 1 } else { 0 },
                payload.updated_at_ms,
            ],
        )
        .map_err(|e| format!("Failed to upsert send policy: {e}"))?;
    Ok(())
}
