use rusqlite::{params, Connection};
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
}

#[derive(Debug, Clone, Serialize)]
pub struct HomeSnapshot {
    pub surfaces: Vec<HomeSurface>,
    pub runner: RunnerStatus,
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

pub fn bootstrap_sqlite(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let app_data = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {e}"))?;
    fs::create_dir_all(&app_data).map_err(|e| format!("Failed to create app data dir: {e}"))?;

    let db_path = app_data.join("terminus.sqlite");
    let mut connection =
        Connection::open(&db_path).map_err(|e| format!("Failed to open sqlite db: {e}"))?;
    bootstrap_schema(&mut connection)?;
    Ok(db_path)
}

pub fn bootstrap_schema(connection: &mut Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "
            PRAGMA foreign_keys = ON;

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
              status TEXT NOT NULL,
              preview TEXT NOT NULL,
              reason TEXT,
              created_at INTEGER NOT NULL,
              updated_at INTEGER NOT NULL,
              decided_at INTEGER,
              UNIQUE (run_id, step_id),
              FOREIGN KEY (run_id) REFERENCES runs(id)
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
              updated_at_ms INTEGER NOT NULL
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
        "web_snapshots",
        "updated_at",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
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
            "INSERT OR IGNORE INTO runner_control (
               singleton_id, background_enabled, watcher_enabled, watcher_poll_seconds, watcher_max_items,
               gmail_autopilot_id, microsoft_autopilot_id, updated_at_ms
             ) VALUES (1, 0, 1, 60, 10, 'auto_inbox_watch_gmail', 'auto_inbox_watch_microsoft365', strftime('%s','now') * 1000)",
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

pub fn get_home_snapshot(db_path: PathBuf) -> Result<HomeSnapshot, String> {
    let connection =
        Connection::open(db_path).map_err(|e| format!("Failed to open sqlite db: {e}"))?;

    let count = |table: &str| -> Result<i64, String> {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        connection
            .query_row(&sql, [], |row| row.get(0))
            .map_err(|e| format!("Failed to count {table}: {e}"))
    };

    let runner_control = get_runner_control(&connection)?;
    let backlog_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM runs WHERE state IN ('ready', 'retrying', 'needs_approval')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to count run backlog: {e}"))?;

    let status_line = if runner_control.watcher_enabled {
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
                count: count("outcomes")?,
                cta: "View Outcomes".into(),
            },
            HomeSurface {
                title: "Approvals".into(),
                subtitle: "Drafts waiting for your go-ahead".into(),
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
            status_line: status_line.to_string(),
            backlog_count,
            watcher_enabled: runner_control.watcher_enabled,
            watcher_last_tick_ms: runner_control.watcher_last_tick_ms,
        },
    })
}

pub fn get_runner_control(connection: &Connection) -> Result<RunnerControlRecord, String> {
    connection
        .query_row(
            "SELECT background_enabled, watcher_enabled, watcher_poll_seconds, watcher_max_items, gmail_autopilot_id, microsoft_autopilot_id, watcher_last_tick_ms
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
                 updated_at_ms = strftime('%s','now') * 1000
             WHERE singleton_id = 1",
            params![
                if payload.background_enabled { 1 } else { 0 },
                if payload.watcher_enabled { 1 } else { 0 },
                payload.watcher_poll_seconds,
                payload.watcher_max_items,
                payload.gmail_autopilot_id,
                payload.microsoft_autopilot_id,
                payload.watcher_last_tick_ms
            ],
        )
        .map_err(|e| format!("Failed to update runner control: {e}"))?;
    Ok(())
}
