use crate::learning::{
    self, AdaptationSummary, DecisionEventMetadata, DecisionEventType, RunEvaluationSummary,
    RuntimeProfile,
};
use crate::primitives::PrimitiveGuard;
use crate::providers::{
    ProviderError, ProviderKind, ProviderRequest, ProviderResponse, ProviderRuntime, ProviderTier,
};
use crate::schema::{
    AutopilotPlan, PlanStep, PrimitiveId, ProviderId as SchemaProviderId,
    ProviderTier as SchemaProviderTier, RecipeKind,
};
use crate::web::{fetch_allowlisted_text, parse_scheme_host, WebFetchError, WebFetchResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

const PER_RUN_SOFT_CAP_USD_CENTS: i64 = 40;
const PER_RUN_HARD_CAP_USD_CENTS: i64 = 80;
const DAILY_SOFT_CAP_USD_CENTS: i64 = 300;
const DAILY_HARD_CAP_USD_CENTS: i64 = 500;
const SOFT_CAP_APPROVAL_STEP_ID: &str = "__soft_cap__";
const INBOX_TEXT_MAX_CHARS: usize = 20_000;
const DAILY_SOURCE_MAX_ITEMS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Draft,
    Ready,
    Running,
    NeedsApproval,
    Retrying,
    Succeeded,
    Failed,
    Blocked,
    Canceled,
}

impl RunState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Ready => "ready",
            Self::Running => "running",
            Self::NeedsApproval => "needs_approval",
            Self::Retrying => "retrying",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
            Self::Canceled => "canceled",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Blocked | Self::Canceled
        )
    }
}

impl FromStr for RunState {
    type Err = RunnerError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "draft" => Ok(Self::Draft),
            "ready" => Ok(Self::Ready),
            "running" => Ok(Self::Running),
            "needs_approval" => Ok(Self::NeedsApproval),
            "retrying" => Ok(Self::Retrying),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "blocked" => Ok(Self::Blocked),
            "canceled" => Ok(Self::Canceled),
            _ => Err(RunnerError::InvalidState(value.to_string())),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub autopilot_id: String,
    pub idempotency_key: String,
    pub provider_kind: ProviderKind,
    pub provider_tier: ProviderTier,
    pub state: RunState,
    pub current_step_index: i64,
    pub retry_count: i64,
    pub max_retries: i64,
    pub next_retry_backoff_ms: Option<i64>,
    pub next_retry_at_ms: Option<i64>,
    pub soft_cap_approved: bool,
    pub usd_cents_estimate: i64,
    pub usd_cents_actual: i64,
    pub failure_reason: Option<String>,
    pub plan: AutopilotPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub id: String,
    pub run_id: String,
    pub step_id: String,
    pub status: String,
    pub preview: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReceipt {
    pub schema_version: String,
    pub run_id: String,
    pub autopilot_id: String,
    pub provider_kind: String,
    pub provider_tier: String,
    pub terminal_state: String,
    pub summary: String,
    pub failure_reason: Option<String>,
    pub recovery_options: Vec<String>,
    pub total_spend_usd_cents: i64,
    pub cost_breakdown: Vec<ReceiptCostLineItem>,
    pub evaluation: Option<RunEvaluationSummary>,
    pub adaptation: Option<AdaptationSummary>,
    #[serde(default)]
    pub memory_titles_used: Vec<String>,
    pub redacted: bool,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptCostLineItem {
    pub step_id: String,
    pub entry_kind: String,
    pub amount_usd_cents: i64,
}

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("database error: {0}")]
    Db(String),
    #[error("serialization error: {0}")]
    Serde(String),
    #[error("run not found")]
    RunNotFound,
    #[error("approval not found")]
    ApprovalNotFound,
    #[error("invalid run state: {0}")]
    InvalidState(String),
    #[error("invalid provider kind: {0}")]
    InvalidProviderKind(String),
    #[error("invalid provider tier: {0}")]
    InvalidProviderTier(String),
    #[error("{0}")]
    Human(String),
    #[error("forced transition failure")]
    ForcedTransitionFailure,
}

#[derive(Debug)]
struct StepExecutionError {
    retryable: bool,
    user_reason: String,
}

#[derive(Debug)]
struct StepExecutionResult {
    user_message: String,
    actual_spend_usd_cents: i64,
    next_step_index_override: Option<i64>,
    terminal_state_override: Option<RunState>,
    terminal_summary_override: Option<String>,
    failure_reason_override: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WebReadArtifact {
    url: String,
    fetched_at_ms: i64,
    status_code: u16,
    content_hash: String,
    changed: bool,
    #[serde(default)]
    diff_score: f64,
    current_excerpt: String,
    previous_excerpt: Option<String>,
}

#[derive(Debug, Clone)]
struct WebSnapshotRecord {
    last_hash: String,
    last_text_excerpt: String,
}

#[derive(Debug, Clone)]
struct InboxItemRecord {
    id: String,
    content_hash: String,
    raw_text: String,
    processed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InboxReadArtifact {
    item_id: String,
    content_hash: String,
    text_excerpt: String,
    created_at_ms: i64,
    deduped_existing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DailySourceResult {
    source_id: String,
    url: String,
    text_excerpt: String,
    fetched_at_ms: i64,
    fetch_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DailySourcesArtifact {
    sources_hash: String,
    source_results: Vec<DailySourceResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DailySummaryArtifact {
    title: String,
    bullet_points: Vec<String>,
    summary_text: String,
    sources_hash: String,
    content_hash: String,
}

enum CapDecision {
    Allow,
    NeedsSoftApproval { message: String },
    BlockHard { message: String },
}

pub struct RunnerEngine;

impl RunnerEngine {
    pub fn start_run(
        connection: &mut Connection,
        autopilot_id: &str,
        plan: AutopilotPlan,
        idempotency_key: &str,
        max_retries: i64,
    ) -> Result<RunRecord, RunnerError> {
        if let Some(existing) = Self::get_run_by_idempotency_key(connection, idempotency_key)? {
            return Ok(existing);
        }

        let run_id = make_id("run");
        let now = now_ms();
        let plan_json =
            serde_json::to_string(&plan).map_err(|e| RunnerError::Serde(e.to_string()))?;
        let provider_kind = provider_kind_from_plan(&plan);
        let provider_tier = provider_tier_from_plan(&plan);

        let tx = connection
            .transaction()
            .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.execute(
            "INSERT OR IGNORE INTO autopilots (id, name, created_at) VALUES (?1, ?2, ?3)",
            params![autopilot_id, "Autopilot", now],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.execute(
            "
            INSERT INTO runs (
              id, autopilot_id, idempotency_key, plan_json,
              provider_kind, provider_tier,
              state, current_step_index, retry_count, max_retries,
              next_retry_backoff_ms, next_retry_at_ms,
              soft_cap_approved, spend_usd_estimate, spend_usd_actual,
              usd_cents_estimate, usd_cents_actual,
              failure_reason, created_at, updated_at
            ) VALUES (
              ?1, ?2, ?3, ?4,
              ?5, ?6,
              ?7, 0, 0, ?8,
              NULL, NULL,
              0, 0.0, 0.0,
              0, 0,
              NULL, ?9, ?9
            )
            ",
            params![
                run_id,
                autopilot_id,
                idempotency_key,
                plan_json,
                provider_kind.as_str(),
                provider_tier.as_str(),
                RunState::Ready.as_str(),
                max_retries,
                now
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.execute(
            "
            INSERT INTO activities (
              id, run_id, activity_type, from_state, to_state, user_message, created_at
            ) VALUES (?1, ?2, 'run_created', NULL, ?3, ?4, ?5)
            ",
            params![
                make_id("activity"),
                run_id,
                RunState::Ready.as_str(),
                "Run was created and is ready.",
                now
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.commit().map_err(|e| RunnerError::Db(e.to_string()))?;
        learning::ensure_autopilot_profile(connection, autopilot_id)
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        Self::get_run(connection, &run_id)
    }

    pub fn run_tick(connection: &mut Connection, run_id: &str) -> Result<RunRecord, RunnerError> {
        Self::run_tick_internal(connection, run_id, None)
    }

    pub fn resume_due_runs(
        connection: &mut Connection,
        limit: usize,
    ) -> Result<Vec<RunRecord>, RunnerError> {
        let now = now_ms();
        let run_ids = {
            let mut stmt = connection
                .prepare(
                    "
                    SELECT id FROM runs
                    WHERE state = 'retrying'
                      AND next_retry_at_ms IS NOT NULL
                      AND next_retry_at_ms <= ?1
                    ORDER BY next_retry_at_ms ASC
                    LIMIT ?2
                    ",
                )
                .map_err(|e| RunnerError::Db(e.to_string()))?;

            let rows = stmt
                .query_map(params![now, limit as i64], |row| row.get::<_, String>(0))
                .map_err(|e| RunnerError::Db(e.to_string()))?;

            let mut collected = Vec::new();
            for row in rows {
                collected.push(row.map_err(|e| RunnerError::Db(e.to_string()))?);
            }
            collected
        };

        let mut updated = Vec::new();
        for run_id in run_ids {
            updated.push(Self::run_tick(connection, &run_id)?);
        }
        Ok(updated)
    }

    pub fn approve(
        connection: &mut Connection,
        approval_id: &str,
    ) -> Result<RunRecord, RunnerError> {
        let approval = Self::get_approval(connection, approval_id)?;
        if approval.status != "pending" {
            return Err(RunnerError::Human(
                "Approval is no longer pending.".to_string(),
            ));
        }
        let decision_now = now_ms();
        let latency_ms = Self::get_approval_created_at(connection, approval_id)?
            .map(|created_at| decision_now.saturating_sub(created_at));

        let tx = connection
            .transaction()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let now = decision_now;

        tx.execute(
            "
            UPDATE approvals
            SET status = 'approved', updated_at = ?1, decided_at = ?1
            WHERE id = ?2
            ",
            params![now, approval_id],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        let is_soft_cap_approval = approval.step_id == SOFT_CAP_APPROVAL_STEP_ID;

        tx.execute(
            "
            UPDATE runs
            SET state = 'ready',
                soft_cap_approved = CASE WHEN ?1 THEN 1 ELSE soft_cap_approved END,
                failure_reason = NULL,
                next_retry_backoff_ms = NULL,
                next_retry_at_ms = NULL,
                updated_at = ?2
            WHERE id = ?3
            ",
            params![is_soft_cap_approval, now, approval.run_id],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.execute(
            "
            INSERT INTO activities (id, run_id, activity_type, from_state, to_state, user_message, created_at)
            VALUES (?1, ?2, 'approval_approved', 'needs_approval', 'ready', ?3, ?4)
            ",
            params![
                make_id("activity"),
                approval.run_id,
                if is_soft_cap_approval {
                    "Spend approval granted. Run is ready for next step."
                } else {
                    "Step approval granted. Run is ready for next step."
                },
                now
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.commit().map_err(|e| RunnerError::Db(e.to_string()))?;
        let run_after_approval = Self::get_run(connection, &approval.run_id)?;
        learning::record_decision_event(
            connection,
            &run_after_approval.autopilot_id,
            &approval.run_id,
            Some(&approval.step_id),
            DecisionEventType::ApprovalApproved,
            DecisionEventMetadata {
                latency_ms,
                reason_code: Some(if is_soft_cap_approval {
                    "soft_cap".to_string()
                } else {
                    "step".to_string()
                }),
                provider_kind: Some(run_after_approval.provider_kind.as_str().to_string()),
                usd_cents_actual: Some(run_after_approval.usd_cents_actual),
                ..Default::default()
            },
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        if is_soft_cap_approval {
            Self::run_tick_internal(connection, &approval.run_id, None)
        } else {
            Self::run_tick_internal(connection, &approval.run_id, Some(&approval.step_id))
        }
    }

    pub fn reject(
        connection: &mut Connection,
        approval_id: &str,
        reason: Option<String>,
    ) -> Result<RunRecord, RunnerError> {
        let approval = Self::get_approval(connection, approval_id)?;
        if approval.status != "pending" {
            return Err(RunnerError::Human(
                "Approval is no longer pending.".to_string(),
            ));
        }
        let decision_now = now_ms();
        let latency_ms = Self::get_approval_created_at(connection, approval_id)?
            .map(|created_at| decision_now.saturating_sub(created_at));

        let reject_reason =
            reason.unwrap_or_else(|| "Approval was rejected by the user.".to_string());
        let terminal_state = if approval.step_id == SOFT_CAP_APPROVAL_STEP_ID {
            RunState::Blocked
        } else {
            RunState::Canceled
        };

        let tx = connection
            .transaction()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let now = decision_now;

        tx.execute(
            "
            UPDATE approvals
            SET status = 'rejected', reason = ?1, updated_at = ?2, decided_at = ?2
            WHERE id = ?3
            ",
            params![reject_reason, now, approval_id],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.execute(
            "
            UPDATE runs
            SET state = ?1,
                failure_reason = ?2,
                updated_at = ?3
            WHERE id = ?4
            ",
            params![terminal_state.as_str(), reject_reason, now, approval.run_id],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.execute(
            "
            INSERT INTO activities (id, run_id, activity_type, from_state, to_state, user_message, created_at)
            VALUES (?1, ?2, 'approval_rejected', 'needs_approval', ?3, ?4, ?5)
            ",
            params![
                make_id("activity"),
                approval.run_id,
                terminal_state.as_str(),
                redact_text(&reject_reason),
                now
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        let run = Self::get_run_in_tx(&tx, &approval.run_id)?;
        Self::upsert_terminal_receipt_in_tx(
            &tx,
            &run,
            terminal_state,
            "Run stopped after approval rejection.",
            Some(&reject_reason),
        )?;

        tx.commit().map_err(|e| RunnerError::Db(e.to_string()))?;
        let run_after_reject = Self::get_run(connection, &approval.run_id)?;
        learning::record_decision_event(
            connection,
            &run_after_reject.autopilot_id,
            &approval.run_id,
            Some(&approval.step_id),
            DecisionEventType::ApprovalRejected,
            DecisionEventMetadata {
                latency_ms,
                reason_code: Some(if approval.step_id == SOFT_CAP_APPROVAL_STEP_ID {
                    "soft_cap_rejected".to_string()
                } else {
                    "user_rejected".to_string()
                }),
                provider_kind: Some(run_after_reject.provider_kind.as_str().to_string()),
                usd_cents_actual: Some(run_after_reject.usd_cents_actual),
                ..Default::default()
            },
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;
        Self::get_run_with_learning(connection, &approval.run_id)
    }

    pub fn list_pending_approvals(
        connection: &Connection,
    ) -> Result<Vec<ApprovalRecord>, RunnerError> {
        let mut stmt = connection
            .prepare(
                "
                SELECT id, run_id, step_id, status, preview, reason
                FROM approvals
                WHERE status = 'pending'
                ORDER BY created_at ASC
                ",
            )
            .map_err(|e| RunnerError::Db(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(ApprovalRecord {
                    id: row.get(0)?,
                    run_id: row.get(1)?,
                    step_id: row.get(2)?,
                    status: row.get(3)?,
                    preview: row.get(4)?,
                    reason: row.get(5)?,
                })
            })
            .map_err(|e| RunnerError::Db(e.to_string()))?;

        let mut approvals = Vec::new();
        for row in rows {
            approvals.push(row.map_err(|e| RunnerError::Db(e.to_string()))?);
        }
        Ok(approvals)
    }

    pub fn get_run(connection: &Connection, run_id: &str) -> Result<RunRecord, RunnerError> {
        connection
            .query_row(
                "
                SELECT id, autopilot_id, idempotency_key,
                       provider_kind, provider_tier,
                       state, current_step_index, retry_count, max_retries,
                       next_retry_backoff_ms, next_retry_at_ms,
                       soft_cap_approved, usd_cents_estimate, usd_cents_actual,
                       failure_reason, plan_json
                FROM runs
                WHERE id = ?1
                ",
                params![run_id],
                |row| {
                    let state_text: String = row.get(5)?;
                    let provider_kind_text: String = row.get(3)?;
                    let provider_tier_text: String = row.get(4)?;
                    let plan_json: String = row.get(15)?;
                    let plan: AutopilotPlan = serde_json::from_str(&plan_json)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    Ok(RunRecord {
                        id: row.get(0)?,
                        autopilot_id: row.get(1)?,
                        idempotency_key: row.get(2)?,
                        provider_kind: parse_provider_kind(&provider_kind_text)
                            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                        provider_tier: parse_provider_tier(&provider_tier_text)
                            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                        state: RunState::from_str(&state_text)
                            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                        current_step_index: row.get(6)?,
                        retry_count: row.get(7)?,
                        max_retries: row.get(8)?,
                        next_retry_backoff_ms: row.get(9)?,
                        next_retry_at_ms: row.get(10)?,
                        soft_cap_approved: row.get::<_, i64>(11)? == 1,
                        usd_cents_estimate: row.get(12)?,
                        usd_cents_actual: row.get(13)?,
                        failure_reason: row.get(14)?,
                        plan,
                    })
                },
            )
            .map_err(|e| {
                if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
                    RunnerError::RunNotFound
                } else {
                    RunnerError::Db(e.to_string())
                }
            })
    }

    fn get_run_with_learning(
        connection: &mut Connection,
        run_id: &str,
    ) -> Result<RunRecord, RunnerError> {
        let run = Self::get_run(connection, run_id)?;
        if run.state.is_terminal() {
            Self::run_learning_pipeline(connection, &run)?;
            return Self::get_run(connection, run_id);
        }
        Ok(run)
    }

    pub fn get_terminal_receipt(
        connection: &Connection,
        run_id: &str,
    ) -> Result<Option<RunReceipt>, RunnerError> {
        let payload: Option<String> = connection
            .query_row(
                "SELECT content FROM outcomes WHERE run_id = ?1 AND kind = 'receipt' LIMIT 1",
                params![run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| RunnerError::Db(e.to_string()))?;

        match payload {
            Some(json) => {
                let receipt: RunReceipt =
                    serde_json::from_str(&json).map_err(|e| RunnerError::Serde(e.to_string()))?;
                Ok(Some(receipt))
            }
            None => Ok(None),
        }
    }

    fn get_run_in_tx(
        tx: &rusqlite::Transaction<'_>,
        run_id: &str,
    ) -> Result<RunRecord, RunnerError> {
        tx.query_row(
            "
            SELECT id, autopilot_id, idempotency_key,
                   provider_kind, provider_tier,
                   state, current_step_index, retry_count, max_retries,
                   next_retry_backoff_ms, next_retry_at_ms,
                   soft_cap_approved, usd_cents_estimate, usd_cents_actual,
                   failure_reason, plan_json
            FROM runs
            WHERE id = ?1
            ",
            params![run_id],
            |row| {
                let state_text: String = row.get(5)?;
                let provider_kind_text: String = row.get(3)?;
                let provider_tier_text: String = row.get(4)?;
                let plan_json: String = row.get(15)?;
                let plan: AutopilotPlan = serde_json::from_str(&plan_json)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                Ok(RunRecord {
                    id: row.get(0)?,
                    autopilot_id: row.get(1)?,
                    idempotency_key: row.get(2)?,
                    provider_kind: parse_provider_kind(&provider_kind_text)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                    provider_tier: parse_provider_tier(&provider_tier_text)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                    state: RunState::from_str(&state_text)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                    current_step_index: row.get(6)?,
                    retry_count: row.get(7)?,
                    max_retries: row.get(8)?,
                    next_retry_backoff_ms: row.get(9)?,
                    next_retry_at_ms: row.get(10)?,
                    soft_cap_approved: row.get::<_, i64>(11)? == 1,
                    usd_cents_estimate: row.get(12)?,
                    usd_cents_actual: row.get(13)?,
                    failure_reason: row.get(14)?,
                    plan,
                })
            },
        )
        .map_err(|e| {
            if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
                RunnerError::RunNotFound
            } else {
                RunnerError::Db(e.to_string())
            }
        })
    }

    fn run_tick_internal(
        connection: &mut Connection,
        run_id: &str,
        approved_step_id: Option<&str>,
    ) -> Result<RunRecord, RunnerError> {
        let run = Self::get_run_with_learning(connection, run_id)?;

        if run.state.is_terminal() || run.state == RunState::NeedsApproval {
            return Ok(run);
        }

        if run.state == RunState::Retrying {
            let now = now_ms();
            if let Some(next_retry_at) = run.next_retry_at_ms {
                if next_retry_at > now {
                    return Ok(run);
                }
            }
        }

        let runtime_profile = learning::get_runtime_profile(connection, &run.autopilot_id)
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        if runtime_profile.learning_enabled {
            if let Some(until) = runtime_profile.suppress_until_ms {
                if until > now_ms() {
                    let message = format!(
                        "This Autopilot is suppressed until {}. No actions were taken.",
                        until
                    );
                    Self::transition_state_with_activity(
                        connection,
                        run_id,
                        run.state,
                        RunState::Succeeded,
                        "run_suppressed",
                        &message,
                        None,
                        Some(run.current_step_index),
                    )?;
                    return Self::get_run_with_learning(connection, run_id);
                }
            }
        }

        let current_idx = run.current_step_index as usize;
        if current_idx >= run.plan.steps.len() {
            Self::transition_state_with_activity(
                connection,
                run_id,
                run.state,
                RunState::Succeeded,
                "run_succeeded",
                "Run completed successfully.",
                None,
                None,
            )?;
            return Self::get_run_with_learning(connection, run_id);
        }

        let step = run
            .plan
            .steps
            .get(current_idx)
            .ok_or_else(|| RunnerError::Human("Run step index is out of bounds.".to_string()))?
            .clone();

        let is_approved_step = approved_step_id
            .map(|id| id == step.id.as_str())
            .unwrap_or(false);

        if step.requires_approval && !is_approved_step {
            Self::pause_for_approval(connection, &run, &step)?;
            return Self::get_run_with_learning(connection, run_id);
        }

        let step_cost_estimate_cents = estimate_step_cost_usd_cents(&run, &step);
        match Self::evaluate_spend_caps(connection, &run, step_cost_estimate_cents)? {
            CapDecision::Allow => {}
            CapDecision::NeedsSoftApproval { message } => {
                Self::pause_for_soft_cap_approval(connection, &run, &message)?;
                return Self::get_run_with_learning(connection, run_id);
            }
            CapDecision::BlockHard { message } => {
                Self::transition_state_with_activity(
                    connection,
                    run_id,
                    run.state,
                    RunState::Blocked,
                    "run_blocked_hard_cap",
                    &message,
                    Some(&message),
                    Some(current_idx as i64),
                )?;
                return Self::get_run_with_learning(connection, run_id);
            }
        }

        let from_state = run.state;
        match Self::execute_step(connection, &run, &step, &runtime_profile) {
            Ok(result) => {
                if result.actual_spend_usd_cents > 0 {
                    Self::record_spend(
                        connection,
                        &run.id,
                        &step.id,
                        "actual",
                        result.actual_spend_usd_cents,
                        &step,
                    )?;
                }

                let next_idx = result
                    .next_step_index_override
                    .unwrap_or((current_idx as i64) + 1);
                let next_state = result.terminal_state_override.unwrap_or_else(|| {
                    if next_idx as usize >= run.plan.steps.len() {
                        RunState::Succeeded
                    } else {
                        RunState::Ready
                    }
                });
                let user_message = result
                    .terminal_summary_override
                    .as_deref()
                    .unwrap_or(&result.user_message);
                let failure_reason = result.failure_reason_override.as_deref();

                let activity = if next_state.is_terminal() {
                    match next_state {
                        RunState::Succeeded => "run_succeeded",
                        RunState::Failed => "run_failed",
                        RunState::Blocked => "run_blocked",
                        RunState::Canceled => "run_canceled",
                        _ => "step_completed",
                    }
                } else {
                    "step_completed"
                };

                Self::transition_state_with_activity(
                    connection,
                    run_id,
                    from_state,
                    next_state,
                    activity,
                    user_message,
                    failure_reason,
                    Some(next_idx),
                )?;
            }
            Err(error) => {
                if error.retryable && run.retry_count < run.max_retries {
                    let next_retry = run.retry_count + 1;
                    let backoff_ms = compute_backoff_ms(next_retry as u32) as i64;
                    let next_retry_at_ms = now_ms() + backoff_ms;
                    Self::schedule_retry(
                        connection,
                        run_id,
                        from_state,
                        next_retry,
                        backoff_ms,
                        next_retry_at_ms,
                        &error.user_reason,
                    )?;
                    return Self::get_run_with_learning(connection, run_id);
                }

                Self::transition_state_with_activity(
                    connection,
                    run_id,
                    from_state,
                    RunState::Failed,
                    "run_failed",
                    &error.user_reason,
                    Some(&error.user_reason),
                    Some(current_idx as i64),
                )?;
            }
        }

        Self::get_run_with_learning(connection, run_id)
    }

    fn evaluate_spend_caps(
        connection: &Connection,
        run: &RunRecord,
        step_cost_cents: i64,
    ) -> Result<CapDecision, RunnerError> {
        if step_cost_cents <= 0 {
            return Ok(CapDecision::Allow);
        }

        let projected_run = run.usd_cents_actual.saturating_add(step_cost_cents);
        let daily_spend = Self::get_daily_spend_usd_cents(connection)?;
        let projected_daily = daily_spend.saturating_add(step_cost_cents);

        if projected_run > PER_RUN_HARD_CAP_USD_CENTS {
            return Ok(CapDecision::BlockHard {
                message: format!(
                    "This run is blocked before execution: projected cost is about {}, over the per-run hard cap of {}. Reduce scope or adjust caps.",
                    format_usd_cents(projected_run),
                    format_usd_cents(PER_RUN_HARD_CAP_USD_CENTS)
                ),
            });
        }

        if projected_daily > DAILY_HARD_CAP_USD_CENTS {
            return Ok(CapDecision::BlockHard {
                message: format!(
                    "Today's cap is reached: projected daily cost is about {}, over the daily hard cap of {}. Try later or adjust caps.",
                    format_usd_cents(projected_daily),
                    format_usd_cents(DAILY_HARD_CAP_USD_CENTS)
                ),
            });
        }

        if !run.soft_cap_approved
            && (projected_run > PER_RUN_SOFT_CAP_USD_CENTS
                || projected_daily > DAILY_SOFT_CAP_USD_CENTS)
        {
            return Ok(CapDecision::NeedsSoftApproval {
                message: format!(
                    "This run may cost about {}. Continue now, or reduce scope first.",
                    format_usd_cents(projected_run)
                ),
            });
        }

        Ok(CapDecision::Allow)
    }

    fn execute_step(
        connection: &mut Connection,
        run: &RunRecord,
        step: &PlanStep,
        runtime_profile: &RuntimeProfile,
    ) -> Result<StepExecutionResult, StepExecutionError> {
        let guard = PrimitiveGuard::new(run.plan.allowed_primitives.clone());
        if let Err(error) = guard.validate(step.primitive) {
            return Err(StepExecutionError {
                retryable: false,
                user_reason: error.to_string(),
            });
        }

        if step.primitive == PrimitiveId::SendEmail {
            return Err(StepExecutionError {
                retryable: false,
                user_reason:
                    "Sending is disabled right now. Drafts are allowed, sends are blocked."
                        .to_string(),
            });
        }

        match step.primitive {
            PrimitiveId::ReadSources => {
                let max_sources = runtime_profile.max_sources.min(DAILY_SOURCE_MAX_ITEMS);
                let configured = run
                    .plan
                    .daily_sources
                    .iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .take(max_sources)
                    .collect::<Vec<String>>();
                let sources = if configured.is_empty() {
                    vec![
                        "https://example.com/".to_string(),
                        "https://www.rust-lang.org/".to_string(),
                        "Inline note: prioritize product and ops updates".to_string(),
                    ]
                } else {
                    configured
                };

                Self::upsert_daily_brief_sources(connection, &run.autopilot_id, &sources).map_err(
                    |e| StepExecutionError {
                        retryable: false,
                        user_reason: e.to_string(),
                    },
                )?;
                let source_results = Self::read_daily_sources(&sources);
                let sources_hash = compute_daily_sources_hash(&source_results);
                let artifact = DailySourcesArtifact {
                    sources_hash,
                    source_results,
                };
                Self::persist_daily_sources_artifact(connection, run, step, &artifact)?;

                Ok(StepExecutionResult {
                    user_message: "Sources captured for Daily Brief.".to_string(),
                    actual_spend_usd_cents: 0,
                    next_step_index_override: None,
                    terminal_state_override: None,
                    terminal_summary_override: None,
                    failure_reason_override: None,
                })
            }
            PrimitiveId::AggregateDailySummary => {
                let sources_artifact = Self::get_daily_sources_artifact(connection, &run.id)
                    .map_err(|_| StepExecutionError {
                        retryable: false,
                        user_reason: "Couldn't load Daily Brief sources for aggregation."
                            .to_string(),
                    })?
                    .ok_or_else(|| StepExecutionError {
                        retryable: false,
                        user_reason: "Daily Brief sources are missing for this run.".to_string(),
                    })?;

                let usable = sources_artifact
                    .source_results
                    .iter()
                    .filter(|s| s.fetch_error.is_none())
                    .cloned()
                    .collect::<Vec<DailySourceResult>>();
                if usable.is_empty() {
                    return Err(StepExecutionError {
                        retryable: false,
                        user_reason:
                            "Could not fetch any Daily Brief sources. Check source URLs and try again."
                                .to_string(),
                    });
                }

                let runtime = ProviderRuntime::default();
                let memory_context =
                    learning::build_memory_context(connection, &run.autopilot_id, run.plan.recipe)
                        .map_err(|e| StepExecutionError {
                            retryable: false,
                            user_reason: format!("Couldn't load learning context: {e}"),
                        })?;
                let source_context = usable
                    .iter()
                    .map(|s| format!("[{}] {}\n{}", s.source_id, s.url, s.text_excerpt))
                    .collect::<Vec<String>>()
                    .join("\n\n");
                let mode_hint = match runtime_profile.mode {
                    learning::LearningMode::MaxSavings => "Mode: Max Savings. Keep output concise.",
                    learning::LearningMode::BestQuality => {
                        "Mode: Best Quality. Prioritize fidelity."
                    }
                    learning::LearningMode::Balanced => "Mode: Balanced.",
                };
                let memory_block = if memory_context.prompt_block.is_empty() {
                    String::new()
                } else {
                    format!("\n{}\n", memory_context.prompt_block)
                };
                let request = ProviderRequest {
                    provider_kind: run.provider_kind,
                    provider_tier: run.provider_tier,
                    model: run.plan.provider.default_model.clone(),
                    input: format!(
                        "Intent: {}\nTask: Create a cohesive daily brief.\n{}\nOutput format:\nTitle: <one line>\n- bullet 1\n- bullet 2\n- bullet 3\n{}\nSources:\n{}",
                        run.plan.intent,
                        mode_hint,
                        memory_block,
                        source_context
                    ),
                    max_output_tokens: Some(match runtime_profile.mode {
                        learning::LearningMode::MaxSavings => 420,
                        learning::LearningMode::BestQuality => 780,
                        learning::LearningMode::Balanced => 700,
                    }),
                    correlation_id: Some(format!("{}:{}", run.id, step.id)),
                };
                let response = runtime.dispatch(&request).map_err(map_provider_error)?;
                let parsed = parse_daily_summary_output(
                    &response.text,
                    &sources_artifact.sources_hash,
                    runtime_profile.max_bullets,
                );
                learning::persist_memory_usage(
                    connection,
                    &run.id,
                    &step.id,
                    &memory_context.titles,
                )
                .map_err(|e| StepExecutionError {
                    retryable: false,
                    user_reason: format!("Couldn't persist learning context usage: {e}"),
                })?;
                let seen_before = Self::daily_summary_exists(
                    connection,
                    &run.autopilot_id,
                    &parsed.sources_hash,
                    &parsed.content_hash,
                )
                .map_err(|e| StepExecutionError {
                    retryable: false,
                    user_reason: e.to_string(),
                })?;
                Self::persist_daily_summary_artifact(connection, run, step, &parsed)?;
                if !seen_before {
                    Self::insert_daily_summary_history(
                        connection,
                        &run.autopilot_id,
                        &run.id,
                        &parsed,
                    )
                    .map_err(|e| StepExecutionError {
                        retryable: false,
                        user_reason: e.to_string(),
                    })?;
                }

                let fallback_estimate = estimate_step_cost_usd_cents(run, step);
                let total_cents =
                    std::cmp::max(fallback_estimate, response.usage.estimated_cost_usd_cents);
                if total_cents > 0 {
                    Self::record_spend_by_sources(connection, run, step, &usable, total_cents)?;
                }

                if seen_before {
                    return Ok(StepExecutionResult {
                        user_message:
                            "Daily Brief sources are unchanged. No new summary draft created."
                                .to_string(),
                        actual_spend_usd_cents: 0,
                        next_step_index_override: Some(run.plan.steps.len() as i64),
                        terminal_state_override: Some(RunState::Succeeded),
                        terminal_summary_override: Some(
                            "Daily Brief unchanged. Existing summary is still current.".to_string(),
                        ),
                        failure_reason_override: None,
                    });
                }

                Ok(StepExecutionResult {
                    user_message: "Daily summary aggregated from sources.".to_string(),
                    actual_spend_usd_cents: 0,
                    next_step_index_override: None,
                    terminal_state_override: None,
                    terminal_summary_override: None,
                    failure_reason_override: None,
                })
            }
            PrimitiveId::ReadWeb => {
                if run.plan.recipe != RecipeKind::WebsiteMonitor {
                    return Ok(StepExecutionResult {
                        user_message: "Step completed.".to_string(),
                        actual_spend_usd_cents: 0,
                        next_step_index_override: None,
                        terminal_state_override: None,
                        terminal_summary_override: None,
                        failure_reason_override: None,
                    });
                }

                let source_url = run.plan.web_source_url.clone().ok_or_else(|| StepExecutionError {
                    retryable: false,
                    user_reason:
                        "Add a website URL to this Autopilot intent before running website monitoring."
                            .to_string(),
                })?;
                if run.plan.web_allowed_domains.is_empty() {
                    return Err(StepExecutionError {
                        retryable: false,
                        user_reason:
                            "This Autopilot has no allowed website domains yet. Add one and try again."
                                .to_string(),
                    });
                }

                let fetched = fetch_allowlisted_text(&source_url, &run.plan.web_allowed_domains)
                    .map_err(map_web_fetch_error)?;
                let previous = Self::get_web_snapshot(connection, &run.autopilot_id, &fetched.url)
                    .map_err(|e| StepExecutionError {
                        retryable: false,
                        user_reason: e.to_string(),
                    })?;
                let changed = previous
                    .as_ref()
                    .map(|prev| prev.last_hash != fetched.content_hash)
                    .unwrap_or(true);
                let diff_score = previous
                    .as_ref()
                    .map(|prev| compute_diff_score(&prev.last_text_excerpt, &fetched.content_text))
                    .unwrap_or(1.0);

                let artifact = WebReadArtifact {
                    url: fetched.url.clone(),
                    fetched_at_ms: fetched.fetched_at_ms,
                    status_code: fetched.status_code,
                    content_hash: fetched.content_hash.clone(),
                    changed,
                    diff_score,
                    current_excerpt: fetched.content_text.clone(),
                    previous_excerpt: previous.as_ref().map(|p| p.last_text_excerpt.clone()),
                };

                Self::upsert_web_snapshot(
                    connection,
                    &run.autopilot_id,
                    &fetched,
                    changed,
                    previous.as_ref(),
                )
                .map_err(|e| StepExecutionError {
                    retryable: false,
                    user_reason: e.to_string(),
                })?;
                Self::persist_web_read_artifact(connection, run, step, &artifact)?;

                if !changed {
                    return Ok(StepExecutionResult {
                        user_message: "No changes detected.".to_string(),
                        actual_spend_usd_cents: 0,
                        next_step_index_override: Some(run.plan.steps.len() as i64),
                        terminal_state_override: Some(RunState::Succeeded),
                        terminal_summary_override: Some(
                            "No changes detected for this website since the last snapshot."
                                .to_string(),
                        ),
                        failure_reason_override: None,
                    });
                }

                if diff_score < runtime_profile.min_diff_score_to_notify {
                    let _ = learning::record_decision_event(
                        connection,
                        &run.autopilot_id,
                        &run.id,
                        Some(&step.id),
                        DecisionEventType::OutcomeIgnored,
                        DecisionEventMetadata {
                            reason_code: Some("below_diff_threshold".to_string()),
                            diff_score: Some(diff_score),
                            content_hash: Some(artifact.content_hash.clone()),
                            content_length: Some(artifact.current_excerpt.chars().count() as i64),
                            ..Default::default()
                        },
                    );
                    return Ok(StepExecutionResult {
                        user_message: "Change was below your notify threshold.".to_string(),
                        actual_spend_usd_cents: 0,
                        next_step_index_override: Some(run.plan.steps.len() as i64),
                        terminal_state_override: Some(RunState::Succeeded),
                        terminal_summary_override: Some(
                            "Change detected but suppressed due to your current sensitivity settings."
                                .to_string(),
                        ),
                        failure_reason_override: None,
                    });
                }

                Ok(StepExecutionResult {
                    user_message: "Website change detected. Continuing to draft summary."
                        .to_string(),
                    actual_spend_usd_cents: 0,
                    next_step_index_override: None,
                    terminal_state_override: None,
                    terminal_summary_override: None,
                    failure_reason_override: None,
                })
            }
            PrimitiveId::WriteOutcomeDraft | PrimitiveId::WriteEmailDraft => {
                let runtime = ProviderRuntime::default();
                let memory_context =
                    learning::build_memory_context(connection, &run.autopilot_id, run.plan.recipe)
                        .map_err(|e| StepExecutionError {
                            retryable: false,
                            user_reason: format!("Couldn't load learning context: {e}"),
                        })?;
                let mut model_input = if run.plan.recipe == RecipeKind::WebsiteMonitor {
                    Self::build_website_monitor_prompt(connection, run, step)
                        .unwrap_or_else(|_| format!("{}\n\nStep: {}", run.plan.intent, step.label))
                } else if run.plan.recipe == RecipeKind::InboxTriage {
                    Self::build_inbox_triage_prompt(connection, run, step)
                        .unwrap_or_else(|_| format!("{}\n\nStep: {}", run.plan.intent, step.label))
                } else if run.plan.recipe == RecipeKind::DailyBrief {
                    Self::build_daily_brief_draft_prompt(connection, run)
                        .unwrap_or_else(|_| format!("{}\n\nStep: {}", run.plan.intent, step.label))
                } else {
                    format!("{}\n\nStep: {}", run.plan.intent, step.label)
                };
                if run.plan.recipe == RecipeKind::InboxTriage {
                    model_input.push_str(&format!(
                        "\nReply length preference: {}.",
                        runtime_profile.reply_length_hint
                    ));
                }
                if !memory_context.prompt_block.is_empty() {
                    model_input.push_str(&format!("\n\n{}", memory_context.prompt_block));
                }
                let request = ProviderRequest {
                    provider_kind: run.provider_kind,
                    provider_tier: run.provider_tier,
                    model: run.plan.provider.default_model.clone(),
                    input: model_input,
                    max_output_tokens: Some(match runtime_profile.mode {
                        learning::LearningMode::MaxSavings => 320,
                        learning::LearningMode::BestQuality => 640,
                        learning::LearningMode::Balanced => 512,
                    }),
                    correlation_id: Some(format!("{}:{}", run.id, step.id)),
                };

                let response = runtime.dispatch(&request).map_err(map_provider_error)?;
                learning::persist_memory_usage(
                    connection,
                    &run.id,
                    &step.id,
                    &memory_context.titles,
                )
                .map_err(|e| StepExecutionError {
                    retryable: false,
                    user_reason: format!("Couldn't persist learning context usage: {e}"),
                })?;
                Self::persist_provider_output(connection, run, step, &response)?;
                if run.plan.recipe == RecipeKind::InboxTriage
                    && step.primitive == PrimitiveId::WriteEmailDraft
                {
                    Self::mark_inbox_item_processed(connection, run)?;
                }
                let fallback_estimate_cents = estimate_step_cost_usd_cents(run, step);
                let actual_cents = std::cmp::max(
                    fallback_estimate_cents,
                    response.usage.estimated_cost_usd_cents,
                );

                Ok(StepExecutionResult {
                    user_message: if step.primitive == PrimitiveId::WriteEmailDraft {
                        "Draft email created and queued for approval.".to_string()
                    } else {
                        "Draft outcome saved.".to_string()
                    },
                    actual_spend_usd_cents: actual_cents,
                    next_step_index_override: None,
                    terminal_state_override: None,
                    terminal_summary_override: None,
                    failure_reason_override: None,
                })
            }
            PrimitiveId::ReadForwardedEmail => {
                let raw_input = run
                    .plan
                    .inbox_source_text
                    .clone()
                    .unwrap_or_else(|| run.plan.intent.clone());
                let normalized = raw_input.trim().to_string();
                if normalized.is_empty() {
                    return Err(StepExecutionError {
                        retryable: false,
                        user_reason: "Paste forwarded email text before running Inbox Triage."
                            .to_string(),
                    });
                }
                if normalized.chars().count() > INBOX_TEXT_MAX_CHARS {
                    return Err(StepExecutionError {
                        retryable: false,
                        user_reason:
                            "Forwarded email text is too large. Paste a smaller message or trim quoted threads."
                                .to_string(),
                    });
                }

                let content_hash = fnv1a_64_hex(&normalized);
                let item = Self::upsert_inbox_item(
                    connection,
                    &run.autopilot_id,
                    &normalized,
                    &content_hash,
                )
                .map_err(|e| StepExecutionError {
                    retryable: false,
                    user_reason: e.to_string(),
                })?;

                let artifact = InboxReadArtifact {
                    item_id: item.id.clone(),
                    content_hash: item.content_hash.clone(),
                    text_excerpt: truncate_chars(&item.raw_text, 1200),
                    created_at_ms: now_ms(),
                    deduped_existing: item.processed_at_ms.is_some(),
                };
                Self::persist_inbox_read_artifact(connection, run, step, &artifact)?;

                if item.processed_at_ms.is_some() {
                    return Ok(StepExecutionResult {
                        user_message:
                            "This forwarded email was already processed. No new draft created."
                                .to_string(),
                        actual_spend_usd_cents: 0,
                        next_step_index_override: Some(run.plan.steps.len() as i64),
                        terminal_state_override: Some(RunState::Succeeded),
                        terminal_summary_override: Some(
                            "Email already processed. Existing draft remains available."
                                .to_string(),
                        ),
                        failure_reason_override: None,
                    });
                }

                Ok(StepExecutionResult {
                    user_message: "Forwarded email captured for triage.".to_string(),
                    actual_spend_usd_cents: 0,
                    next_step_index_override: None,
                    terminal_state_override: None,
                    terminal_summary_override: None,
                    failure_reason_override: None,
                })
            }
            PrimitiveId::ReadVaultFile | PrimitiveId::ScheduleRun | PrimitiveId::NotifyUser => {
                Ok(StepExecutionResult {
                    user_message: "Step completed.".to_string(),
                    actual_spend_usd_cents: 0,
                    next_step_index_override: None,
                    terminal_state_override: None,
                    terminal_summary_override: None,
                    failure_reason_override: None,
                })
            }
            PrimitiveId::SendEmail => Err(StepExecutionError {
                retryable: false,
                user_reason:
                    "Sending is disabled right now. Drafts are allowed, sends are blocked."
                        .to_string(),
            }),
        }
    }

    fn persist_provider_output(
        connection: &Connection,
        run: &RunRecord,
        step: &PlanStep,
        response: &ProviderResponse,
    ) -> Result<(), StepExecutionError> {
        let kind = if step.primitive == PrimitiveId::WriteEmailDraft {
            "email_draft"
        } else {
            "outcome_draft"
        };

        let content = redact_text(&response.text);
        connection
            .execute(
                "
                INSERT INTO outcomes (
                  id, run_id, step_id, kind, status, content,
                  created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, 'drafted', ?5, ?6, ?6)
                ON CONFLICT(run_id, step_id, kind)
                DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at
                ",
                params![make_id("outcome"), run.id, step.id, kind, content, now_ms()],
            )
            .map_err(|_| StepExecutionError {
                retryable: true,
                user_reason: "Couldn't save generated output yet.".to_string(),
            })?;

        Ok(())
    }

    fn persist_web_read_artifact(
        connection: &Connection,
        run: &RunRecord,
        step: &PlanStep,
        artifact: &WebReadArtifact,
    ) -> Result<(), StepExecutionError> {
        let payload = serde_json::to_string(artifact).map_err(|_| StepExecutionError {
            retryable: false,
            user_reason: "Couldn't store website snapshot details.".to_string(),
        })?;
        connection
            .execute(
                "
                INSERT INTO outcomes (
                  id, run_id, step_id, kind, status, content, created_at, updated_at
                ) VALUES (?1, ?2, ?3, 'web_read', 'captured', ?4, ?5, ?5)
                ON CONFLICT(run_id, step_id, kind)
                DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at
                ",
                params![make_id("outcome"), run.id, step.id, payload, now_ms()],
            )
            .map_err(|_| StepExecutionError {
                retryable: false,
                user_reason: "Couldn't save website read artifact.".to_string(),
            })?;
        Ok(())
    }

    fn get_web_read_artifact(
        connection: &Connection,
        run_id: &str,
    ) -> Result<Option<WebReadArtifact>, RunnerError> {
        let payload: Option<String> = connection
            .query_row(
                "SELECT content FROM outcomes WHERE run_id = ?1 AND kind = 'web_read' LIMIT 1",
                params![run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| RunnerError::Db(e.to_string()))?;

        match payload {
            Some(json) => {
                let artifact =
                    serde_json::from_str(&json).map_err(|e| RunnerError::Serde(e.to_string()))?;
                Ok(Some(artifact))
            }
            None => Ok(None),
        }
    }

    fn build_website_monitor_prompt(
        connection: &Connection,
        run: &RunRecord,
        step: &PlanStep,
    ) -> Result<String, StepExecutionError> {
        let artifact = Self::get_web_read_artifact(connection, &run.id)
            .map_err(|_| StepExecutionError {
                retryable: false,
                user_reason: "Couldn't load website snapshot for drafting.".to_string(),
            })?
            .ok_or_else(|| StepExecutionError {
                retryable: false,
                user_reason: "Website content is missing for this run.".to_string(),
            })?;

        let previous = artifact
            .previous_excerpt
            .as_deref()
            .unwrap_or("No previous snapshot.");
        let current = artifact.current_excerpt.as_str();
        let task = if step.primitive == PrimitiveId::WriteEmailDraft {
            "Draft a calm email update describing the key changes."
        } else {
            "Summarize what's changed in concise bullets."
        };

        Ok(format!(
            "Intent: {}\nTask: {}\nURL: {}\nFetched at: {}\nPrevious snapshot:\n{}\n\nCurrent snapshot:\n{}\n",
            run.plan.intent,
            task,
            artifact.url,
            artifact.fetched_at_ms,
            previous,
            current
        ))
    }

    fn persist_inbox_read_artifact(
        connection: &Connection,
        run: &RunRecord,
        step: &PlanStep,
        artifact: &InboxReadArtifact,
    ) -> Result<(), StepExecutionError> {
        let payload = serde_json::to_string(artifact).map_err(|_| StepExecutionError {
            retryable: false,
            user_reason: "Couldn't store forwarded email artifact.".to_string(),
        })?;
        connection
            .execute(
                "
                INSERT INTO outcomes (
                  id, run_id, step_id, kind, status, content, created_at, updated_at
                ) VALUES (?1, ?2, ?3, 'inbox_read', 'captured', ?4, ?5, ?5)
                ON CONFLICT(run_id, step_id, kind)
                DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at
                ",
                params![make_id("outcome"), run.id, step.id, payload, now_ms()],
            )
            .map_err(|_| StepExecutionError {
                retryable: false,
                user_reason: "Couldn't save forwarded email artifact.".to_string(),
            })?;
        Ok(())
    }

    fn get_inbox_read_artifact(
        connection: &Connection,
        run_id: &str,
    ) -> Result<Option<InboxReadArtifact>, RunnerError> {
        let payload: Option<String> = connection
            .query_row(
                "SELECT content FROM outcomes WHERE run_id = ?1 AND kind = 'inbox_read' LIMIT 1",
                params![run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        match payload {
            Some(json) => {
                let artifact =
                    serde_json::from_str(&json).map_err(|e| RunnerError::Serde(e.to_string()))?;
                Ok(Some(artifact))
            }
            None => Ok(None),
        }
    }

    fn build_inbox_triage_prompt(
        connection: &Connection,
        run: &RunRecord,
        step: &PlanStep,
    ) -> Result<String, StepExecutionError> {
        let artifact = Self::get_inbox_read_artifact(connection, &run.id)
            .map_err(|_| StepExecutionError {
                retryable: false,
                user_reason: "Couldn't load forwarded email for drafting.".to_string(),
            })?
            .ok_or_else(|| StepExecutionError {
                retryable: false,
                user_reason: "Forwarded email content is missing for this run.".to_string(),
            })?;
        let task = if step.primitive == PrimitiveId::WriteEmailDraft {
            "Draft a clear, concise reply email."
        } else {
            "Summarize the email and suggest triage labels."
        };
        Ok(format!(
            "Intent: {}\nTask: {}\nForwarded email:\n{}\n",
            run.plan.intent, task, artifact.text_excerpt
        ))
    }

    fn upsert_inbox_item(
        connection: &mut Connection,
        autopilot_id: &str,
        raw_text: &str,
        content_hash: &str,
    ) -> Result<InboxItemRecord, RunnerError> {
        let now = now_ms();
        connection
            .execute(
                "
                INSERT OR IGNORE INTO inbox_items (id, autopilot_id, content_hash, raw_text, created_at_ms, processed_at_ms)
                VALUES (?1, ?2, ?3, ?4, ?5, NULL)
                ",
                params![make_id("inbox"), autopilot_id, content_hash, raw_text, now],
            )
            .map_err(|e| RunnerError::Db(e.to_string()))?;

        connection
            .query_row(
                "SELECT id, content_hash, raw_text, processed_at_ms FROM inbox_items WHERE content_hash = ?1",
                params![content_hash],
                |row| {
                    Ok(InboxItemRecord {
                        id: row.get(0)?,
                        content_hash: row.get(1)?,
                        raw_text: row.get(2)?,
                        processed_at_ms: row.get(3)?,
                    })
                },
            )
            .map_err(|e| RunnerError::Db(e.to_string()))
    }

    fn mark_inbox_item_processed(
        connection: &Connection,
        run: &RunRecord,
    ) -> Result<(), StepExecutionError> {
        let artifact = Self::get_inbox_read_artifact(connection, &run.id)
            .map_err(|_| StepExecutionError {
                retryable: false,
                user_reason: "Couldn't load forwarded email record.".to_string(),
            })?
            .ok_or_else(|| StepExecutionError {
                retryable: false,
                user_reason: "Forwarded email record is missing.".to_string(),
            })?;

        connection
            .execute(
                "UPDATE inbox_items SET processed_at_ms = COALESCE(processed_at_ms, ?1) WHERE id = ?2",
                params![now_ms(), artifact.item_id],
            )
            .map_err(|_| StepExecutionError {
                retryable: false,
                user_reason: "Couldn't finalize forwarded email processing state.".to_string(),
            })?;
        Ok(())
    }

    fn upsert_daily_brief_sources(
        connection: &mut Connection,
        autopilot_id: &str,
        sources: &[String],
    ) -> Result<(), RunnerError> {
        let sources_json =
            serde_json::to_string(sources).map_err(|e| RunnerError::Serde(e.to_string()))?;
        let sources_hash = fnv1a_64_hex(&sources_json);
        let current: Option<String> = connection
            .query_row(
                "SELECT sources_hash FROM daily_brief_sources WHERE autopilot_id = ?1",
                params![autopilot_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| RunnerError::Db(e.to_string()))?;

        if current.as_deref() == Some(sources_hash.as_str()) {
            return Ok(());
        }

        connection
            .execute(
                "
                INSERT INTO daily_brief_sources (autopilot_id, sources_json, sources_hash, updated_at_ms)
                VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT(autopilot_id) DO UPDATE SET
                  sources_json = excluded.sources_json,
                  sources_hash = excluded.sources_hash,
                  updated_at_ms = excluded.updated_at_ms
                ",
                params![autopilot_id, sources_json, sources_hash, now_ms()],
            )
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        Ok(())
    }

    fn read_daily_sources(inputs: &[String]) -> Vec<DailySourceResult> {
        inputs
            .iter()
            .enumerate()
            .map(|(idx, raw)| {
                let source_id = format!("source_{}", idx + 1);
                let trimmed = raw.trim().to_string();
                if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                    let host_allowlist = parse_scheme_host(&trimmed)
                        .map(|(_, host)| vec![host])
                        .unwrap_or_default();
                    match fetch_allowlisted_text(&trimmed, &host_allowlist) {
                        Ok(fetched) => DailySourceResult {
                            source_id,
                            url: fetched.url,
                            text_excerpt: truncate_chars(&fetched.content_text, 1200),
                            fetched_at_ms: fetched.fetched_at_ms,
                            fetch_error: None,
                        },
                        Err(err) => DailySourceResult {
                            source_id,
                            url: trimmed,
                            text_excerpt: String::new(),
                            fetched_at_ms: now_ms(),
                            fetch_error: Some(err.to_string()),
                        },
                    }
                } else {
                    DailySourceResult {
                        source_id,
                        url: "inline://text".to_string(),
                        text_excerpt: truncate_chars(&trimmed, 1200),
                        fetched_at_ms: now_ms(),
                        fetch_error: None,
                    }
                }
            })
            .collect::<Vec<DailySourceResult>>()
    }

    fn persist_daily_sources_artifact(
        connection: &Connection,
        run: &RunRecord,
        step: &PlanStep,
        artifact: &DailySourcesArtifact,
    ) -> Result<(), StepExecutionError> {
        let payload = serde_json::to_string(artifact).map_err(|_| StepExecutionError {
            retryable: false,
            user_reason: "Couldn't store Daily Brief source artifact.".to_string(),
        })?;
        connection
            .execute(
                "
                INSERT INTO outcomes (
                  id, run_id, step_id, kind, status, content, created_at, updated_at
                ) VALUES (?1, ?2, ?3, 'daily_sources', 'captured', ?4, ?5, ?5)
                ON CONFLICT(run_id, step_id, kind)
                DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at
                ",
                params![make_id("outcome"), run.id, step.id, payload, now_ms()],
            )
            .map_err(|_| StepExecutionError {
                retryable: false,
                user_reason: "Couldn't save Daily Brief source artifact.".to_string(),
            })?;
        Ok(())
    }

    fn get_daily_sources_artifact(
        connection: &Connection,
        run_id: &str,
    ) -> Result<Option<DailySourcesArtifact>, RunnerError> {
        let payload: Option<String> = connection
            .query_row(
                "SELECT content FROM outcomes WHERE run_id = ?1 AND kind = 'daily_sources' LIMIT 1",
                params![run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        match payload {
            Some(json) => {
                let artifact =
                    serde_json::from_str(&json).map_err(|e| RunnerError::Serde(e.to_string()))?;
                Ok(Some(artifact))
            }
            None => Ok(None),
        }
    }

    fn persist_daily_summary_artifact(
        connection: &Connection,
        run: &RunRecord,
        step: &PlanStep,
        artifact: &DailySummaryArtifact,
    ) -> Result<(), StepExecutionError> {
        let payload = serde_json::to_string(artifact).map_err(|_| StepExecutionError {
            retryable: false,
            user_reason: "Couldn't store Daily Brief summary artifact.".to_string(),
        })?;
        connection
            .execute(
                "
                INSERT INTO outcomes (
                  id, run_id, step_id, kind, status, content, created_at, updated_at
                ) VALUES (?1, ?2, ?3, 'daily_summary', 'aggregated', ?4, ?5, ?5)
                ON CONFLICT(run_id, step_id, kind)
                DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at
                ",
                params![make_id("outcome"), run.id, step.id, payload, now_ms()],
            )
            .map_err(|_| StepExecutionError {
                retryable: false,
                user_reason: "Couldn't save Daily Brief summary artifact.".to_string(),
            })?;
        Ok(())
    }

    fn get_daily_summary_artifact(
        connection: &Connection,
        run_id: &str,
    ) -> Result<Option<DailySummaryArtifact>, RunnerError> {
        let payload: Option<String> = connection
            .query_row(
                "SELECT content FROM outcomes WHERE run_id = ?1 AND kind = 'daily_summary' LIMIT 1",
                params![run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        match payload {
            Some(json) => {
                let artifact =
                    serde_json::from_str(&json).map_err(|e| RunnerError::Serde(e.to_string()))?;
                Ok(Some(artifact))
            }
            None => Ok(None),
        }
    }

    fn daily_summary_exists(
        connection: &Connection,
        autopilot_id: &str,
        sources_hash: &str,
        content_hash: &str,
    ) -> Result<bool, RunnerError> {
        let found: Option<String> = connection
            .query_row(
                "SELECT id FROM daily_brief_history WHERE autopilot_id = ?1 AND sources_hash = ?2 AND content_hash = ?3 LIMIT 1",
                params![autopilot_id, sources_hash, content_hash],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        Ok(found.is_some())
    }

    fn insert_daily_summary_history(
        connection: &Connection,
        autopilot_id: &str,
        run_id: &str,
        artifact: &DailySummaryArtifact,
    ) -> Result<(), RunnerError> {
        let summary_json =
            serde_json::to_string(artifact).map_err(|e| RunnerError::Serde(e.to_string()))?;
        connection
            .execute(
                "
                INSERT OR IGNORE INTO daily_brief_history (
                  id, autopilot_id, run_id, sources_hash, content_hash, summary_json, created_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ",
                params![
                    make_id("brief_hist"),
                    autopilot_id,
                    run_id,
                    artifact.sources_hash,
                    artifact.content_hash,
                    summary_json,
                    now_ms()
                ],
            )
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        Ok(())
    }

    fn build_daily_brief_draft_prompt(
        connection: &Connection,
        run: &RunRecord,
    ) -> Result<String, StepExecutionError> {
        let summary = Self::get_daily_summary_artifact(connection, &run.id)
            .map_err(|_| StepExecutionError {
                retryable: false,
                user_reason: "Couldn't load Daily Brief summary for drafting.".to_string(),
            })?
            .ok_or_else(|| StepExecutionError {
                retryable: false,
                user_reason: "Daily Brief summary is missing for this run.".to_string(),
            })?;

        let bullets = summary
            .bullet_points
            .iter()
            .map(|b| format!("- {b}"))
            .collect::<Vec<String>>()
            .join("\n");

        Ok(format!(
            "Create a polished Daily Brief card.\nTitle: {}\nBullets:\n{}\nSummary:\n{}",
            summary.title, bullets, summary.summary_text
        ))
    }

    fn record_spend_by_sources(
        connection: &mut Connection,
        run: &RunRecord,
        step: &PlanStep,
        sources: &[DailySourceResult],
        total_cents: i64,
    ) -> Result<(), StepExecutionError> {
        if sources.is_empty() || total_cents <= 0 {
            return Ok(());
        }
        let count = sources.len() as i64;
        let base = total_cents / count;
        let mut remainder = total_cents % count;
        for source in sources {
            let mut cents = base;
            if remainder > 0 {
                cents += 1;
                remainder -= 1;
            }
            let step_id = format!("{}:{}", step.id, source.source_id);
            Self::record_spend(connection, &run.id, &step_id, "source_usage", cents, step)
                .map_err(|e| StepExecutionError {
                    retryable: false,
                    user_reason: e.to_string(),
                })?;
        }
        Ok(())
    }

    fn get_web_snapshot(
        connection: &Connection,
        autopilot_id: &str,
        url: &str,
    ) -> Result<Option<WebSnapshotRecord>, RunnerError> {
        connection
            .query_row(
                "SELECT autopilot_id, url, last_hash, last_fetched_at_ms, last_text_excerpt FROM web_snapshots WHERE autopilot_id = ?1 AND url = ?2",
                params![autopilot_id, url],
                |row| {
                    Ok(WebSnapshotRecord {
                        last_hash: row.get(2)?,
                        last_text_excerpt: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(|e| RunnerError::Db(e.to_string()))
    }

    fn upsert_web_snapshot(
        connection: &mut Connection,
        autopilot_id: &str,
        fetched: &WebFetchResult,
        changed: bool,
        previous: Option<&WebSnapshotRecord>,
    ) -> Result<(), RunnerError> {
        let excerpt = if changed {
            truncate_chars(&fetched.content_text, 2_000)
        } else {
            previous
                .map(|p| p.last_text_excerpt.clone())
                .unwrap_or_else(|| truncate_chars(&fetched.content_text, 2_000))
        };
        connection
            .execute(
                "
                INSERT INTO web_snapshots (
                  autopilot_id, url, last_hash, last_fetched_at_ms, last_text_excerpt, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT(autopilot_id, url)
                DO UPDATE SET
                  last_hash = excluded.last_hash,
                  last_fetched_at_ms = excluded.last_fetched_at_ms,
                  last_text_excerpt = excluded.last_text_excerpt,
                  updated_at = excluded.updated_at
                ",
                params![
                    autopilot_id,
                    fetched.url,
                    fetched.content_hash,
                    fetched.fetched_at_ms,
                    excerpt,
                    now_ms()
                ],
            )
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        Ok(())
    }

    fn get_daily_spend_usd_cents(connection: &Connection) -> Result<i64, RunnerError> {
        let day_bucket = current_day_bucket();
        let spent: Option<i64> = connection
            .query_row(
                "SELECT SUM(amount_usd_cents) FROM spend_ledger WHERE day_bucket = ?1",
                params![day_bucket],
                |row| row.get(0),
            )
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        Ok(spent.unwrap_or(0))
    }

    fn record_spend(
        connection: &mut Connection,
        run_id: &str,
        step_id: &str,
        entry_kind: &str,
        amount_usd_cents: i64,
        step: &PlanStep,
    ) -> Result<(), RunnerError> {
        let tx = connection
            .transaction()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let now = now_ms();

        tx.execute(
            "
            INSERT INTO spend_ledger (id, run_id, step_id, entry_kind, amount_usd, amount_usd_cents, reason, day_bucket, created_at)
            VALUES (?1, ?2, ?3, ?4, 0.0, ?5, ?6, ?7, ?8)
            ON CONFLICT(run_id, step_id, entry_kind) DO NOTHING
            ",
            params![
                make_id("spend"),
                run_id,
                step_id,
                entry_kind,
                amount_usd_cents,
                format!("Step {}", step.id),
                current_day_bucket(),
                now
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        let total: i64 = tx
            .query_row(
                "SELECT COALESCE(SUM(amount_usd_cents), 0) FROM spend_ledger WHERE run_id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.execute(
            "
            UPDATE runs
            SET usd_cents_actual = ?1,
                usd_cents_estimate = ?1,
                updated_at = ?2
            WHERE id = ?3
            ",
            params![total, now, run_id],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.commit().map_err(|e| RunnerError::Db(e.to_string()))
    }

    fn get_run_by_idempotency_key(
        connection: &Connection,
        idempotency_key: &str,
    ) -> Result<Option<RunRecord>, RunnerError> {
        let run_id: Option<String> = connection
            .query_row(
                "SELECT id FROM runs WHERE idempotency_key = ?1",
                params![idempotency_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| RunnerError::Db(e.to_string()))?;

        match run_id {
            Some(id) => Self::get_run(connection, &id).map(Some),
            None => Ok(None),
        }
    }

    fn get_approval(
        connection: &Connection,
        approval_id: &str,
    ) -> Result<ApprovalRecord, RunnerError> {
        connection
            .query_row(
                "SELECT id, run_id, step_id, status, preview, reason FROM approvals WHERE id = ?1",
                params![approval_id],
                |row| {
                    Ok(ApprovalRecord {
                        id: row.get(0)?,
                        run_id: row.get(1)?,
                        step_id: row.get(2)?,
                        status: row.get(3)?,
                        preview: row.get(4)?,
                        reason: row.get(5)?,
                    })
                },
            )
            .map_err(|e| {
                if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
                    RunnerError::ApprovalNotFound
                } else {
                    RunnerError::Db(e.to_string())
                }
            })
    }

    fn get_approval_created_at(
        connection: &Connection,
        approval_id: &str,
    ) -> Result<Option<i64>, RunnerError> {
        connection
            .query_row(
                "SELECT created_at FROM approvals WHERE id = ?1",
                params![approval_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| RunnerError::Db(e.to_string()))
    }

    fn pause_for_approval(
        connection: &mut Connection,
        run: &RunRecord,
        step: &PlanStep,
    ) -> Result<(), RunnerError> {
        let tx = connection
            .transaction()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let now = now_ms();

        tx.execute(
            "
            INSERT OR IGNORE INTO approvals
              (id, run_id, step_id, status, preview, created_at, updated_at)
            VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?5)
            ",
            params![
                make_id("approval"),
                run.id,
                step.id,
                format!("Approve step: {}", step.label),
                now
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.execute(
            "
            UPDATE runs
            SET state = ?1,
                failure_reason = NULL,
                next_retry_backoff_ms = NULL,
                next_retry_at_ms = NULL,
                updated_at = ?2
            WHERE id = ?3
            ",
            params![RunState::NeedsApproval.as_str(), now, run.id],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.execute(
            "
            INSERT INTO activities (id, run_id, activity_type, from_state, to_state, user_message, created_at)
            VALUES (?1, ?2, 'approval_required', ?3, ?4, ?5, ?6)
            ",
            params![
                make_id("activity"),
                run.id,
                run.state.as_str(),
                RunState::NeedsApproval.as_str(),
                format!("Approval required for step: {}", step.label),
                now
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.commit().map_err(|e| RunnerError::Db(e.to_string()))
    }

    fn pause_for_soft_cap_approval(
        connection: &mut Connection,
        run: &RunRecord,
        message: &str,
    ) -> Result<(), RunnerError> {
        let tx = connection
            .transaction()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let now = now_ms();

        tx.execute(
            "
            INSERT OR IGNORE INTO approvals
              (id, run_id, step_id, status, preview, created_at, updated_at)
            VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?5)
            ",
            params![
                make_id("approval"),
                run.id,
                SOFT_CAP_APPROVAL_STEP_ID,
                message,
                now
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.execute(
            "
            UPDATE runs
            SET state = 'needs_approval',
                updated_at = ?1
            WHERE id = ?2
            ",
            params![now, run.id],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.execute(
            "
            INSERT INTO activities (id, run_id, activity_type, from_state, to_state, user_message, created_at)
            VALUES (?1, ?2, 'spend_soft_cap_approval_required', ?3, 'needs_approval', ?4, ?5)
            ",
            params![make_id("activity"), run.id, run.state.as_str(), message, now],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.commit().map_err(|e| RunnerError::Db(e.to_string()))
    }

    fn schedule_retry(
        connection: &mut Connection,
        run_id: &str,
        from_state: RunState,
        retry_count: i64,
        backoff_ms: i64,
        next_retry_at_ms: i64,
        reason: &str,
    ) -> Result<(), RunnerError> {
        Self::schedule_retry_internal(
            connection,
            run_id,
            from_state,
            retry_count,
            backoff_ms,
            next_retry_at_ms,
            reason,
            false,
        )
    }

    fn schedule_retry_internal(
        connection: &mut Connection,
        run_id: &str,
        from_state: RunState,
        retry_count: i64,
        backoff_ms: i64,
        next_retry_at_ms: i64,
        reason: &str,
        force_fail_before_activity: bool,
    ) -> Result<(), RunnerError> {
        let tx = connection
            .transaction()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let now = now_ms();

        tx.execute(
            "
            UPDATE runs
            SET state = 'retrying',
                retry_count = ?1,
                next_retry_backoff_ms = ?2,
                next_retry_at_ms = ?3,
                failure_reason = ?4,
                updated_at = ?5
            WHERE id = ?6
            ",
            params![
                retry_count,
                backoff_ms,
                next_retry_at_ms,
                reason,
                now,
                run_id
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        if force_fail_before_activity {
            return Err(RunnerError::ForcedTransitionFailure);
        }

        tx.execute(
            "
            INSERT INTO activities (id, run_id, activity_type, from_state, to_state, user_message, created_at)
            VALUES (?1, ?2, 'run_retry_scheduled', ?3, 'retrying', ?4, ?5)
            ",
            params![
                make_id("activity"),
                run_id,
                from_state.as_str(),
                format!("Retry scheduled in {} ms. {}", backoff_ms, redact_text(reason)),
                now
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.commit().map_err(|e| RunnerError::Db(e.to_string()))
    }

    pub fn transition_state_with_activity(
        connection: &mut Connection,
        run_id: &str,
        from_state: RunState,
        to_state: RunState,
        activity_type: &str,
        user_message: &str,
        failure_reason: Option<&str>,
        current_step_index: Option<i64>,
    ) -> Result<(), RunnerError> {
        Self::transition_state_with_activity_internal(
            connection,
            run_id,
            from_state,
            to_state,
            activity_type,
            user_message,
            failure_reason,
            current_step_index,
            false,
        )
    }

    fn transition_state_with_activity_internal(
        connection: &mut Connection,
        run_id: &str,
        from_state: RunState,
        to_state: RunState,
        activity_type: &str,
        user_message: &str,
        failure_reason: Option<&str>,
        current_step_index: Option<i64>,
        force_fail_before_activity: bool,
    ) -> Result<(), RunnerError> {
        let tx = connection
            .transaction()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let now = now_ms();

        tx.execute(
            "
            UPDATE runs
            SET state = ?1,
                failure_reason = ?2,
                current_step_index = COALESCE(?3, current_step_index),
                next_retry_backoff_ms = CASE WHEN ?1 != 'retrying' THEN NULL ELSE next_retry_backoff_ms END,
                next_retry_at_ms = CASE WHEN ?1 != 'retrying' THEN NULL ELSE next_retry_at_ms END,
                updated_at = ?4
            WHERE id = ?5
            ",
            params![
                to_state.as_str(),
                failure_reason,
                current_step_index,
                now,
                run_id
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        if force_fail_before_activity {
            return Err(RunnerError::ForcedTransitionFailure);
        }

        tx.execute(
            "
            INSERT INTO activities (id, run_id, activity_type, from_state, to_state, user_message, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            params![
                make_id("activity"),
                run_id,
                activity_type,
                from_state.as_str(),
                to_state.as_str(),
                redact_text(user_message),
                now
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        if to_state.is_terminal() {
            let run = Self::get_run_in_tx(&tx, run_id)?;
            Self::upsert_terminal_receipt_in_tx(&tx, &run, to_state, user_message, failure_reason)?;
        }

        tx.commit().map_err(|e| RunnerError::Db(e.to_string()))
    }

    fn upsert_terminal_receipt_in_tx(
        tx: &rusqlite::Transaction<'_>,
        run: &RunRecord,
        terminal_state: RunState,
        summary: &str,
        failure_reason: Option<&str>,
    ) -> Result<(), RunnerError> {
        let cost_breakdown = Self::cost_breakdown_for_run_in_tx(tx, &run.id)?;
        let receipt = build_receipt(run, terminal_state, summary, failure_reason, cost_breakdown);
        let receipt_json =
            serde_json::to_string(&receipt).map_err(|e| RunnerError::Serde(e.to_string()))?;
        let now = now_ms();

        tx.execute(
            "
            INSERT INTO outcomes (
              id, run_id, step_id, kind, status, content, failure_reason, created_at, updated_at
            ) VALUES (?1, ?2, 'terminal', 'receipt', 'final', ?3, ?4, ?5, ?5)
            ON CONFLICT(run_id, step_id, kind)
            DO UPDATE SET
              status = excluded.status,
              content = excluded.content,
              failure_reason = excluded.failure_reason,
              updated_at = excluded.updated_at
            ",
            params![
                make_id("outcome"),
                run.id,
                receipt_json,
                failure_reason.map(redact_text),
                now
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        Ok(())
    }

    fn cost_breakdown_for_run_in_tx(
        tx: &rusqlite::Transaction<'_>,
        run_id: &str,
    ) -> Result<Vec<ReceiptCostLineItem>, RunnerError> {
        let mut stmt = tx
            .prepare(
                "SELECT step_id, entry_kind, amount_usd_cents
                 FROM spend_ledger
                 WHERE run_id = ?1
                 ORDER BY created_at ASC",
            )
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(params![run_id], |row| {
                Ok(ReceiptCostLineItem {
                    step_id: row.get(0)?,
                    entry_kind: row.get(1)?,
                    amount_usd_cents: row.get(2)?,
                })
            })
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| RunnerError::Db(e.to_string()))?);
        }
        Ok(out)
    }

    fn run_learning_pipeline(
        connection: &mut Connection,
        run: &RunRecord,
    ) -> Result<(), RunnerError> {
        let evaluation = learning::evaluate_run(connection, &run.id)
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let adaptation =
            learning::adapt_autopilot(connection, &run.autopilot_id, &run.id, run.plan.recipe)
                .map_err(|e| RunnerError::Db(e.to_string()))?;
        learning::update_memory_cards(connection, &run.autopilot_id, &run.id, run.plan.recipe)
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let memory_titles = learning::list_memory_titles_for_run(connection, &run.id)
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        Self::enrich_terminal_receipt(connection, &run.id, evaluation, adaptation, memory_titles)?;
        Ok(())
    }

    fn enrich_terminal_receipt(
        connection: &Connection,
        run_id: &str,
        evaluation: RunEvaluationSummary,
        adaptation: AdaptationSummary,
        memory_titles: Vec<String>,
    ) -> Result<(), RunnerError> {
        let existing: Option<String> = connection
            .query_row(
                "SELECT content FROM outcomes WHERE run_id = ?1 AND kind = 'receipt' LIMIT 1",
                params![run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let Some(payload) = existing else {
            return Ok(());
        };

        let mut receipt: RunReceipt =
            serde_json::from_str(&payload).map_err(|e| RunnerError::Serde(e.to_string()))?;
        receipt.evaluation = Some(evaluation);
        receipt.adaptation = Some(adaptation);
        receipt.memory_titles_used = memory_titles;
        let updated =
            serde_json::to_string(&receipt).map_err(|e| RunnerError::Serde(e.to_string()))?;
        connection
            .execute(
                "
                UPDATE outcomes
                SET content = ?1, updated_at = ?2
                WHERE run_id = ?3 AND kind = 'receipt'
                ",
                params![updated, now_ms(), run_id],
            )
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        Ok(())
    }

    #[cfg(test)]
    fn transition_state_with_forced_failure(
        connection: &mut Connection,
        run_id: &str,
        from_state: RunState,
        to_state: RunState,
    ) -> Result<(), RunnerError> {
        Self::transition_state_with_activity_internal(
            connection,
            run_id,
            from_state,
            to_state,
            "forced_test",
            "forced failure",
            None,
            None,
            true,
        )
    }

    #[cfg(test)]
    fn schedule_retry_with_forced_failure(
        connection: &mut Connection,
        run_id: &str,
        from_state: RunState,
        retry_count: i64,
        backoff_ms: i64,
        next_retry_at_ms: i64,
    ) -> Result<(), RunnerError> {
        Self::schedule_retry_internal(
            connection,
            run_id,
            from_state,
            retry_count,
            backoff_ms,
            next_retry_at_ms,
            "forced retry failure",
            true,
        )
    }
}

fn map_provider_error(error: ProviderError) -> StepExecutionError {
    StepExecutionError {
        retryable: error.is_retryable(),
        user_reason: redact_text(&error.message),
    }
}

fn map_web_fetch_error(error: WebFetchError) -> StepExecutionError {
    StepExecutionError {
        retryable: error.is_retryable(),
        user_reason: error.to_string(),
    }
}

fn provider_kind_from_plan(plan: &AutopilotPlan) -> ProviderKind {
    match plan.provider.id {
        SchemaProviderId::OpenAi => ProviderKind::OpenAi,
        SchemaProviderId::Anthropic => ProviderKind::Anthropic,
        SchemaProviderId::Gemini => ProviderKind::Gemini,
    }
}

fn provider_tier_from_plan(plan: &AutopilotPlan) -> ProviderTier {
    match plan.provider.tier {
        SchemaProviderTier::Supported => ProviderTier::Supported,
        SchemaProviderTier::Experimental => ProviderTier::Experimental,
    }
}

fn parse_provider_kind(value: &str) -> Result<ProviderKind, RunnerError> {
    match value {
        "openai" => Ok(ProviderKind::OpenAi),
        "anthropic" => Ok(ProviderKind::Anthropic),
        "gemini" => Ok(ProviderKind::Gemini),
        _ => Err(RunnerError::InvalidProviderKind(value.to_string())),
    }
}

fn parse_provider_tier(value: &str) -> Result<ProviderTier, RunnerError> {
    match value {
        "supported" => Ok(ProviderTier::Supported),
        "experimental" => Ok(ProviderTier::Experimental),
        _ => Err(RunnerError::InvalidProviderTier(value.to_string())),
    }
}

fn build_receipt(
    run: &RunRecord,
    terminal_state: RunState,
    summary: &str,
    failure_reason: Option<&str>,
    cost_breakdown: Vec<ReceiptCostLineItem>,
) -> RunReceipt {
    let recovery_options = match terminal_state {
        RunState::Succeeded => {
            vec!["Review the outcome and keep this Autopilot running.".to_string()]
        }
        RunState::Failed => vec![
            "Retry now.".to_string(),
            "Reduce scope and run again.".to_string(),
            "Check Activity for the failed step.".to_string(),
        ],
        RunState::Blocked => vec![
            "Reduce scope to lower cost.".to_string(),
            "Adjust spend caps in Settings.".to_string(),
            "Approve spend if you still want to continue.".to_string(),
        ],
        RunState::Canceled => vec!["Resume later from the Autopilot detail view.".to_string()],
        _ => vec!["Review Activity for details.".to_string()],
    };

    RunReceipt {
        schema_version: "1.0".to_string(),
        run_id: run.id.clone(),
        autopilot_id: run.autopilot_id.clone(),
        provider_kind: run.provider_kind.as_str().to_string(),
        provider_tier: run.provider_tier.as_str().to_string(),
        terminal_state: terminal_state.as_str().to_string(),
        summary: redact_text(summary),
        failure_reason: failure_reason.map(redact_text),
        recovery_options,
        total_spend_usd_cents: run.usd_cents_actual,
        cost_breakdown,
        evaluation: None,
        adaptation: None,
        memory_titles_used: Vec::new(),
        redacted: true,
        created_at_ms: now_ms(),
    }
}

fn redact_text(input: &str) -> String {
    input
        .replace("sk-", "[REDACTED_KEY]-")
        .replace("api_key", "[REDACTED_FIELD]")
        .replace('@', "[at]")
}

fn format_usd_cents(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.abs();
    format!("{sign}${}.{:02}", abs / 100, abs % 100)
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect::<String>()
}

fn fnv1a_64_hex(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in input.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn parse_daily_summary_output(
    raw: &str,
    sources_hash: &str,
    max_bullets: usize,
) -> DailySummaryArtifact {
    let lines = raw
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<&str>>();
    let mut title = lines
        .iter()
        .find_map(|line| {
            line.strip_prefix("Title:")
                .map(|rest| rest.trim().to_string())
        })
        .unwrap_or_else(|| "Daily Brief".to_string());
    if title.is_empty() {
        title = "Daily Brief".to_string();
    }

    let mut bullet_points = lines
        .iter()
        .filter_map(|line| {
            if line.starts_with("- ") {
                Some(line.trim_start_matches("- ").trim().to_string())
            } else {
                None
            }
        })
        .filter(|b| !b.is_empty())
        .collect::<Vec<String>>();
    if bullet_points.is_empty() {
        bullet_points = lines
            .iter()
            .take(4)
            .map(|line| line.to_string())
            .collect::<Vec<String>>();
    }
    if bullet_points.len() > max_bullets.max(1) {
        bullet_points.truncate(max_bullets.max(1));
    }

    let summary_text = truncate_chars(&lines.join(" "), 4000);
    let content_hash = fnv1a_64_hex(&summary_text);
    DailySummaryArtifact {
        title,
        bullet_points,
        summary_text,
        sources_hash: sources_hash.to_string(),
        content_hash,
    }
}

fn compute_diff_score(previous: &str, current: &str) -> f64 {
    let prev = previous.trim();
    let curr = current.trim();
    if prev.is_empty() && curr.is_empty() {
        return 0.0;
    }
    if prev.is_empty() || curr.is_empty() {
        return 1.0;
    }

    let prev_chars = prev.chars().collect::<Vec<char>>();
    let curr_chars = curr.chars().collect::<Vec<char>>();
    let max_len = prev_chars.len().max(curr_chars.len()) as f64;
    if max_len == 0.0 {
        return 0.0;
    }

    let shared_prefix = prev_chars
        .iter()
        .zip(curr_chars.iter())
        .take_while(|(a, b)| a == b)
        .count() as f64;
    (1.0 - (shared_prefix / max_len)).clamp(0.0, 1.0)
}

fn compute_daily_sources_hash(results: &[DailySourceResult]) -> String {
    let material = results
        .iter()
        .map(|r| {
            format!(
                "{}|{}|{}|{}",
                r.source_id,
                r.url,
                r.text_excerpt,
                r.fetch_error.clone().unwrap_or_default()
            )
        })
        .collect::<Vec<String>>()
        .join("\n");
    fnv1a_64_hex(&material)
}

fn estimate_step_cost_usd_cents(run: &RunRecord, step: &PlanStep) -> i64 {
    if run.plan.intent.contains("simulate_cap_hard") {
        return 95;
    }
    if run.plan.intent.contains("simulate_cap_soft") {
        return 45;
    }
    if run.plan.intent.contains("simulate_cap_boundary") {
        return 80;
    }

    match step.primitive {
        PrimitiveId::AggregateDailySummary => 16,
        PrimitiveId::WriteOutcomeDraft => 12,
        PrimitiveId::WriteEmailDraft => 14,
        _ => 0,
    }
}

fn make_id(prefix: &str) -> String {
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}_{}_{}", prefix, now_ms(), counter)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn current_day_bucket() -> i64 {
    now_ms() / 86_400_000
}

fn compute_backoff_ms(retry_attempt: u32) -> u32 {
    let base: u32 = 200;
    base.saturating_mul(2u32.saturating_pow(retry_attempt.saturating_sub(1)))
        .min(2_000)
}

#[cfg(test)]
mod tests {
    use super::{RunReceipt, RunState, RunnerEngine};
    use crate::db::{bootstrap_schema, AutopilotProfileUpsert};
    use crate::learning;
    use crate::schema::{AutopilotPlan, PlanStep, PrimitiveId, ProviderId, RecipeKind, RiskTier};
    use rusqlite::{params, Connection};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn setup_conn() -> Connection {
        // Keep runner tests deterministic regardless of local shell environment.
        std::env::set_var("TERMINUS_TRANSPORT", "mock");
        let mut conn = Connection::open_in_memory().expect("open memory db");
        bootstrap_schema(&mut conn).expect("bootstrap schema");
        conn
    }

    fn plan_with_single_write_step(intent: &str) -> AutopilotPlan {
        AutopilotPlan {
            schema_version: "1.0".to_string(),
            recipe: RecipeKind::DailyBrief,
            intent: intent.to_string(),
            provider: crate::schema::ProviderMetadata::from_provider_id(ProviderId::OpenAi),
            web_source_url: None,
            web_allowed_domains: Vec::new(),
            inbox_source_text: None,
            daily_sources: Vec::new(),
            allowed_primitives: vec![PrimitiveId::WriteOutcomeDraft],
            steps: vec![PlanStep {
                id: "step_1".to_string(),
                label: "Write draft outcome".to_string(),
                primitive: PrimitiveId::WriteOutcomeDraft,
                requires_approval: false,
                risk_tier: RiskTier::Low,
            }],
        }
    }

    fn website_plan_with_url(url: &str) -> AutopilotPlan {
        AutopilotPlan::from_intent(
            RecipeKind::WebsiteMonitor,
            format!("Monitor this website for changes: {url}"),
            ProviderId::OpenAi,
        )
    }

    fn spawn_http_server(
        bodies: Vec<String>,
        content_type: &str,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("local addr");
        let url = format!("http://{addr}/monitor");
        let content_type = content_type.to_string();

        let handle = thread::spawn(move || {
            for body in bodies {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut buf = [0_u8; 2048];
                let _ = stream.read(&mut buf);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write response");
            }
        });

        (url, handle)
    }

    #[test]
    fn retries_only_retryable_provider_errors() {
        let mut conn = setup_conn();

        let retryable_plan = plan_with_single_write_step("simulate_provider_retryable_failure");
        let run_retryable =
            RunnerEngine::start_run(&mut conn, "auto_retryable", retryable_plan, "idem_r1", 1)
                .expect("start");
        let first = RunnerEngine::run_tick(&mut conn, &run_retryable.id).expect("tick");
        assert_eq!(first.state, RunState::Retrying);

        conn.execute(
            "UPDATE runs SET next_retry_at_ms = 0 WHERE id = ?1",
            params![run_retryable.id],
        )
        .expect("force due");
        let resumed = RunnerEngine::resume_due_runs(&mut conn, 10).expect("resume");
        assert_eq!(resumed[0].state, RunState::Succeeded);

        let non_retryable_plan =
            plan_with_single_write_step("simulate_provider_non_retryable_failure");
        let run_non_retry =
            RunnerEngine::start_run(&mut conn, "auto_nonretry", non_retryable_plan, "idem_r2", 1)
                .expect("start");
        let failed = RunnerEngine::run_tick(&mut conn, &run_non_retry.id).expect("tick");
        assert_eq!(failed.state, RunState::Failed);
        assert_eq!(failed.retry_count, 0);
    }

    #[test]
    fn spend_ledger_updates_once_per_step_even_after_retry_resume() {
        let mut conn = setup_conn();
        let plan = plan_with_single_write_step("simulate_provider_retryable_failure");
        let run =
            RunnerEngine::start_run(&mut conn, "auto_spend", plan, "idem_spend", 1).expect("start");
        let first = RunnerEngine::run_tick(&mut conn, &run.id).expect("tick");
        assert_eq!(first.state, RunState::Retrying);

        conn.execute(
            "UPDATE runs SET next_retry_at_ms = 0 WHERE id = ?1",
            params![run.id],
        )
        .expect("force due");
        let resumed = RunnerEngine::resume_due_runs(&mut conn, 10).expect("resume");
        assert_eq!(resumed[0].state, RunState::Succeeded);

        let spend_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM spend_ledger WHERE run_id = ?1 AND step_id = 'step_1' AND entry_kind = 'actual'",
                params![run.id],
                |row| row.get(0),
            )
            .expect("count spend rows");
        assert_eq!(spend_rows, 1);
    }

    #[test]
    fn hard_cap_blocks_before_side_effects() {
        let mut conn = setup_conn();
        let plan = plan_with_single_write_step("simulate_cap_hard");
        let run = RunnerEngine::start_run(&mut conn, "auto_hard", plan, "idem_hard", 1)
            .expect("run starts");

        let blocked = RunnerEngine::run_tick(&mut conn, &run.id).expect("run blocked");
        assert_eq!(blocked.state, RunState::Blocked);

        let draft_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM outcomes WHERE run_id = ?1 AND kind = 'outcome_draft'",
                params![run.id],
                |row| row.get(0),
            )
            .expect("count drafts");
        assert_eq!(draft_count, 0);
    }

    #[test]
    fn per_run_hard_cap_boundary_at_exactly_80_cents_is_not_blocked() {
        let mut conn = setup_conn();
        let plan = plan_with_single_write_step("simulate_cap_boundary");
        let run = RunnerEngine::start_run(&mut conn, "auto_boundary", plan, "idem_boundary", 1)
            .expect("start");

        let paused = RunnerEngine::run_tick(&mut conn, &run.id).expect("soft cap gate");
        assert_eq!(paused.state, RunState::NeedsApproval);

        let approvals = RunnerEngine::list_pending_approvals(&conn).expect("list approvals");
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].step_id, "__soft_cap__");
    }

    #[test]
    fn soft_cap_requires_approval_to_proceed() {
        let mut conn = setup_conn();
        let plan = plan_with_single_write_step("simulate_cap_soft");
        let run = RunnerEngine::start_run(&mut conn, "auto_soft", plan, "idem_soft", 1)
            .expect("run starts");

        let paused = RunnerEngine::run_tick(&mut conn, &run.id).expect("soft cap gate");
        assert_eq!(paused.state, RunState::NeedsApproval);

        let approvals = RunnerEngine::list_pending_approvals(&conn).expect("list approvals");
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].step_id, "__soft_cap__");

        let resumed = RunnerEngine::approve(&mut conn, &approvals[0].id).expect("approve spend");
        assert!(resumed.soft_cap_approved);
        assert_eq!(resumed.state, RunState::Succeeded);
    }

    #[test]
    fn transition_and_activity_are_atomic_in_single_transaction() {
        let mut conn = setup_conn();
        let plan = plan_with_single_write_step("atomicity test");
        let run = RunnerEngine::start_run(&mut conn, "auto_atomic", plan, "idem_atomic", 1)
            .expect("run created");

        RunnerEngine::transition_state_with_forced_failure(
            &mut conn,
            &run.id,
            RunState::Ready,
            RunState::Failed,
        )
        .expect_err("forced failure should abort transition");

        let post = RunnerEngine::get_run(&conn, &run.id).expect("run still readable");
        assert_eq!(post.state, RunState::Ready);
    }

    #[test]
    fn retry_metadata_and_activity_are_atomic() {
        let mut conn = setup_conn();
        let plan = plan_with_single_write_step("atomic retry test");
        let run =
            RunnerEngine::start_run(&mut conn, "auto_retry_atomic", plan, "idem_atomic_retry", 2)
                .expect("run created");

        RunnerEngine::schedule_retry_with_forced_failure(
            &mut conn,
            &run.id,
            RunState::Ready,
            1,
            200,
            500,
        )
        .expect_err("forced failure should rollback retry scheduling");

        let post = RunnerEngine::get_run(&conn, &run.id).expect("run still readable");
        assert_eq!(post.state, RunState::Ready);
        assert_eq!(post.retry_count, 0);
        assert!(post.next_retry_at_ms.is_none());
    }

    #[test]
    fn receipt_includes_provider_tier_and_cost_and_is_redacted() {
        let mut conn = setup_conn();
        let plan = plan_with_single_write_step("simulate_cap_hard sk-secret a@b.com");
        let run = RunnerEngine::start_run(&mut conn, "auto_receipt", plan, "idem_receipt", 1)
            .expect("run starts");

        let blocked = RunnerEngine::run_tick(&mut conn, &run.id).expect("blocked run");
        assert_eq!(blocked.state, RunState::Blocked);

        let receipt_json: String = conn
            .query_row(
                "SELECT content FROM outcomes WHERE run_id = ?1 AND kind = 'receipt'",
                params![run.id],
                |row| row.get(0),
            )
            .expect("receipt exists");
        let receipt: RunReceipt = serde_json::from_str(&receipt_json).expect("parse receipt");
        assert_eq!(receipt.provider_tier, "supported");
        assert!(receipt.total_spend_usd_cents >= 0);
        assert!(receipt.redacted);
    }

    #[test]
    fn website_monitor_happy_path_shared_runtime() {
        let mut conn = setup_conn();
        let (url, server) = spawn_http_server(
            vec![
                "<html><body><h1>Launch update</h1><p>new feature shipped</p></body></html>"
                    .to_string(),
            ],
            "text/html",
        );
        let plan = website_plan_with_url(&url);
        let run =
            RunnerEngine::start_run(&mut conn, "auto_web", plan, "idem_web", 2).expect("start");

        let s1 = RunnerEngine::run_tick(&mut conn, &run.id).expect("step 1");
        assert_eq!(s1.state, RunState::Ready);
        let snapshots: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM web_snapshots WHERE autopilot_id = 'auto_web'",
                [],
                |row| row.get(0),
            )
            .expect("snapshot exists");
        assert_eq!(snapshots, 1);

        let need_approval_1 = RunnerEngine::run_tick(&mut conn, &run.id).expect("approval1");
        assert_eq!(need_approval_1.state, RunState::NeedsApproval);

        let approvals = RunnerEngine::list_pending_approvals(&conn).expect("pending");
        let first = approvals
            .iter()
            .find(|a| a.run_id == run.id)
            .expect("first approval");
        let after_first = RunnerEngine::approve(&mut conn, &first.id).expect("approve first");
        assert_eq!(after_first.state, RunState::Ready);

        let need_approval_2 = RunnerEngine::run_tick(&mut conn, &run.id).expect("approval2");
        assert_eq!(need_approval_2.state, RunState::NeedsApproval);
        let approvals_2 = RunnerEngine::list_pending_approvals(&conn).expect("pending2");
        let second = approvals_2
            .iter()
            .find(|a| a.run_id == run.id)
            .expect("second approval");
        let done = RunnerEngine::approve(&mut conn, &second.id).expect("approve second");
        assert_eq!(done.state, RunState::Succeeded);
        server.join().expect("server join");
    }

    #[test]
    fn website_monitor_second_run_no_change_ends_without_draft() {
        let mut conn = setup_conn();
        let (url, server) = spawn_http_server(
            vec![
                "<html><body><p>same content</p></body></html>".to_string(),
                "<html><body><p>same content</p></body></html>".to_string(),
            ],
            "text/html",
        );
        let plan = website_plan_with_url(&url);

        let run1 = RunnerEngine::start_run(
            &mut conn,
            "auto_no_change",
            plan.clone(),
            "idem_nochange_1",
            2,
        )
        .expect("start1");
        let first = RunnerEngine::run_tick(&mut conn, &run1.id).expect("run1 step1");
        assert_eq!(first.state, RunState::Ready);

        let run2 = RunnerEngine::start_run(&mut conn, "auto_no_change", plan, "idem_nochange_2", 2)
            .expect("start2");
        let second = RunnerEngine::run_tick(&mut conn, &run2.id).expect("run2 step1");
        assert_eq!(second.state, RunState::Succeeded);

        let drafts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM outcomes WHERE run_id = ?1 AND kind = 'email_draft'",
                params![run2.id],
                |row| row.get(0),
            )
            .expect("count drafts");
        assert_eq!(drafts, 0);
        server.join().expect("server join");
    }

    #[test]
    fn website_monitor_change_triggers_summary_and_email_draft() {
        let mut conn = setup_conn();
        let (url, server) = spawn_http_server(
            vec![
                "<html><body><p>version one</p></body></html>".to_string(),
                "<html><body><p>version two changed</p></body></html>".to_string(),
            ],
            "text/html",
        );
        let plan = website_plan_with_url(&url);

        let run1 =
            RunnerEngine::start_run(&mut conn, "auto_change", plan.clone(), "idem_change_1", 2)
                .expect("start1");
        let _ = RunnerEngine::run_tick(&mut conn, &run1.id).expect("run1 step1");

        let run2 = RunnerEngine::start_run(&mut conn, "auto_change", plan, "idem_change_2", 2)
            .expect("start2");
        let s1 = RunnerEngine::run_tick(&mut conn, &run2.id).expect("run2 step1");
        assert_eq!(s1.state, RunState::Ready);

        let need_approval_1 = RunnerEngine::run_tick(&mut conn, &run2.id).expect("run2 approval1");
        assert_eq!(need_approval_1.state, RunState::NeedsApproval);
        let approvals = RunnerEngine::list_pending_approvals(&conn).expect("pending");
        let first = approvals
            .iter()
            .find(|a| a.run_id == run2.id)
            .expect("first approval");
        let after_first = RunnerEngine::approve(&mut conn, &first.id).expect("approve first");
        assert_eq!(after_first.state, RunState::Ready);

        let need_approval_2 = RunnerEngine::run_tick(&mut conn, &run2.id).expect("run2 approval2");
        assert_eq!(need_approval_2.state, RunState::NeedsApproval);
        let approvals_2 = RunnerEngine::list_pending_approvals(&conn).expect("pending2");
        let second = approvals_2
            .iter()
            .find(|a| a.run_id == run2.id)
            .expect("second approval");
        let done = RunnerEngine::approve(&mut conn, &second.id).expect("approve second");
        assert_eq!(done.state, RunState::Succeeded);

        let email_drafts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM outcomes WHERE run_id = ?1 AND kind = 'email_draft'",
                params![run2.id],
                |row| row.get(0),
            )
            .expect("count email drafts");
        assert_eq!(email_drafts, 1);
        server.join().expect("server join");
    }

    #[test]
    fn read_web_blocks_disallowed_host_with_human_message() {
        let mut conn = setup_conn();
        let url = "http://127.0.0.1:65530/blocked";
        let mut plan = website_plan_with_url(&url);
        plan.web_allowed_domains = vec!["example.com".to_string()];
        let run = RunnerEngine::start_run(&mut conn, "auto_block", plan, "idem_block_host", 2)
            .expect("start");

        let failed = RunnerEngine::run_tick(&mut conn, &run.id).expect("tick");
        assert_eq!(failed.state, RunState::Failed);
        let reason = failed.failure_reason.expect("reason");
        assert!(reason.contains("allowlist"));
    }

    #[test]
    fn read_web_large_response_fails_safely() {
        let mut conn = setup_conn();
        let huge = "A".repeat(260_000);
        let (url, server) = spawn_http_server(vec![huge], "text/plain");
        let plan = website_plan_with_url(&url);
        let run = RunnerEngine::start_run(&mut conn, "auto_large", plan, "idem_large_content", 2)
            .expect("start");

        let failed = RunnerEngine::run_tick(&mut conn, &run.id).expect("tick");
        assert_eq!(failed.state, RunState::Failed);
        let reason = failed.failure_reason.expect("reason");
        assert!(reason.contains("too large") || reason.contains("Reduce scope"));
        server.join().expect("server join");
    }

    #[test]
    fn inbox_triage_happy_path_shared_runtime() {
        let mut conn = setup_conn();
        let mut plan = AutopilotPlan::from_intent(
            RecipeKind::InboxTriage,
            "Inbox triage happy path".to_string(),
            ProviderId::Anthropic,
        );
        plan.inbox_source_text = Some(
            "Subject: Vendor follow-up\nCan you confirm timeline for contract signature?"
                .to_string(),
        );
        let run =
            RunnerEngine::start_run(&mut conn, "auto_inbox", plan, "idem_inbox", 2).expect("start");

        let s1 = RunnerEngine::run_tick(&mut conn, &run.id).expect("step 1");
        assert_eq!(s1.state, RunState::Ready);

        let s2 = RunnerEngine::run_tick(&mut conn, &run.id).expect("step 2");
        assert_eq!(s2.state, RunState::Ready);

        let need_approval_2 = RunnerEngine::run_tick(&mut conn, &run.id).expect("approval");
        assert_eq!(need_approval_2.state, RunState::NeedsApproval);
        let approvals_2 = RunnerEngine::list_pending_approvals(&conn).expect("pending2");
        let second = approvals_2
            .iter()
            .find(|a| a.run_id == run.id)
            .expect("approval");
        let done = RunnerEngine::approve(&mut conn, &second.id).expect("approve");
        assert_eq!(done.state, RunState::Succeeded);
    }

    #[test]
    fn inbox_triage_dedupes_identical_pasted_content() {
        let mut conn = setup_conn();
        let pasted = "Subject: Question\nCould you review this deck before Friday?".to_string();

        let mut plan_first = AutopilotPlan::from_intent(
            RecipeKind::InboxTriage,
            "Inbox triage first".to_string(),
            ProviderId::OpenAi,
        );
        plan_first.inbox_source_text = Some(pasted.clone());
        let run1 = RunnerEngine::start_run(
            &mut conn,
            "auto_inbox_dedupe",
            plan_first,
            "idem_inbox_dedupe_1",
            2,
        )
        .expect("start1");
        let _ = RunnerEngine::run_tick(&mut conn, &run1.id).expect("run1 step1");
        let _ = RunnerEngine::run_tick(&mut conn, &run1.id).expect("run1 step2");
        let need_approval = RunnerEngine::run_tick(&mut conn, &run1.id).expect("run1 approval");
        assert_eq!(need_approval.state, RunState::NeedsApproval);
        let approvals = RunnerEngine::list_pending_approvals(&conn).expect("pending");
        let approval = approvals
            .iter()
            .find(|a| a.run_id == run1.id)
            .expect("approval");
        let done = RunnerEngine::approve(&mut conn, &approval.id).expect("approve");
        assert_eq!(done.state, RunState::Succeeded);

        let mut plan_second = AutopilotPlan::from_intent(
            RecipeKind::InboxTriage,
            "Inbox triage second".to_string(),
            ProviderId::OpenAi,
        );
        plan_second.inbox_source_text = Some(pasted);
        let run2 = RunnerEngine::start_run(
            &mut conn,
            "auto_inbox_dedupe",
            plan_second,
            "idem_inbox_dedupe_2",
            2,
        )
        .expect("start2");
        let second_tick = RunnerEngine::run_tick(&mut conn, &run2.id).expect("run2 step1");
        assert_eq!(second_tick.state, RunState::Succeeded);

        let draft_count_run2: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM outcomes WHERE run_id = ?1 AND kind = 'email_draft'",
                params![run2.id],
                |row| row.get(0),
            )
            .expect("count drafts run2");
        assert_eq!(draft_count_run2, 0);

        let inbox_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM inbox_items WHERE autopilot_id = 'auto_inbox_dedupe'",
                [],
                |row| row.get(0),
            )
            .expect("count inbox rows");
        assert_eq!(inbox_rows, 1);
    }

    #[test]
    fn inbox_triage_size_limit_is_enforced() {
        let mut conn = setup_conn();
        let mut plan = AutopilotPlan::from_intent(
            RecipeKind::InboxTriage,
            "Inbox triage large".to_string(),
            ProviderId::OpenAi,
        );
        plan.inbox_source_text = Some("X".repeat(25_000));
        let run =
            RunnerEngine::start_run(&mut conn, "auto_inbox_large", plan, "idem_inbox_large", 2)
                .expect("start");
        let failed = RunnerEngine::run_tick(&mut conn, &run.id).expect("tick");
        assert_eq!(failed.state, RunState::Failed);
        let reason = failed.failure_reason.expect("reason");
        assert!(reason.contains("too large"));
    }

    #[test]
    fn inbox_triage_denies_read_when_primitive_not_allowlisted() {
        let mut conn = setup_conn();
        let plan = AutopilotPlan {
            schema_version: "1.0".to_string(),
            recipe: RecipeKind::InboxTriage,
            intent: "Inbox triage deny test".to_string(),
            provider: crate::schema::ProviderMetadata::from_provider_id(ProviderId::OpenAi),
            web_source_url: None,
            web_allowed_domains: Vec::new(),
            inbox_source_text: Some("Subject: hi\nCan we meet tomorrow?".to_string()),
            daily_sources: Vec::new(),
            allowed_primitives: vec![PrimitiveId::WriteOutcomeDraft, PrimitiveId::WriteEmailDraft],
            steps: vec![PlanStep {
                id: "step_1".to_string(),
                label: "Read forwarded email".to_string(),
                primitive: PrimitiveId::ReadForwardedEmail,
                requires_approval: false,
                risk_tier: RiskTier::Low,
            }],
        };
        let run = RunnerEngine::start_run(&mut conn, "auto_inbox_deny", plan, "idem_inbox_deny", 1)
            .expect("start");
        let failed = RunnerEngine::run_tick(&mut conn, &run.id).expect("tick");
        assert_eq!(failed.state, RunState::Failed);
        let reason = failed.failure_reason.expect("reason");
        assert_eq!(reason, "This action isn't allowed in Terminus yet.");
    }

    #[test]
    fn daily_brief_happy_path_shared_runtime() {
        let mut conn = setup_conn();
        let (url, server) = spawn_http_server(
            vec!["<html><body><p>daily source content</p></body></html>".to_string()],
            "text/html",
        );
        let mut plan = AutopilotPlan::from_intent(
            RecipeKind::DailyBrief,
            "Daily brief happy path".to_string(),
            ProviderId::Gemini,
        );
        plan.daily_sources = vec![url];
        let run =
            RunnerEngine::start_run(&mut conn, "auto_brief", plan, "idem_brief", 2).expect("start");

        let s1 = RunnerEngine::run_tick(&mut conn, &run.id).expect("step 1");
        assert_eq!(s1.state, RunState::Ready);

        let s2 = RunnerEngine::run_tick(&mut conn, &run.id).expect("step 2");
        assert_eq!(s2.state, RunState::Ready);

        let need_approval = RunnerEngine::run_tick(&mut conn, &run.id).expect("approval");
        assert_eq!(need_approval.state, RunState::NeedsApproval);

        let approvals = RunnerEngine::list_pending_approvals(&conn).expect("pending");
        let first = approvals
            .iter()
            .find(|a| a.run_id == run.id)
            .expect("approval exists");
        let done = RunnerEngine::approve(&mut conn, &first.id).expect("approve");
        assert_eq!(done.state, RunState::Succeeded);
        server.join().expect("server join");
    }

    #[test]
    fn daily_brief_same_sources_same_content_dedupes_draft_creation() {
        let mut conn = setup_conn();
        let (url, server) = spawn_http_server(
            vec![
                "<html><body><p>same daily content</p></body></html>".to_string(),
                "<html><body><p>same daily content</p></body></html>".to_string(),
            ],
            "text/html",
        );

        let mut plan1 = AutopilotPlan::from_intent(
            RecipeKind::DailyBrief,
            "Daily brief dedupe one".to_string(),
            ProviderId::OpenAi,
        );
        plan1.daily_sources = vec![url.clone()];
        let run1 = RunnerEngine::start_run(
            &mut conn,
            "auto_brief_dedupe",
            plan1,
            "idem_brief_dedupe_1",
            2,
        )
        .expect("start1");
        let _ = RunnerEngine::run_tick(&mut conn, &run1.id).expect("run1 s1");
        let _ = RunnerEngine::run_tick(&mut conn, &run1.id).expect("run1 s2");
        let approval = RunnerEngine::run_tick(&mut conn, &run1.id).expect("run1 approval");
        assert_eq!(approval.state, RunState::NeedsApproval);
        let approvals = RunnerEngine::list_pending_approvals(&conn).expect("pending");
        let first = approvals
            .iter()
            .find(|a| a.run_id == run1.id)
            .expect("approval");
        let done = RunnerEngine::approve(&mut conn, &first.id).expect("approve");
        assert_eq!(done.state, RunState::Succeeded);

        let mut plan2 = AutopilotPlan::from_intent(
            RecipeKind::DailyBrief,
            "Daily brief dedupe two".to_string(),
            ProviderId::OpenAi,
        );
        plan2.daily_sources = vec![url];
        let run2 = RunnerEngine::start_run(
            &mut conn,
            "auto_brief_dedupe",
            plan2,
            "idem_brief_dedupe_2",
            2,
        )
        .expect("start2");
        let _ = RunnerEngine::run_tick(&mut conn, &run2.id).expect("run2 s1");
        let r2s2 = RunnerEngine::run_tick(&mut conn, &run2.id).expect("run2 s2");
        assert_eq!(r2s2.state, RunState::Succeeded);

        let run2_drafts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM outcomes WHERE run_id = ?1 AND kind = 'outcome_draft'",
                params![run2.id],
                |row| row.get(0),
            )
            .expect("count drafts run2");
        assert_eq!(run2_drafts, 0);
        server.join().expect("server join");
    }

    #[test]
    fn daily_brief_source_list_change_triggers_new_summary() {
        let mut conn = setup_conn();
        let (url, server) = spawn_http_server(
            vec![
                "<html><body><p>market update one</p></body></html>".to_string(),
                "<html><body><p>market update one</p></body></html>".to_string(),
            ],
            "text/html",
        );

        let mut plan1 = AutopilotPlan::from_intent(
            RecipeKind::DailyBrief,
            "Daily brief source change one".to_string(),
            ProviderId::OpenAi,
        );
        plan1.daily_sources = vec![url.clone()];
        let run1 = RunnerEngine::start_run(
            &mut conn,
            "auto_brief_change",
            plan1,
            "idem_brief_change_1",
            2,
        )
        .expect("start1");
        let _ = RunnerEngine::run_tick(&mut conn, &run1.id).expect("run1 s1");
        let _ = RunnerEngine::run_tick(&mut conn, &run1.id).expect("run1 s2");
        let approval = RunnerEngine::run_tick(&mut conn, &run1.id).expect("run1 approval");
        assert_eq!(approval.state, RunState::NeedsApproval);
        let approvals = RunnerEngine::list_pending_approvals(&conn).expect("pending");
        let first = approvals
            .iter()
            .find(|a| a.run_id == run1.id)
            .expect("approval");
        let _ = RunnerEngine::approve(&mut conn, &first.id).expect("approve");

        let mut plan2 = AutopilotPlan::from_intent(
            RecipeKind::DailyBrief,
            "Daily brief source change two".to_string(),
            ProviderId::OpenAi,
        );
        plan2.daily_sources = vec![url, "Inline: include founder note".to_string()];
        let run2 = RunnerEngine::start_run(
            &mut conn,
            "auto_brief_change",
            plan2,
            "idem_brief_change_2",
            2,
        )
        .expect("start2");
        let _ = RunnerEngine::run_tick(&mut conn, &run2.id).expect("run2 s1");
        let r2s2 = RunnerEngine::run_tick(&mut conn, &run2.id).expect("run2 s2");
        assert_eq!(r2s2.state, RunState::Ready);
        let approval2 = RunnerEngine::run_tick(&mut conn, &run2.id).expect("run2 approval");
        assert_eq!(approval2.state, RunState::NeedsApproval);
        server.join().expect("server join");
    }

    #[test]
    fn daily_brief_handles_partial_fetch_failures_gracefully() {
        let mut conn = setup_conn();
        let (url, server) = spawn_http_server(
            vec!["<html><body><p>reliable source</p></body></html>".to_string()],
            "text/html",
        );
        let mut plan = AutopilotPlan::from_intent(
            RecipeKind::DailyBrief,
            "Daily brief graceful source errors".to_string(),
            ProviderId::OpenAi,
        );
        plan.daily_sources = vec![url, "http://127.0.0.1:65530/unreachable".to_string()];
        let run = RunnerEngine::start_run(
            &mut conn,
            "auto_brief_partial",
            plan,
            "idem_brief_partial",
            2,
        )
        .expect("start");
        let s1 = RunnerEngine::run_tick(&mut conn, &run.id).expect("s1");
        assert_eq!(s1.state, RunState::Ready);
        let s2 = RunnerEngine::run_tick(&mut conn, &run.id).expect("s2");
        assert_eq!(s2.state, RunState::Ready);
        let approval = RunnerEngine::run_tick(&mut conn, &run.id).expect("approval");
        assert_eq!(approval.state, RunState::NeedsApproval);
        server.join().expect("server join");
    }

    #[test]
    fn daily_brief_retry_does_not_double_charge_source_usage() {
        let mut conn = setup_conn();
        let (url, server) = spawn_http_server(
            vec!["<html><body><p>retry source pass one</p></body></html>".to_string()],
            "text/html",
        );
        let mut plan = AutopilotPlan::from_intent(
            RecipeKind::DailyBrief,
            "simulate_provider_retryable_failure".to_string(),
            ProviderId::OpenAi,
        );
        plan.daily_sources = vec![url];
        let run =
            RunnerEngine::start_run(&mut conn, "auto_brief_retry", plan, "idem_brief_retry", 2)
                .expect("start");
        let _ = RunnerEngine::run_tick(&mut conn, &run.id).expect("s1");
        let retrying = RunnerEngine::run_tick(&mut conn, &run.id).expect("s2 retry");
        assert_eq!(retrying.state, RunState::Retrying);

        conn.execute(
            "UPDATE runs SET next_retry_at_ms = 0 WHERE id = ?1",
            params![run.id],
        )
        .expect("force due");
        let resumed = RunnerEngine::resume_due_runs(&mut conn, 10).expect("resume");
        assert_eq!(resumed[0].state, RunState::Ready);

        let spend_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM spend_ledger WHERE run_id = ?1 AND entry_kind = 'source_usage'",
                params![run.id],
                |row| row.get(0),
            )
            .expect("count source usage rows");
        assert_eq!(spend_rows, 1);
        server.join().expect("server join");
    }

    #[test]
    fn approval_decisions_emit_decision_events() {
        let mut conn = setup_conn();
        let mut plan = plan_with_single_write_step("approval event capture");
        plan.steps[0].requires_approval = true;
        let run = RunnerEngine::start_run(&mut conn, "auto_decisions", plan, "idem_decisions_1", 2)
            .expect("start");
        let needs = RunnerEngine::run_tick(&mut conn, &run.id).expect("approval needed");
        assert_eq!(needs.state, RunState::NeedsApproval);

        let approvals = RunnerEngine::list_pending_approvals(&conn).expect("approvals");
        let approval = approvals
            .iter()
            .find(|a| a.run_id == run.id)
            .expect("approval");
        let _done = RunnerEngine::approve(&mut conn, &approval.id).expect("approve");

        let approved_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM decision_events WHERE run_id = ?1 AND event_type = 'approval_approved'",
                params![run.id],
                |row| row.get(0),
            )
            .expect("count approved decision events");
        assert_eq!(approved_count, 1);

        let mut plan2 = plan_with_single_write_step("approval reject capture");
        plan2.steps[0].requires_approval = true;
        let run2 =
            RunnerEngine::start_run(&mut conn, "auto_decisions", plan2, "idem_decisions_2", 2)
                .expect("start2");
        let needs2 = RunnerEngine::run_tick(&mut conn, &run2.id).expect("approval needed");
        assert_eq!(needs2.state, RunState::NeedsApproval);
        let approvals2 = RunnerEngine::list_pending_approvals(&conn).expect("approvals2");
        let approval2 = approvals2
            .iter()
            .find(|a| a.run_id == run2.id)
            .expect("approval2");
        let _ = RunnerEngine::reject(
            &mut conn,
            &approval2.id,
            Some("Not needed right now".to_string()),
        )
        .expect("reject");
        let rejected_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM decision_events WHERE run_id = ?1 AND event_type = 'approval_rejected'",
                params![run2.id],
                |row| row.get(0),
            )
            .expect("count rejected decision events");
        assert_eq!(rejected_count, 1);
    }

    #[test]
    fn terminal_receipt_includes_evaluation_once_and_no_raw_inputs() {
        let mut conn = setup_conn();
        let run = RunnerEngine::start_run(
            &mut conn,
            "auto_eval_receipt",
            plan_with_single_write_step("sensitive phrase: customer-pii-123"),
            "idem_eval_receipt",
            2,
        )
        .expect("start");
        let done = RunnerEngine::run_tick(&mut conn, &run.id).expect("tick");
        assert_eq!(done.state, RunState::Succeeded);

        let eval_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM run_evaluations WHERE run_id = ?1",
                params![run.id],
                |row| row.get(0),
            )
            .expect("count run evaluations");
        assert_eq!(eval_count, 1);

        let _ = learning::evaluate_run(&conn, &run.id).expect("evaluate twice");
        let eval_count_after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM run_evaluations WHERE run_id = ?1",
                params![run.id],
                |row| row.get(0),
            )
            .expect("count run evaluations after");
        assert_eq!(eval_count_after, 1);

        let receipt = RunnerEngine::get_terminal_receipt(&conn, &run.id)
            .expect("get receipt")
            .expect("receipt exists");
        assert!(receipt.evaluation.is_some());

        let signals_json: String = conn
            .query_row(
                "SELECT signals_json FROM run_evaluations WHERE run_id = ?1",
                params![run.id],
                |row| row.get(0),
            )
            .expect("signals");
        assert!(!signals_json.contains("customer-pii-123"));
    }

    #[test]
    fn suppression_profile_stops_run_early_without_side_effects() {
        let mut conn = setup_conn();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("now")
            .as_millis() as i64;
        conn.execute(
            "INSERT OR IGNORE INTO autopilots (id, name, created_at) VALUES (?1, 'Autopilot', ?2)",
            params!["auto_suppressed", now_ms],
        )
        .expect("insert autopilot");
        crate::db::upsert_autopilot_profile(
            &conn,
            &AutopilotProfileUpsert {
                autopilot_id: "auto_suppressed".to_string(),
                learning_enabled: true,
                mode: "balanced".to_string(),
                knobs_json: "{\"min_diff_score_to_notify\":0.2,\"max_sources\":5,\"max_bullets\":6,\"reply_length_hint\":\"medium\"}".to_string(),
                suppression_json: format!(
                    "{{\"suppress_until_ms\":{},\"quiet_until_ms\":null}}",
                    now_ms + 60_000
                ),
                updated_at_ms: now_ms,
                version: 1,
            },
        )
        .expect("upsert profile");

        let run = RunnerEngine::start_run(
            &mut conn,
            "auto_suppressed",
            plan_with_single_write_step("suppressed should skip"),
            "idem_suppressed",
            2,
        )
        .expect("start");
        let done = RunnerEngine::run_tick(&mut conn, &run.id).expect("tick");
        assert_eq!(done.state, RunState::Succeeded);

        let approval_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM approvals WHERE run_id = ?1",
                params![run.id],
                |row| row.get(0),
            )
            .expect("approvals");
        assert_eq!(approval_count, 0);

        let draft_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM outcomes WHERE run_id = ?1 AND kind IN ('outcome_draft','email_draft')",
                params![run.id],
                |row| row.get(0),
            )
            .expect("draft count");
        assert_eq!(draft_count, 0);

        let receipt = RunnerEngine::get_terminal_receipt(&conn, &run.id)
            .expect("receipt")
            .expect("exists");
        assert!(receipt.summary.to_ascii_lowercase().contains("suppressed"));
    }

    #[test]
    fn memory_titles_are_persisted_and_exposed_in_receipt() {
        let mut conn = setup_conn();
        let run1 = RunnerEngine::start_run(
            &mut conn,
            "auto_memory",
            plan_with_single_write_step("first run"),
            "idem_memory_1",
            2,
        )
        .expect("start1");
        let done1 = RunnerEngine::run_tick(&mut conn, &run1.id).expect("tick1");
        assert_eq!(done1.state, RunState::Succeeded);

        learning::record_decision_event(
            &conn,
            "auto_memory",
            &run1.id,
            Some("step_1"),
            learning::DecisionEventType::DraftEdited,
            learning::DecisionEventMetadata {
                draft_length: Some(500),
                ..Default::default()
            },
        )
        .expect("event1");
        learning::record_decision_event(
            &conn,
            "auto_memory",
            &run1.id,
            Some("step_1"),
            learning::DecisionEventType::DraftEdited,
            learning::DecisionEventMetadata {
                draft_length: Some(530),
                ..Default::default()
            },
        )
        .expect("event2");
        learning::update_memory_cards(&conn, "auto_memory", &run1.id, RecipeKind::InboxTriage)
            .expect("update cards");

        let run2 = RunnerEngine::start_run(
            &mut conn,
            "auto_memory",
            plan_with_single_write_step("second run"),
            "idem_memory_2",
            2,
        )
        .expect("start2");
        let done2 = RunnerEngine::run_tick(&mut conn, &run2.id).expect("tick2");
        assert_eq!(done2.state, RunState::Succeeded);

        let receipt = RunnerEngine::get_terminal_receipt(&conn, &run2.id)
            .expect("receipt")
            .expect("exists");
        assert!(!receipt.memory_titles_used.is_empty());
        assert!(receipt
            .memory_titles_used
            .iter()
            .all(|title| !title.to_ascii_lowercase().contains("subject:")));
    }
}
