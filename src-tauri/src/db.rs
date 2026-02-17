use rusqlite::Connection;
use serde::Serialize;
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
}

#[derive(Debug, Clone, Serialize)]
pub struct HomeSnapshot {
    pub surfaces: Vec<HomeSurface>,
    pub runner: RunnerStatus,
}

pub fn bootstrap_sqlite(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let app_data = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {e}"))?;
    fs::create_dir_all(&app_data).map_err(|e| format!("Failed to create app data dir: {e}"))?;

    let db_path = app_data.join("terminus.sqlite");
    let mut connection = Connection::open(&db_path).map_err(|e| format!("Failed to open sqlite db: {e}"))?;
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
    ensure_column(connection, "runs", "provider_kind", "TEXT NOT NULL DEFAULT 'openai'")?;
    ensure_column(connection, "runs", "provider_tier", "TEXT NOT NULL DEFAULT 'supported'")?;
    ensure_column(connection, "runs", "soft_cap_approved", "INTEGER NOT NULL DEFAULT 0")?;
    ensure_column(connection, "runs", "spend_usd_estimate", "REAL NOT NULL DEFAULT 0.0")?;
    ensure_column(connection, "runs", "spend_usd_actual", "REAL NOT NULL DEFAULT 0.0")?;
    ensure_column(connection, "runs", "usd_cents_estimate", "INTEGER NOT NULL DEFAULT 0")?;
    ensure_column(connection, "runs", "usd_cents_actual", "INTEGER NOT NULL DEFAULT 0")?;
    ensure_column(connection, "spend_ledger", "step_id", "TEXT NOT NULL DEFAULT ''")?;
    ensure_column(connection, "spend_ledger", "entry_kind", "TEXT NOT NULL DEFAULT 'actual'")?;
    ensure_column(connection, "spend_ledger", "amount_usd_cents", "INTEGER NOT NULL DEFAULT 0")?;
    ensure_column(connection, "web_snapshots", "last_text_excerpt", "TEXT NOT NULL DEFAULT ''")?;
    ensure_column(connection, "web_snapshots", "updated_at", "INTEGER NOT NULL DEFAULT 0")?;

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

pub fn get_home_snapshot(db_path: PathBuf) -> Result<HomeSnapshot, String> {
    let connection = Connection::open(db_path).map_err(|e| format!("Failed to open sqlite db: {e}"))?;

    let count = |table: &str| -> Result<i64, String> {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        connection
            .query_row(&sql, [], |row| row.get(0))
            .map_err(|e| format!("Failed to count {table}: {e}"))
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
            mode: "app_open".into(),
            status_line: "Autopilots run only while the app is open.".into(),
        },
    })
}
