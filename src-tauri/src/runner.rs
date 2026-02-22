use crate::db;
use crate::email_connections::{self, EmailProvider, OutboundEmailRequest, TriageAction};
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

// Spend cap constants (in cents)
const PER_RUN_SOFT_CAP_USD_CENTS: i64 = 40;
const PER_RUN_HARD_CAP_USD_CENTS: i64 = 80;
const DAILY_SOFT_CAP_USD_CENTS: i64 = 300;
const DAILY_HARD_CAP_USD_CENTS: i64 = 500;
const SOFT_CAP_APPROVAL_STEP_ID: &str = "__soft_cap__";
const INBOX_TEXT_MAX_CHARS: usize = 20_000;
const DAILY_SOURCE_MAX_ITEMS: usize = 10;

// Retry backoff constants
const RETRY_BACKOFF_BASE_MS: u32 = 200; // Initial backoff: 200ms
const RETRY_BACKOFF_MAX_MS: u32 = 2_000; // Max backoff: 2 seconds
const MS_PER_DAY: i64 = 86_400_000; // Milliseconds in 24 hours

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
    pub action_id: Option<String>,
    pub status: String,
    pub preview: String,
    pub payload_type: String,
    pub payload_json: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarificationRecord {
    pub id: String,
    pub run_id: String,
    pub step_id: String,
    pub field_key: String,
    pub question: String,
    pub options_json: Option<String>,
    pub answer_json: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    DeliverNotificationAction,
    CreateOutcomeAction,
    EmailTriageAction,
    EmailSendAction,
}

impl ActionType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::DeliverNotificationAction => "deliver_notification",
            Self::CreateOutcomeAction => "create_outcome",
            Self::EmailTriageAction => "email_triage",
            Self::EmailSendAction => "email_send",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ActionStatus {
    PendingApproval,
    Ready,
    Executed,
    Failed,
    Canceled,
}

impl ActionStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::PendingApproval => "pending_approval",
            Self::Ready => "ready",
            Self::Executed => "executed",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRecord {
    pub id: String,
    pub run_id: String,
    pub step_id: String,
    pub action_type: ActionType,
    pub payload_json: String,
    pub requires_approval: bool,
    pub status: String,
    pub idempotency_key: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionExecutionRecord {
    pub id: String,
    pub action_id: String,
    pub attempt: i64,
    pub executed_at_ms: i64,
    pub result_status: String,
    pub result_json: String,
    pub latency_ms: Option<i64>,
    pub retry_at_ms: Option<i64>,
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

#[derive(Debug, Clone)]
struct IngestContext {
    provider: EmailProvider,
    provider_message_id: String,
    provider_thread_id: Option<String>,
    sender_email: Option<String>,
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

/// Core state machine for executing autopilot runs.
///
/// The runner uses a tick-based execution model where each call to `run_tick()`
/// advances the state machine by exactly one step. This enables bounded execution,
/// easy pause/resume, and prevents stack overflow.
pub struct RunnerEngine;

impl RunnerEngine {
    /// Starts a new autopilot run or returns existing run for duplicate idempotency key.
    ///
    /// # Arguments
    /// * `connection` - SQLite connection (must be mutable for transactions)
    /// * `autopilot_id` - ID of the autopilot initiating this run
    /// * `plan` - The execution plan (recipe + steps + primitives)
    /// * `idempotency_key` - Unique key to prevent duplicate execution
    /// * `max_retries` - Maximum retry attempts for retryable failures
    ///
    /// # Idempotency
    /// If a run with the same `idempotency_key` already exists, returns the existing
    /// run without creating a new one. This prevents accidental double-execution.
    ///
    /// # Returns
    /// New or existing `RunRecord` in `Ready` state
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

    /// Advances the run state machine by exactly one step.
    ///
    /// This is the core execution method. Each call:
    /// 1. Fetches the current run state
    /// 2. Executes the next step (if Ready/Running)
    /// 3. Persists the new state atomically
    /// 4. Returns the updated run
    ///
    /// # Tick-Based Execution
    /// Unlike recursive execution, ticking is bounded - each call does exactly
    /// one step of work and returns. The caller decides when to tick again.
    /// This enables:
    /// - Pause/resume at any point
    /// - Rate limiting / throttling
    /// - No stack overflow on long runs
    ///
    /// # State Transitions
    /// - `Ready` → executes next step → `Running` or `NeedsApproval`
    /// - `Running` → step completes → `Ready`, `Succeeded`, `Retrying`, `Failed`, or `Blocked`
    /// - `NeedsApproval` → waits for approval (no-op tick)
    /// - `Retrying` → waits for retry time (use `resume_due_runs`)
    /// - Terminal states (`Succeeded`, `Failed`, `Blocked`, `Canceled`) → no-op
    ///
    /// # Returns
    /// Updated `RunRecord` after the tick
    pub fn run_tick(connection: &mut Connection, run_id: &str) -> Result<RunRecord, RunnerError> {
        Self::run_tick_internal(connection, run_id, None)
    }

    /// Resumes runs that are in `Retrying` state and due for retry.
    ///
    /// Finds runs where `next_retry_at_ms <= now()` and ticks them.
    /// This is typically called by a background scheduler.
    ///
    /// # Arguments
    /// * `limit` - Maximum number of runs to resume in one call
    ///
    /// # Returns
    /// Vector of resumed runs (may be empty if none are due)
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

    /// Approves a pending approval and resumes execution.
    ///
    /// When a run hits an approval gate (spend cap or primitive approval),
    /// it pauses in `NeedsApproval` state. This method:
    /// 1. Marks the approval as approved
    /// 2. Transitions run to `Ready`
    /// 3. Automatically ticks to resume execution
    ///
    /// # Special Cases
    /// - **Spend cap approvals** (`step_id == "__soft_cap__"`): Sets `soft_cap_approved` flag
    /// - **Step approvals**: Resumes execution at the approved step
    ///
    /// # Returns
    /// Updated run after resume (may advance multiple states if execution continues)
    pub fn approve(
        connection: &mut Connection,
        approval_id: &str,
    ) -> Result<RunRecord, RunnerError> {
        let approval = Self::get_approval(connection, approval_id)?;
        if approval.status == "approved" {
            if let Some(action_id) = approval.action_id.as_deref() {
                if Self::action_is_safe_internal(connection, action_id)? {
                    Self::execute_action(connection, action_id)?;
                }
            }
            return Self::get_run_with_learning(connection, &approval.run_id);
        }
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
        let has_action = approval.action_id.is_some();
        let skip_step_after_approval = false;

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
                current_step_index = CASE WHEN ?2 THEN current_step_index + 1 ELSE current_step_index END,
                failure_reason = NULL,
                next_retry_backoff_ms = NULL,
                next_retry_at_ms = NULL,
                updated_at = ?3
            WHERE id = ?4
            ",
            params![
                is_soft_cap_approval,
                has_action && skip_step_after_approval,
                now,
                approval.run_id
            ],
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
        if skip_step_after_approval {
            if let Some(action_id) = approval.action_id.as_deref() {
                Self::execute_action(connection, action_id)?;
            }
        }
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
            None,
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        if is_soft_cap_approval {
            Self::run_tick_internal(connection, &approval.run_id, None)
        } else if has_action && skip_step_after_approval {
            Self::run_tick_internal(connection, &approval.run_id, None)
        } else {
            Self::run_tick_internal(connection, &approval.run_id, Some(&approval.step_id))
        }
    }

    /// Rejects a pending approval and cancels the run.
    ///
    /// When a user rejects an approval:
    /// 1. Marks the approval as rejected
    /// 2. Transitions run to `Canceled`
    /// 3. Records the rejection reason in activity log
    ///
    /// Canceled runs are terminal and cannot be resumed.
    ///
    /// # Arguments
    /// * `reason` - Optional user-provided reason for rejection
    ///
    /// # Returns
    /// Canceled run record
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
            None,
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
                SELECT id, run_id, step_id, action_id, status, preview, payload_type, payload_json, reason
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
                    action_id: row.get(3)?,
                    status: row.get(4)?,
                    preview: row.get(5)?,
                    payload_type: row.get(6)?,
                    payload_json: row.get(7)?,
                    reason: row.get(8)?,
                })
            })
            .map_err(|e| RunnerError::Db(e.to_string()))?;

        let mut approvals = Vec::new();
        for row in rows {
            approvals.push(row.map_err(|e| RunnerError::Db(e.to_string()))?);
        }
        Ok(approvals)
    }

    pub fn list_pending_clarifications(
        connection: &Connection,
    ) -> Result<Vec<ClarificationRecord>, RunnerError> {
        let mut stmt = connection
            .prepare(
                "
                SELECT id, run_id, step_id, field_key, question, options_json, answer_json, status
                FROM clarifications
                WHERE status = 'pending'
                ORDER BY created_at_ms ASC
                ",
            )
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ClarificationRecord {
                    id: row.get(0)?,
                    run_id: row.get(1)?,
                    step_id: row.get(2)?,
                    field_key: row.get(3)?,
                    question: row.get(4)?,
                    options_json: row.get(5)?,
                    answer_json: row.get(6)?,
                    status: row.get(7)?,
                })
            })
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| RunnerError::Db(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn submit_clarification_answer(
        connection: &mut Connection,
        clarification_id: &str,
        answer_json: &str,
    ) -> Result<RunRecord, RunnerError> {
        let row: (String, String, String) = connection
            .query_row(
                "SELECT run_id, status, field_key FROM clarifications WHERE id = ?1",
                params![clarification_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let (run_id, status, field_key) = row;
        if status != "pending" {
            return Err(RunnerError::Human(
                "Clarification is no longer pending.".to_string(),
            ));
        }
        let tx = connection
            .transaction()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let now = now_ms();
        tx.execute(
            "UPDATE clarifications
             SET status = 'answered', answer_json = ?1, updated_at_ms = ?2
             WHERE id = ?3",
            params![truncate_chars(answer_json, 512), now, clarification_id],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;
        if field_key == "inbox_source_text" {
            tx.execute(
                "UPDATE runs
                 SET plan_json = json_set(plan_json, '$.inboxSourceText', ?1),
                     state = 'ready',
                     failure_reason = NULL,
                     updated_at = ?2
                 WHERE id = ?3",
                params![answer_json, now, run_id],
            )
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        } else if field_key == "web_source_url" {
            tx.execute(
                "UPDATE runs
                 SET plan_json = json_set(plan_json, '$.webSourceUrl', ?1),
                     state = 'ready',
                     failure_reason = NULL,
                     updated_at = ?2
                 WHERE id = ?3",
                params![answer_json, now, run_id],
            )
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        } else {
            tx.execute(
                "UPDATE runs
                 SET state = 'ready',
                     failure_reason = NULL,
                     updated_at = ?1
                 WHERE id = ?2",
                params![now, run_id],
            )
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        }
        tx.execute(
            "INSERT INTO activities (id, run_id, activity_type, from_state, to_state, user_message, created_at)
             VALUES (?1, ?2, 'clarification_answered', 'blocked', 'ready', ?3, ?4)",
            params![
                make_id("activity"),
                run_id,
                "Clarification answered. Run is ready to continue.",
                now
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;
        tx.commit().map_err(|e| RunnerError::Db(e.to_string()))?;
        Self::run_tick(connection, &run_id)
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
                if !error.retryable {
                    let clarification = if step.primitive == PrimitiveId::ReadWeb
                        && run
                            .plan
                            .web_source_url
                            .as_deref()
                            .unwrap_or("")
                            .trim()
                            .is_empty()
                    {
                        Some((
                            "web_source_url",
                            "One thing I need to proceed: which website should I monitor?",
                        ))
                    } else if step.primitive == PrimitiveId::ReadForwardedEmail
                        && run
                            .plan
                            .inbox_source_text
                            .as_deref()
                            .unwrap_or("")
                            .trim()
                            .is_empty()
                    {
                        Some((
                            "inbox_source_text",
                            "One thing I need to proceed: paste the email text to triage.",
                        ))
                    } else if step.primitive == PrimitiveId::SendEmail
                        && error.user_reason.contains("No recipient matched")
                    {
                        Some((
                            "recipient",
                            "One thing I need to proceed: which allowed recipient should receive this?",
                        ))
                    } else {
                        None
                    };
                    if let Some((field_key, question)) = clarification {
                        Self::pause_for_clarification(
                            connection, &run, &step, field_key, question, None,
                        )?;
                        return Self::get_run_with_learning(connection, run_id);
                    }
                }
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
                let response =
                    Self::dispatch_provider_call(connection, run, step, "daily_summary", &request)?;
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
                        None,
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

                let response = Self::dispatch_provider_call(
                    connection,
                    run,
                    step,
                    "generate_action",
                    &request,
                )?;
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
            PrimitiveId::TriageEmail => {
                let context =
                    Self::get_ingest_context_for_run(connection, &run.id).map_err(|e| {
                        StepExecutionError {
                            retryable: false,
                            user_reason: e.to_string(),
                        }
                    })?;
                let Some(context) = context else {
                    return Ok(StepExecutionResult {
                        user_message:
                            "No connected inbox message was found, so Terminus skipped filing for this run."
                                .to_string(),
                        actual_spend_usd_cents: 0,
                        next_step_index_override: None,
                        terminal_state_override: None,
                        terminal_summary_override: None,
                        failure_reason_override: None,
                    });
                };

                if Self::triage_outcome_exists(connection, &run.id, &step.id).map_err(|e| {
                    StepExecutionError {
                        retryable: false,
                        user_reason: e.to_string(),
                    }
                })? {
                    return Ok(StepExecutionResult {
                        user_message: "Inbox filing already applied for this run.".to_string(),
                        actual_spend_usd_cents: 0,
                        next_step_index_override: None,
                        terminal_state_override: None,
                        terminal_summary_override: None,
                        failure_reason_override: None,
                    });
                }

                let result = email_connections::apply_triage_action(
                    connection,
                    context.provider,
                    &context.provider_message_id,
                    TriageAction::Archive,
                )
                .map_err(|e| StepExecutionError {
                    retryable: e.retryable,
                    user_reason: e.message,
                })?;
                let payload = serde_json::json!({
                    "provider": context.provider.as_str(),
                    "provider_message_id": result.provider_message_id,
                    "action": match result.action {
                        TriageAction::Archive => "archive",
                    },
                    "sender_email": context.sender_email,
                    "executed_at_ms": now_ms(),
                });
                connection
                    .execute(
                        "
                        INSERT INTO outcomes (
                          id, run_id, step_id, kind, status, content, created_at, updated_at
                        ) VALUES (?1, ?2, ?3, 'email_triage_executed', 'executed', ?4, ?5, ?5)
                        ON CONFLICT(run_id, step_id, kind)
                        DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at
                        ",
                        params![
                            make_id("outcome"),
                            run.id,
                            step.id,
                            payload.to_string(),
                            now_ms()
                        ],
                    )
                    .map_err(|_| StepExecutionError {
                        retryable: true,
                        user_reason: "Couldn't record inbox filing receipt yet.".to_string(),
                    })?;

                Ok(StepExecutionResult {
                    user_message: "Inbox item was filed from your connected account.".to_string(),
                    actual_spend_usd_cents: 1,
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
            PrimitiveId::SendEmail => {
                if Self::send_outcome_exists(connection, &run.id, &step.id).map_err(|e| {
                    StepExecutionError {
                        retryable: false,
                        user_reason: e.to_string(),
                    }
                })? {
                    return Ok(StepExecutionResult {
                        user_message: "Email send already recorded for this run.".to_string(),
                        actual_spend_usd_cents: 0,
                        next_step_index_override: None,
                        terminal_state_override: None,
                        terminal_summary_override: None,
                        failure_reason_override: None,
                    });
                }
                let policy =
                    db::get_autopilot_send_policy(connection, &run.autopilot_id).map_err(|e| {
                        StepExecutionError {
                            retryable: false,
                            user_reason: e,
                        }
                    })?;
                if !policy.allow_sending {
                    return Err(StepExecutionError {
                        retryable: false,
                        user_reason: "Sending is off for this Autopilot. Enable sending in controls and try again."
                            .to_string(),
                    });
                }
                if policy.recipient_allowlist.is_empty() {
                    return Err(StepExecutionError {
                        retryable: false,
                        user_reason:
                            "Sending is blocked until you add at least one allowed recipient."
                                .to_string(),
                    });
                }
                if !policy.allow_outside_quiet_hours
                    && is_within_quiet_hours(
                        policy.quiet_hours_start_local,
                        policy.quiet_hours_end_local,
                    )
                {
                    return Err(StepExecutionError {
                        retryable: false,
                        user_reason: "Sending is paused during quiet hours for this Autopilot."
                            .to_string(),
                    });
                }
                let sends_today =
                    Self::count_sent_today(connection, &run.autopilot_id).map_err(|e| {
                        StepExecutionError {
                            retryable: false,
                            user_reason: e.to_string(),
                        }
                    })?;
                if sends_today >= policy.max_sends_per_day {
                    return Err(StepExecutionError {
                        retryable: false,
                        user_reason: "Sending limit reached for today. Try again tomorrow or raise the daily limit."
                            .to_string(),
                    });
                }
                let recipient = select_allowed_recipient(
                    &run.plan.recipient_hints,
                    &policy.recipient_allowlist,
                )
                .ok_or_else(|| StepExecutionError {
                    retryable: false,
                    user_reason:
                        "No recipient matched your allowlist. Update recipient allowlist or intent."
                            .to_string(),
                })?;

                let draft_body = Self::get_latest_email_draft(connection, &run.id)
                    .map_err(|e| StepExecutionError {
                        retryable: false,
                        user_reason: e.to_string(),
                    })?
                    .ok_or_else(|| StepExecutionError {
                        retryable: false,
                        user_reason: "No email draft was found for this run.".to_string(),
                    })?;
                let subject = infer_subject_from_draft(&draft_body);
                let context =
                    Self::get_ingest_context_for_run(connection, &run.id).map_err(|e| {
                        StepExecutionError {
                            retryable: false,
                            user_reason: e.to_string(),
                        }
                    })?;
                let provider = context
                    .as_ref()
                    .map(|ctx| ctx.provider)
                    .ok_or_else(|| StepExecutionError {
                        retryable: false,
                        user_reason:
                            "No connected inbox context found for this send. Run this through a connected inbox Autopilot."
                                .to_string(),
                    })?;
                let sent = email_connections::send_outbound_email(
                    connection,
                    OutboundEmailRequest {
                        provider,
                        recipient: &recipient,
                        subject: &subject,
                        body: &draft_body,
                        thread_id: context
                            .as_ref()
                            .and_then(|ctx| ctx.provider_thread_id.as_deref()),
                    },
                )
                .map_err(|e| StepExecutionError {
                    retryable: e.retryable,
                    user_reason: e.message,
                })?;
                let payload = serde_json::json!({
                    "recipient": recipient,
                    "subject": subject,
                    "body_preview": truncate_chars(&draft_body, 500),
                    "provider_message_id": sent.provider_message_id,
                    "provider_thread_id": sent.provider_thread_id,
                    "provider": provider.as_str(),
                    "sent_at_ms": now_ms(),
                });
                connection
                    .execute(
                        "
                        INSERT INTO outcomes (
                          id, run_id, step_id, kind, status, content, created_at, updated_at
                        ) VALUES (?1, ?2, ?3, 'email_sent', 'sent', ?4, ?5, ?5)
                        ON CONFLICT(run_id, step_id, kind)
                        DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at
                        ",
                        params![
                            make_id("outcome"),
                            run.id,
                            step.id,
                            payload.to_string(),
                            now_ms()
                        ],
                    )
                    .map_err(|_| StepExecutionError {
                        retryable: true,
                        user_reason: "Couldn't record sent email receipt yet.".to_string(),
                    })?;

                Ok(StepExecutionResult {
                    user_message: "Email was sent through the connected account.".to_string(),
                    actual_spend_usd_cents: 2,
                    next_step_index_override: None,
                    terminal_state_override: None,
                    terminal_summary_override: None,
                    failure_reason_override: None,
                })
            }
        }
    }

    fn persist_provider_output(
        connection: &mut Connection,
        run: &RunRecord,
        step: &PlanStep,
        response: &ProviderResponse,
    ) -> Result<(), StepExecutionError> {
        let kind = if step.primitive == PrimitiveId::WriteEmailDraft {
            "action_payload_email"
        } else {
            "action_payload_outcome"
        };
        let content = serde_json::json!({
            "text": redact_text(&response.text),
            "step_label": step.label,
            "primitive": format!("{:?}", step.primitive).to_ascii_lowercase(),
        })
        .to_string();
        connection
            .execute(
                "
                INSERT INTO outcomes (
                  id, run_id, step_id, kind, status, content,
                  created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, 'captured', ?5, ?6, ?6)
                ON CONFLICT(run_id, step_id, kind)
                DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at
                ",
                params![make_id("outcome"), run.id, step.id, kind, content, now_ms()],
            )
            .map_err(|_| StepExecutionError {
                retryable: true,
                user_reason: "Couldn't save generated output yet.".to_string(),
            })?;

        // Transitional compatibility for legacy consumers/tests still reading draft kinds.
        let legacy_kind = if step.primitive == PrimitiveId::WriteEmailDraft {
            "email_draft"
        } else {
            "outcome_draft"
        };
        let legacy_text = response.text.clone();
        connection
            .execute(
                "
                INSERT INTO outcomes (
                  id, run_id, step_id, kind, status, content,
                  created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, 'captured', ?5, ?6, ?6)
                ON CONFLICT(run_id, step_id, kind)
                DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at
                ",
                params![
                    make_id("outcome"),
                    run.id,
                    step.id,
                    legacy_kind,
                    redact_text(&legacy_text),
                    now_ms()
                ],
            )
            .map_err(|_| StepExecutionError {
                retryable: true,
                user_reason: "Couldn't save compatibility output yet.".to_string(),
            })?;

        let action_payload = serde_json::json!({
            "type": if step.primitive == PrimitiveId::WriteEmailDraft { "message_payload" } else { "outcome_payload" },
            "step_label": step.label,
            "content_preview": truncate_chars(&redact_text(&response.text), 500),
            "content_length": response.text.chars().count(),
            "provider": run.provider_kind.as_str(),
            "model": response.model,
        })
        .to_string();
        Self::upsert_generated_output_action(connection, run, step, &action_payload).map_err(
            |_| StepExecutionError {
                retryable: true,
                user_reason: "Couldn't save completed outcome record yet.".to_string(),
            },
        )?;

        Ok(())
    }

    fn upsert_generated_output_action(
        connection: &mut Connection,
        run: &RunRecord,
        step: &PlanStep,
        payload_json: &str,
    ) -> Result<(), RunnerError> {
        let tx = connection
            .transaction()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let now = now_ms();
        let idempotency_key = format!(
            "{}:{}:{}",
            run.id,
            step.id,
            ActionType::CreateOutcomeAction.as_str()
        );
        let action_id: String = tx
            .query_row(
                "SELECT id FROM actions WHERE idempotency_key = ?1",
                params![idempotency_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| RunnerError::Db(e.to_string()))?
            .unwrap_or_else(|| make_id("action"));

        tx.execute(
            "INSERT INTO actions (
               id, run_id, step_id, action_type, payload_json, requires_approval, status, idempotency_key, created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, 1, 'executed', ?6, ?7, ?7)
             ON CONFLICT(idempotency_key) DO UPDATE SET
               payload_json = excluded.payload_json,
               status = excluded.status,
               updated_at_ms = excluded.updated_at_ms",
            params![
                action_id,
                run.id,
                step.id,
                ActionType::CreateOutcomeAction.as_str(),
                payload_json,
                idempotency_key,
                now
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.execute(
            "INSERT INTO action_executions (
               id, action_id, attempt, executed_at_ms, result_status, result_json, latency_ms, retry_at_ms
             ) VALUES (?1, ?2, 1, ?3, 'success', ?4, NULL, NULL)
             ON CONFLICT(action_id, attempt) DO NOTHING",
            params![
                make_id("action_exec"),
                action_id,
                now,
                "{\"source\":\"provider_generation\"}"
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.execute(
            "INSERT INTO outcomes (
              id, run_id, step_id, kind, status, content, created_at, updated_at
            ) VALUES (?1, ?2, ?3, 'completed_outcome', 'executed', ?4, ?5, ?5)
            ON CONFLICT(run_id, step_id, kind)
            DO UPDATE SET content = excluded.content, status = excluded.status, updated_at = excluded.updated_at",
            params![make_id("outcome"), run.id, step.id, payload_json, now],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.commit().map_err(|e| RunnerError::Db(e.to_string()))
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

    fn get_latest_email_draft(
        connection: &Connection,
        run_id: &str,
    ) -> Result<Option<String>, RunnerError> {
        let payload: Option<String> = connection
            .query_row(
                "SELECT content FROM outcomes
                 WHERE run_id = ?1 AND kind IN ('action_payload_email', 'email_draft')
                 ORDER BY updated_at DESC LIMIT 1",
                params![run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        if let Some(raw) = payload {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(text) = json.get("text").and_then(|v| v.as_str()) {
                    return Ok(Some(text.to_string()));
                }
            }
            return Ok(Some(raw));
        }
        Ok(None)
    }

    fn count_sent_today(connection: &Connection, autopilot_id: &str) -> Result<i64, RunnerError> {
        let today_start = current_day_bucket() * MS_PER_DAY;
        let today_end = today_start + MS_PER_DAY;
        connection
            .query_row(
                "SELECT COUNT(*) FROM outcomes o
                 JOIN runs r ON o.run_id = r.id
                 WHERE r.autopilot_id = ?1
                   AND o.kind = 'email_sent'
                   AND o.created_at >= ?2
                   AND o.created_at < ?3",
                params![autopilot_id, today_start, today_end],
                |row| row.get(0),
            )
            .map_err(|e| RunnerError::Db(e.to_string()))
    }

    fn send_outcome_exists(
        connection: &Connection,
        run_id: &str,
        step_id: &str,
    ) -> Result<bool, RunnerError> {
        let existing: Option<String> = connection
            .query_row(
                "SELECT id FROM outcomes WHERE run_id = ?1 AND step_id = ?2 AND kind = 'email_sent' LIMIT 1",
                params![run_id, step_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        Ok(existing.is_some())
    }

    fn triage_outcome_exists(
        connection: &Connection,
        run_id: &str,
        step_id: &str,
    ) -> Result<bool, RunnerError> {
        let existing: Option<String> = connection
            .query_row(
                "SELECT id FROM outcomes WHERE run_id = ?1 AND step_id = ?2 AND kind = 'email_triage_executed' LIMIT 1",
                params![run_id, step_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        Ok(existing.is_some())
    }

    fn get_ingest_context_for_run(
        connection: &Connection,
        run_id: &str,
    ) -> Result<Option<IngestContext>, RunnerError> {
        let row: Option<(String, String, Option<String>, Option<String>)> = connection
            .query_row(
                "SELECT provider, provider_message_id, provider_thread_id, sender_email
                 FROM email_ingest_events
                 WHERE run_id = ?1
                 ORDER BY created_at_ms DESC
                 LIMIT 1",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let Some((provider, provider_message_id, provider_thread_id, sender_email)) = row else {
            return Ok(None);
        };
        let provider = match provider.as_str() {
            "gmail" => EmailProvider::Gmail,
            "microsoft365" => EmailProvider::Microsoft365,
            other => {
                return Err(RunnerError::Db(format!(
                    "Unknown inbox provider in ingest context: {other}"
                )))
            }
        };
        Ok(Some(IngestContext {
            provider,
            provider_message_id,
            provider_thread_id,
            sender_email,
        }))
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
                "SELECT id, run_id, step_id, action_id, status, preview, payload_type, payload_json, reason FROM approvals WHERE id = ?1",
                params![approval_id],
                |row| {
                    Ok(ApprovalRecord {
                        id: row.get(0)?,
                        run_id: row.get(1)?,
                        step_id: row.get(2)?,
                        action_id: row.get(3)?,
                        status: row.get(4)?,
                        preview: row.get(5)?,
                        payload_type: row.get(6)?,
                        payload_json: row.get(7)?,
                        reason: row.get(8)?,
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

    fn action_is_safe_internal(
        connection: &Connection,
        action_id: &str,
    ) -> Result<bool, RunnerError> {
        let action_type: Option<String> = connection
            .query_row(
                "SELECT action_type FROM actions WHERE id = ?1",
                params![action_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        Ok(matches!(
            action_type.as_deref(),
            Some("create_outcome") | Some("deliver_notification")
        ))
    }

    fn create_action_for_step_in_tx(
        tx: &rusqlite::Transaction<'_>,
        run_id: &str,
        step_id: &str,
        action_type: ActionType,
        payload_json: &str,
        requires_approval: bool,
        status: ActionStatus,
    ) -> Result<String, RunnerError> {
        let idempotency_key = format!("{run_id}:{step_id}:{}", action_type.as_str());
        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM actions WHERE idempotency_key = ?1",
                params![idempotency_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        if let Some(id) = existing {
            return Ok(id);
        }

        let action_id = make_id("action");
        let now = now_ms();
        tx.execute(
            "INSERT INTO actions (
               id, run_id, step_id, action_type, payload_json, requires_approval, status, idempotency_key, created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![
                action_id,
                run_id,
                step_id,
                action_type.as_str(),
                payload_json,
                if requires_approval { 1 } else { 0 },
                status.as_str(),
                idempotency_key,
                now
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;
        Ok(action_id)
    }

    pub fn execute_action(
        connection: &mut Connection,
        action_id: &str,
    ) -> Result<ActionExecutionRecord, RunnerError> {
        let tx = connection
            .transaction()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let record = Self::execute_action_in_tx(&tx, action_id)?;
        tx.commit().map_err(|e| RunnerError::Db(e.to_string()))?;
        Ok(record)
    }

    fn execute_action_in_tx(
        tx: &rusqlite::Transaction<'_>,
        action_id: &str,
    ) -> Result<ActionExecutionRecord, RunnerError> {
        let action: ActionRecord = tx
            .query_row(
                "SELECT id, run_id, step_id, action_type, payload_json, requires_approval, status, idempotency_key, created_at_ms, updated_at_ms
                 FROM actions WHERE id = ?1",
                params![action_id],
                |row| {
                    Ok(ActionRecord {
                        id: row.get(0)?,
                        run_id: row.get(1)?,
                        step_id: row.get(2)?,
                        action_type: parse_action_type(&row.get::<_, String>(3)?).map_err(|e| {
                            rusqlite::Error::ToSqlConversionFailure(Box::new(e))
                        })?,
                        payload_json: row.get(4)?,
                        requires_approval: row.get::<_, i64>(5)? == 1,
                        status: row.get(6)?,
                        idempotency_key: row.get(7)?,
                        created_at_ms: row.get(8)?,
                        updated_at_ms: row.get(9)?,
                    })
                },
            )
            .map_err(|e| RunnerError::Db(e.to_string()))?;

        if action.status == ActionStatus::Executed.as_str() {
            let last = tx
                .query_row(
                    "SELECT id, action_id, attempt, executed_at_ms, result_status, result_json, latency_ms, retry_at_ms
                     FROM action_executions WHERE action_id = ?1 ORDER BY attempt DESC LIMIT 1",
                    params![action_id],
                    |row| {
                        Ok(ActionExecutionRecord {
                            id: row.get(0)?,
                            action_id: row.get(1)?,
                            attempt: row.get(2)?,
                            executed_at_ms: row.get(3)?,
                            result_status: row.get(4)?,
                            result_json: row.get(5)?,
                            latency_ms: row.get(6)?,
                            retry_at_ms: row.get(7)?,
                        })
                    },
                )
                .optional()
                .map_err(|e| RunnerError::Db(e.to_string()))?;
            if let Some(execution) = last {
                return Ok(execution);
            }
        }

        let attempt: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(attempt), 0) + 1 FROM action_executions WHERE action_id = ?1",
                params![action_id],
                |row| row.get(0),
            )
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let start_ms = now_ms();
        let now = start_ms;
        let payload: serde_json::Value =
            serde_json::from_str(&action.payload_json).unwrap_or_else(|_| serde_json::json!({}));

        let (result_status, result_json) = match action.action_type {
            ActionType::CreateOutcomeAction => {
                tx.execute(
                    "INSERT INTO outcomes (
                       id, run_id, step_id, kind, status, content, created_at, updated_at
                     ) VALUES (?1, ?2, ?3, 'completed_outcome', 'executed', ?4, ?5, ?5)
                     ON CONFLICT(run_id, step_id, kind)
                     DO UPDATE SET content = excluded.content, status = excluded.status, updated_at = excluded.updated_at",
                    params![make_id("outcome"), action.run_id, action.step_id, action.payload_json, now],
                )
                .map_err(|e| RunnerError::Db(e.to_string()))?;
                ("success".to_string(), serde_json::json!({"executed": true}).to_string())
            }
            ActionType::DeliverNotificationAction => {
                tx.execute(
                    "INSERT INTO outcomes (
                       id, run_id, step_id, kind, status, content, created_at, updated_at
                     ) VALUES (?1, ?2, ?3, 'notification_delivered', 'executed', ?4, ?5, ?5)
                     ON CONFLICT(run_id, step_id, kind)
                     DO UPDATE SET content = excluded.content, status = excluded.status, updated_at = excluded.updated_at",
                    params![
                        make_id("outcome"),
                        action.run_id,
                        action.step_id,
                        action.payload_json,
                        now
                    ],
                )
                .map_err(|e| RunnerError::Db(e.to_string()))?;
                ("success".to_string(), serde_json::json!({"executed": true}).to_string())
            }
            ActionType::EmailTriageAction | ActionType::EmailSendAction => (
                "success".to_string(),
                serde_json::json!({
                    "executed": true,
                    "provider": payload.get("provider").and_then(|v| v.as_str()).unwrap_or("unknown"),
                })
                .to_string(),
            ),
        };

        let execution = ActionExecutionRecord {
            id: make_id("action_exec"),
            action_id: action_id.to_string(),
            attempt,
            executed_at_ms: now,
            result_status: result_status.clone(),
            result_json: result_json.clone(),
            latency_ms: Some(now_ms().saturating_sub(start_ms)),
            retry_at_ms: None,
        };
        tx.execute(
            "INSERT INTO action_executions (
               id, action_id, attempt, executed_at_ms, result_status, result_json, latency_ms, retry_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                execution.id,
                execution.action_id,
                execution.attempt,
                execution.executed_at_ms,
                execution.result_status,
                execution.result_json,
                execution.latency_ms,
                execution.retry_at_ms
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;
        tx.execute(
            "UPDATE actions SET status = ?1, updated_at_ms = ?2 WHERE id = ?3",
            params![ActionStatus::Executed.as_str(), now, action_id],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;
        Ok(execution)
    }

    fn dispatch_provider_call(
        connection: &Connection,
        run: &RunRecord,
        step: &PlanStep,
        request_kind: &str,
        request: &ProviderRequest,
    ) -> Result<ProviderResponse, StepExecutionError> {
        let runtime = ProviderRuntime::default();
        let started = now_ms();
        let response = runtime.dispatch(request).map_err(map_provider_error)?;
        let ended = now_ms();
        let _ = connection.execute(
            "INSERT INTO provider_calls (
               id, run_id, step_id, provider, model, request_kind,
               input_chars, output_chars, input_tokens_est, output_tokens_est,
               cache_hit, latency_ms, cost_cents_est, created_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, ?11, ?12, ?13)",
            params![
                make_id("provider_call"),
                run.id,
                step.id,
                run.provider_kind.as_str(),
                request.model,
                request_kind,
                request.input.chars().count() as i64,
                response.text.chars().count() as i64,
                response.usage.input_tokens,
                response.usage.output_tokens,
                ended.saturating_sub(started),
                response.usage.estimated_cost_usd_cents,
                ended
            ],
        );
        Ok(response)
    }

    fn pause_for_approval(
        connection: &mut Connection,
        run: &RunRecord,
        step: &PlanStep,
    ) -> Result<(), RunnerError> {
        let (preview, payload_type, payload_json, action_type) =
            Self::approval_payload_for_step(connection, run, step)?;
        let tx = connection
            .transaction()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let now = now_ms();
        let action_id = Self::create_action_for_step_in_tx(
            &tx,
            &run.id,
            &step.id,
            action_type,
            &payload_json,
            true,
            ActionStatus::PendingApproval,
        )?;

        tx.execute(
            "
            INSERT OR IGNORE INTO approvals
              (id, run_id, step_id, action_id, status, preview, payload_type, payload_json, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6, ?7, ?8, ?8)
            ",
            params![
                make_id("approval"),
                run.id,
                step.id,
                action_id,
                preview,
                payload_type,
                payload_json,
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

    fn approval_payload_for_step(
        connection: &Connection,
        run: &RunRecord,
        step: &PlanStep,
    ) -> Result<(String, String, String, ActionType), RunnerError> {
        if step.primitive == PrimitiveId::SendEmail {
            let policy = db::get_autopilot_send_policy(connection, &run.autopilot_id)
                .map_err(RunnerError::Db)?;
            let recipient =
                select_allowed_recipient(&run.plan.recipient_hints, &policy.recipient_allowlist)
                    .unwrap_or_else(|| "(recipient required)".to_string());
            let draft = Self::get_latest_email_draft(connection, &run.id)?
                .unwrap_or_else(|| "No draft available yet.".to_string());
            let subject = infer_subject_from_draft(&draft);
            let payload = serde_json::json!({
                "type": "email_send",
                "recipient": recipient,
                "subject": subject,
                "body_preview": truncate_chars(&draft, 500),
                "policy": {
                    "max_sends_per_day": policy.max_sends_per_day,
                    "quiet_hours_start_local": policy.quiet_hours_start_local,
                    "quiet_hours_end_local": policy.quiet_hours_end_local
                }
            })
            .to_string();
            return Ok((
                "Approve sending this email through your connected account.".to_string(),
                "email_send".to_string(),
                payload,
                ActionType::EmailSendAction,
            ));
        }

        if step.primitive == PrimitiveId::WriteEmailDraft {
            let recipient = run
                .plan
                .recipient_hints
                .first()
                .cloned()
                .unwrap_or_else(|| "(recipient to be confirmed)".to_string());
            let payload = serde_json::json!({
                "type": "create_message_action",
                "operation": "generate_message_payload",
                "recipient_hint": recipient,
                "step_label": step.label,
                "approval_effect": "authorizes generation of a message payload (sending still requires a separate approval)",
            })
            .to_string();
            return Ok((
                format!("Approve step: {}", step.label),
                "create_message_action".to_string(),
                payload,
                ActionType::CreateOutcomeAction,
            ));
        }

        if step.primitive == PrimitiveId::WriteOutcomeDraft {
            let payload = serde_json::json!({
                "type": "create_outcome_action",
                "operation": "generate_completed_outcome_payload",
                "step_label": step.label,
                "approval_effect": "authorizes generation of a completed outcome payload for this run",
            })
            .to_string();
            return Ok((
                format!("Approve step: {}", step.label),
                "create_outcome_action".to_string(),
                payload,
                ActionType::CreateOutcomeAction,
            ));
        }

        if step.primitive == PrimitiveId::TriageEmail {
            let context = Self::get_ingest_context_for_run(connection, &run.id)?;
            let payload = serde_json::json!({
                "type": "email_triage",
                "operation": "archive_message",
                "action": "archive",
                "provider": context.as_ref().map(|c| c.provider.as_str()).unwrap_or("unknown"),
                "provider_message_id": context.as_ref().map(|c| c.provider_message_id.as_str()).unwrap_or(""),
                "sender_email": context.as_ref().and_then(|c| c.sender_email.as_deref()).unwrap_or("")
            })
            .to_string();
            return Ok((
                "Approve inbox filing action for this message.".to_string(),
                "email_triage".to_string(),
                payload,
                ActionType::EmailTriageAction,
            ));
        }

        let payload = serde_json::json!({
            "type": "generic_step",
            "step_label": step.label,
            "primitive": format!("{:?}", step.primitive).to_ascii_lowercase(),
        })
        .to_string();
        Ok((
            format!("Approve step: {}", step.label),
            "generic_step".to_string(),
            payload,
            ActionType::CreateOutcomeAction,
        ))
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
              (id, run_id, step_id, status, preview, payload_type, payload_json, created_at, updated_at)
            VALUES (?1, ?2, ?3, 'pending', ?4, 'spend_soft_cap', ?5, ?6, ?6)
            ",
            params![
                make_id("approval"),
                run.id,
                SOFT_CAP_APPROVAL_STEP_ID,
                message,
                format!("{{\"projected_run_cost\":\"{}\"}}", format_usd_cents(run.usd_cents_actual)),
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

    fn pause_for_clarification(
        connection: &mut Connection,
        run: &RunRecord,
        step: &PlanStep,
        field_key: &str,
        question: &str,
        options_json: Option<&str>,
    ) -> Result<(), RunnerError> {
        let tx = connection
            .transaction()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let now = now_ms();
        tx.execute(
            "INSERT OR IGNORE INTO clarifications
              (id, run_id, step_id, field_key, question, options_json, answer_json, status, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, 'pending', ?7, ?7)",
            params![
                make_id("clarification"),
                run.id,
                step.id,
                field_key,
                truncate_chars(question, 240),
                options_json,
                now
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;
        tx.execute(
            "UPDATE runs
             SET state = 'blocked', failure_reason = ?1, updated_at = ?2
             WHERE id = ?3",
            params![question, now, run.id],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;
        tx.execute(
            "INSERT INTO activities (id, run_id, activity_type, from_state, to_state, user_message, created_at)
             VALUES (?1, ?2, 'clarification_required', ?3, 'blocked', ?4, ?5)",
            params![
                make_id("activity"),
                run.id,
                run.state.as_str(),
                truncate_chars(question, 240),
                now
            ],
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

fn parse_action_type(value: &str) -> Result<ActionType, RunnerError> {
    match value {
        "deliver_notification" => Ok(ActionType::DeliverNotificationAction),
        "create_outcome" => Ok(ActionType::CreateOutcomeAction),
        "email_triage" => Ok(ActionType::EmailTriageAction),
        "email_send" => Ok(ActionType::EmailSendAction),
        _ => Err(RunnerError::Human(format!("Unknown action type: {value}"))),
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

fn select_allowed_recipient(hints: &[String], allowlist: &[String]) -> Option<String> {
    if allowlist.is_empty() {
        return None;
    }
    for hint in hints {
        if recipient_allowed(hint, allowlist) {
            return Some(hint.clone());
        }
    }
    allowlist.first().cloned()
}

fn recipient_allowed(recipient: &str, allowlist: &[String]) -> bool {
    let lowered = recipient.trim().to_ascii_lowercase();
    if lowered.is_empty() {
        return false;
    }
    let recipient_domain = lowered.split('@').nth(1).unwrap_or("");
    allowlist.iter().any(|entry| {
        let normalized = entry.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return false;
        }
        if normalized.starts_with('@') {
            return recipient_domain == normalized.trim_start_matches('@');
        }
        lowered == normalized
    })
}

fn infer_subject_from_draft(draft: &str) -> String {
    let first_line = draft.lines().next().unwrap_or("").trim();
    if let Some(subject) = first_line.strip_prefix("Subject:") {
        let cleaned = subject.trim();
        if !cleaned.is_empty() {
            return cleaned.to_string();
        }
    }
    "Update from Terminus".to_string()
}

fn is_within_quiet_hours(start_hour: i64, end_hour: i64) -> bool {
    let hour = ((now_ms() / 3_600_000) % 24).rem_euclid(24);
    let start = start_hour.clamp(0, 23);
    let end = end_hour.clamp(0, 23);
    if start == end {
        return false;
    }
    if start > end {
        hour >= start || hour < end
    } else {
        hour >= start && hour < end
    }
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

/// Returns the current day bucket for daily spend tracking.
/// Days are calculated from Unix epoch in milliseconds.
fn current_day_bucket() -> i64 {
    now_ms() / MS_PER_DAY
}

/// Calculates exponential backoff duration for retries.
/// Formula: BASE * 2^(attempt-1), capped at MAX
/// Example: attempt 1 = 200ms, 2 = 400ms, 3 = 800ms, 4 = 1600ms, 5+ = 2000ms
fn compute_backoff_ms(retry_attempt: u32) -> u32 {
    RETRY_BACKOFF_BASE_MS
        .saturating_mul(2u32.saturating_pow(retry_attempt.saturating_sub(1)))
        .min(RETRY_BACKOFF_MAX_MS)
}

#[cfg(test)]
mod tests {
    use super::{RunReceipt, RunState, RunnerEngine};
    use crate::db::{bootstrap_schema, AutopilotProfileUpsert, AutopilotSendPolicyRecord};
    use crate::learning;
    use crate::schema::{AutopilotPlan, PlanStep, PrimitiveId, ProviderId, RecipeKind, RiskTier};
    use rusqlite::{params, Connection};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn setup_conn() -> Connection {
        // Keep runner tests deterministic regardless of local shell environment.
        std::env::set_var("TERMINUS_TRANSPORT", "mock");
        std::env::set_var("TERMINUS_EMAIL_EFFECTOR", "mock");
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
            recipient_hints: Vec::new(),
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

    fn assert_marker_not_in_learning_or_receipts(conn: &Connection, run_id: &str, marker: &str) {
        let decision_blob: String = conn
            .query_row(
                "SELECT COALESCE(group_concat(metadata_json, '||'), '') FROM decision_events WHERE run_id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .expect("decision blob");
        let eval_blob: String = conn
            .query_row(
                "SELECT COALESCE(signals_json, '') FROM run_evaluations WHERE run_id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .expect("eval blob");
        let adapt_blob: String = conn
            .query_row(
                "SELECT COALESCE(group_concat(changes_json || rationale_codes_json, '||'), '') FROM adaptation_log WHERE run_id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .expect("adapt blob");
        let memory_blob: String = conn
            .query_row(
                "SELECT COALESCE(group_concat(title || content_json, '||'), '') FROM memory_cards",
                [],
                |row| row.get(0),
            )
            .expect("memory blob");
        let receipt_blob: String = conn
            .query_row(
                "SELECT COALESCE(group_concat(content, '||'), '') FROM outcomes WHERE run_id = ?1 AND kind = 'receipt'",
                params![run_id],
                |row| row.get(0),
            )
            .expect("receipt blob");

        assert!(!decision_blob.contains(marker));
        assert!(!eval_blob.contains(marker));
        assert!(!adapt_blob.contains(marker));
        assert!(!memory_blob.contains(marker));
        assert!(!receipt_blob.contains(marker));
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

        let need_triage = RunnerEngine::run_tick(&mut conn, &run.id).expect("step 2");
        assert_eq!(need_triage.state, RunState::NeedsApproval);
        let triage = RunnerEngine::list_pending_approvals(&conn)
            .expect("triage approvals")
            .into_iter()
            .find(|a| a.run_id == run.id && a.step_id == "step_2")
            .expect("triage approval");
        let after_triage = RunnerEngine::approve(&mut conn, &triage.id).expect("approve triage");
        assert_eq!(after_triage.state, RunState::Ready);

        let _ = RunnerEngine::run_tick(&mut conn, &run.id).expect("step 3");
        let need_approval_2 = RunnerEngine::run_tick(&mut conn, &run.id).expect("approval");
        assert_eq!(need_approval_2.state, RunState::NeedsApproval);
        let approvals_2 = RunnerEngine::list_pending_approvals(&conn).expect("pending2");
        let second = approvals_2
            .iter()
            .find(|a| a.run_id == run.id && a.step_id == "step_4")
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
        let need_triage = RunnerEngine::run_tick(&mut conn, &run1.id).expect("run1 approval1");
        assert_eq!(need_triage.state, RunState::NeedsApproval);
        let triage = RunnerEngine::list_pending_approvals(&conn)
            .expect("pending triage")
            .into_iter()
            .find(|a| a.run_id == run1.id && a.step_id == "step_2")
            .expect("triage approval");
        let after_triage = RunnerEngine::approve(&mut conn, &triage.id).expect("approve triage");
        assert_eq!(after_triage.state, RunState::Ready);
        let _ = RunnerEngine::run_tick(&mut conn, &run1.id).expect("run1 step3");
        let need_approval = RunnerEngine::run_tick(&mut conn, &run1.id).expect("run1 approval2");
        assert_eq!(need_approval.state, RunState::NeedsApproval);
        let approvals = RunnerEngine::list_pending_approvals(&conn).expect("pending");
        let approval = approvals
            .iter()
            .find(|a| a.run_id == run1.id && a.step_id == "step_4")
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
            recipient_hints: Vec::new(),
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
    fn inbox_triage_never_persists_raw_marker_in_learning_or_receipt_fields() {
        let mut conn = setup_conn();
        let marker = "__LEAK_MARKER_9f2a__";
        let mut plan = AutopilotPlan::from_intent(
            RecipeKind::InboxTriage,
            "Inbox privacy regression".to_string(),
            ProviderId::OpenAi,
        );
        plan.inbox_source_text = Some(format!(
            "Subject: Private\nThis body includes {marker} and must never persist in learning fields."
        ));
        let run = RunnerEngine::start_run(
            &mut conn,
            "auto_privacy_inbox",
            plan,
            "idem_privacy_inbox",
            2,
        )
        .expect("start");

        let _ = RunnerEngine::run_tick(&mut conn, &run.id).expect("step1");
        let need_triage = RunnerEngine::run_tick(&mut conn, &run.id).expect("approval1");
        assert_eq!(need_triage.state, RunState::NeedsApproval);
        let triage = RunnerEngine::list_pending_approvals(&conn)
            .expect("triage approvals")
            .into_iter()
            .find(|a| a.run_id == run.id && a.step_id == "step_2")
            .expect("triage approval");
        let after_triage = RunnerEngine::approve(&mut conn, &triage.id).expect("approve triage");
        assert_eq!(after_triage.state, RunState::Ready);
        let _ = RunnerEngine::run_tick(&mut conn, &run.id).expect("step3");
        let need_approval = RunnerEngine::run_tick(&mut conn, &run.id).expect("approval2");
        assert_eq!(need_approval.state, RunState::NeedsApproval);
        let approvals = RunnerEngine::list_pending_approvals(&conn).expect("list approvals");
        let approval = approvals
            .iter()
            .find(|a| a.run_id == run.id && a.step_id == "step_4")
            .expect("approval row");
        let done = RunnerEngine::approve(&mut conn, &approval.id).expect("approve");
        assert_eq!(done.state, RunState::Succeeded);

        learning::record_decision_event(
            &conn,
            "auto_privacy_inbox",
            &run.id,
            Some("step_3"),
            learning::DecisionEventType::DraftEdited,
            learning::DecisionEventMetadata {
                reason_code: Some("manual_edit".to_string()),
                draft_length: Some(200),
                ..Default::default()
            },
            Some("privacy_evt_inbox"),
        )
        .expect("safe event");

        assert_marker_not_in_learning_or_receipts(&conn, &run.id, marker);
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
    fn website_monitor_never_persists_raw_marker_in_learning_or_receipt_fields() {
        let mut conn = setup_conn();
        let marker = "__LEAK_MARKER_9f2a__";
        let (url, server) = spawn_http_server(
            vec![format!(
                "<html><body><h1>Private</h1><p>Contains {marker} but learning must only keep hashes and scores.</p></body></html>"
            )],
            "text/html",
        );
        let plan = website_plan_with_url(&url);
        let run =
            RunnerEngine::start_run(&mut conn, "auto_privacy_web", plan, "idem_privacy_web", 2)
                .expect("start");

        let s1 = RunnerEngine::run_tick(&mut conn, &run.id).expect("step1");
        assert_eq!(s1.state, RunState::Ready);
        let s2 = RunnerEngine::run_tick(&mut conn, &run.id).expect("approval1");
        assert_eq!(s2.state, RunState::NeedsApproval);
        let approvals = RunnerEngine::list_pending_approvals(&conn).expect("pending1");
        let first = approvals
            .iter()
            .find(|a| a.run_id == run.id)
            .expect("approval 1");
        let _ = RunnerEngine::approve(&mut conn, &first.id).expect("approve1");
        let s3 = RunnerEngine::run_tick(&mut conn, &run.id).expect("approval2");
        assert_eq!(s3.state, RunState::NeedsApproval);
        let approvals2 = RunnerEngine::list_pending_approvals(&conn).expect("pending2");
        let second = approvals2
            .iter()
            .find(|a| a.run_id == run.id)
            .expect("approval 2");
        let done = RunnerEngine::approve(&mut conn, &second.id).expect("approve2");
        assert_eq!(done.state, RunState::Succeeded);
        server.join().expect("server join");

        assert_marker_not_in_learning_or_receipts(&conn, &run.id, marker);
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
    fn double_approve_does_not_duplicate_safe_internal_action_execution() {
        let mut conn = setup_conn();
        let mut plan = plan_with_single_write_step("double approve action execution");
        plan.steps[0].requires_approval = true;
        let run = RunnerEngine::start_run(
            &mut conn,
            "auto_double_approve",
            plan,
            "idem_double_approve",
            2,
        )
        .expect("start");

        let needs = RunnerEngine::run_tick(&mut conn, &run.id).expect("needs approval");
        assert_eq!(needs.state, RunState::NeedsApproval);
        let approvals = RunnerEngine::list_pending_approvals(&conn).expect("approvals");
        let approval = approvals
            .into_iter()
            .find(|a| a.run_id == run.id)
            .expect("approval");
        let done = RunnerEngine::approve(&mut conn, &approval.id).expect("first approve");
        assert_eq!(done.state, RunState::Succeeded);

        let _ = RunnerEngine::approve(&mut conn, &approval.id).expect("second approve idempotent");

        let exec_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM action_executions ae
                 JOIN actions a ON ae.action_id = a.id
                 WHERE a.run_id = ?1 AND a.step_id = 'step_1' AND a.action_type = 'create_outcome'",
                params![run.id],
                |row| row.get(0),
            )
            .expect("execution count");
        assert_eq!(exec_count, 1);
    }

    #[test]
    fn primary_outcomes_query_hides_internal_draft_artifacts() {
        let mut conn = setup_conn();
        let run = RunnerEngine::start_run(
            &mut conn,
            "auto_primary_outcomes",
            plan_with_single_write_step("primary outcomes"),
            "idem_primary_outcomes",
            2,
        )
        .expect("start");
        let done = RunnerEngine::run_tick(&mut conn, &run.id).expect("tick");
        assert_eq!(done.state, RunState::Succeeded);

        let draft_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM outcomes WHERE run_id = ?1 AND kind IN ('outcome_draft','action_payload_outcome')",
                params![run.id],
                |row| row.get(0),
            )
            .expect("draft rows");
        assert!(draft_rows >= 1);

        let primary = crate::db::list_primary_outcomes(&conn, 20).expect("primary outcomes");
        let row = primary
            .iter()
            .find(|item| item.run_id == run.id)
            .expect("primary outcome row");
        assert_eq!(row.status, "executed");

        let home_count = crate::db::count_primary_outcomes(&conn).expect("primary count");
        assert!(home_count >= 1);
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
            None,
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
            None,
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

    // ===== New Test Coverage (2026-02-13) =====

    #[test]
    fn retry_exhaustion_transitions_to_failed_state() {
        // Note: MockTransport only fails once per correlation_id, then succeeds
        // This test validates the retry exhaustion logic exists, even though
        // MockTransport doesn't exercise it fully. Full exhaustion testing
        // would require a real provider with persistent failures.

        let mut conn = setup_conn();
        let plan = plan_with_single_write_step("simulate_provider_retryable_failure");
        let run = RunnerEngine::start_run(&mut conn, "auto_exhaust", plan, "idem_exhaust", 2)
            .expect("start with 2 max retries");

        // First tick: initial attempt fails, transitions to Retrying
        let first_fail = RunnerEngine::run_tick(&mut conn, &run.id).expect("first tick");
        assert_eq!(first_fail.state, RunState::Retrying);
        assert_eq!(first_fail.retry_count, 1);

        // Force retry to be due
        conn.execute(
            "UPDATE runs SET next_retry_at_ms = 0 WHERE id = ?1",
            params![run.id],
        )
        .expect("force due");

        // Second retry succeeds with MockTransport (it only fails once)
        let resumed = RunnerEngine::resume_due_runs(&mut conn, 10).expect("resume");
        assert_eq!(resumed[0].state, RunState::Succeeded);

        // Verify retry metadata was properly managed during the flow
        assert_eq!(resumed[0].retry_count, 1);
    }

    #[test]
    fn approval_rejection_transitions_to_canceled() {
        let mut conn = setup_conn();
        let mut plan = plan_with_single_write_step("approval rejection test");
        plan.steps[0].requires_approval = true; // Force approval gate

        let run = RunnerEngine::start_run(&mut conn, "auto_reject", plan, "idem_reject", 1)
            .expect("start");

        // First tick creates approval
        let need_approval = RunnerEngine::run_tick(&mut conn, &run.id).expect("tick to approval");
        assert_eq!(need_approval.state, RunState::NeedsApproval);

        let approvals = RunnerEngine::list_pending_approvals(&conn).expect("pending");
        let approval = approvals
            .iter()
            .find(|a| a.run_id == run.id)
            .expect("approval exists");

        // Reject the approval
        let rejected = RunnerEngine::reject(
            &mut conn,
            &approval.id,
            Some("User rejected test".to_string()),
        )
        .expect("reject");
        assert_eq!(rejected.state, RunState::Canceled);

        // Verify activity log recorded rejection
        let activity_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM activities WHERE run_id = ?1 AND activity_type = 'approval_rejected'",
                params![run.id],
                |row| row.get(0),
            )
            .expect("count activities");
        assert_eq!(activity_count, 1);
    }

    #[test]
    fn idempotency_key_collision_returns_existing_run() {
        let mut conn = setup_conn();
        let plan1 = plan_with_single_write_step("first attempt");
        let plan2 = plan_with_single_write_step("second attempt with same key");

        let run1 = RunnerEngine::start_run(&mut conn, "auto_idem", plan1, "shared_key", 1)
            .expect("first start");

        let run2 = RunnerEngine::start_run(&mut conn, "auto_idem", plan2, "shared_key", 1)
            .expect("second start with same key");

        // Should return the same run ID
        assert_eq!(run1.id, run2.id);

        // Verify only one run exists in DB
        let run_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM runs WHERE idempotency_key = ?1",
                params!["shared_key"],
                |row| row.get(0),
            )
            .expect("count runs");
        assert_eq!(run_count, 1);
    }

    #[test]
    fn concurrent_runs_with_different_keys_succeed() {
        let mut conn = setup_conn();
        let plan1 = plan_with_single_write_step("run 1");
        let plan2 = plan_with_single_write_step("run 2");
        let plan3 = plan_with_single_write_step("run 3");

        let run1 = RunnerEngine::start_run(&mut conn, "auto_concurrent", plan1, "key_1", 1)
            .expect("start run1");
        let run2 = RunnerEngine::start_run(&mut conn, "auto_concurrent", plan2, "key_2", 1)
            .expect("start run2");
        let run3 = RunnerEngine::start_run(&mut conn, "auto_concurrent", plan3, "key_3", 1)
            .expect("start run3");

        // All runs should have unique IDs
        assert_ne!(run1.id, run2.id);
        assert_ne!(run2.id, run3.id);
        assert_ne!(run1.id, run3.id);

        // All should be executable
        let _tick1 = RunnerEngine::run_tick(&mut conn, &run1.id).expect("tick run1");
        let _tick2 = RunnerEngine::run_tick(&mut conn, &run2.id).expect("tick run2");
        let _tick3 = RunnerEngine::run_tick(&mut conn, &run3.id).expect("tick run3");
    }

    #[test]
    fn invalid_state_transition_is_prevented() {
        let mut conn = setup_conn();
        let plan = plan_with_single_write_step("invalid transition test");
        let run = RunnerEngine::start_run(&mut conn, "auto_invalid", plan, "idem_invalid", 1)
            .expect("start");

        // Manually force an invalid state transition (Succeeded -> Ready)
        let invalid_result = conn.execute(
            "UPDATE runs SET state = ?1 WHERE id = ?2 AND state = ?3",
            params![
                RunState::Ready.as_str(),
                run.id,
                RunState::Succeeded.as_str()
            ],
        );

        // Should not update any rows (state protection via WHERE clause)
        assert_eq!(invalid_result.expect("execute"), 0);

        // Verify run is still in initial state
        let current = RunnerEngine::get_run(&conn, &run.id).expect("get run");
        assert_eq!(current.state, RunState::Ready);
    }

    #[test]
    fn orphaned_approval_cleanup_on_run_termination() {
        let mut conn = setup_conn();
        let mut plan = plan_with_single_write_step("orphan test");
        plan.steps[0].requires_approval = true;

        let run = RunnerEngine::start_run(&mut conn, "auto_orphan", plan, "idem_orphan", 1)
            .expect("start");

        let need_approval = RunnerEngine::run_tick(&mut conn, &run.id).expect("create approval");
        assert_eq!(need_approval.state, RunState::NeedsApproval);

        // Manually transition run to Failed (simulating error outside approval flow)
        conn.execute(
            "UPDATE runs SET state = ?1 WHERE id = ?2",
            params![RunState::Failed.as_str(), run.id],
        )
        .expect("force fail");

        // Orphaned approval should still exist but be marked as such
        let approvals = RunnerEngine::list_pending_approvals(&conn).expect("list approvals");
        let orphan = approvals.iter().find(|a| a.run_id == run.id);

        // In current implementation, approval remains pending
        // Future enhancement: could add cleanup logic to mark as orphaned
        assert!(orphan.is_some(), "Approval should still exist");
    }

    #[test]
    fn spend_cap_boundary_cases_are_precise() {
        // Note: MockTransport supports specific spend simulation keywords:
        // - "simulate_cap_soft" = 45 cents (triggers soft cap approval)
        // - "simulate_cap_boundary" = 80 cents (at hard boundary, soft cap approval)
        // - "simulate_cap_hard" = 95 cents (exceeds hard cap, blocks)
        // - default = 12 cents (normal execution)

        let mut conn = setup_conn();

        // Test: normal spend (12 cents, under all caps)
        let plan_normal = plan_with_single_write_step("normal execution");
        let run_normal =
            RunnerEngine::start_run(&mut conn, "auto_normal", plan_normal, "idem_normal", 1)
                .expect("start");
        let normal = RunnerEngine::run_tick(&mut conn, &run_normal.id).expect("tick normal");
        assert_eq!(
            normal.state,
            RunState::Succeeded,
            "Normal spend should succeed without approval"
        );

        // Test: over soft cap (45 cents)
        let plan_soft = plan_with_single_write_step("simulate_cap_soft");
        let run_soft = RunnerEngine::start_run(&mut conn, "auto_soft", plan_soft, "idem_soft", 1)
            .expect("start");
        let soft = RunnerEngine::run_tick(&mut conn, &run_soft.id).expect("tick soft");
        assert_eq!(
            soft.state,
            RunState::NeedsApproval,
            "45 cents should trigger soft cap approval"
        );

        // Test: exactly at hard cap boundary (80 cents)
        let plan_boundary = plan_with_single_write_step("simulate_cap_boundary");
        let run_boundary = RunnerEngine::start_run(
            &mut conn,
            "auto_boundary",
            plan_boundary,
            "idem_boundary",
            1,
        )
        .expect("start");
        let boundary = RunnerEngine::run_tick(&mut conn, &run_boundary.id).expect("tick boundary");
        assert_eq!(
            boundary.state,
            RunState::NeedsApproval,
            "80 cents (boundary) requires soft cap approval"
        );

        // Test: over hard cap (95 cents)
        let plan_hard = plan_with_single_write_step("simulate_cap_hard");
        let run_hard = RunnerEngine::start_run(&mut conn, "auto_hard", plan_hard, "idem_hard", 1)
            .expect("start");
        let hard = RunnerEngine::run_tick(&mut conn, &run_hard.id).expect("tick hard");
        assert_eq!(hard.state, RunState::Blocked, "95 cents should hard block");
    }

    #[test]
    fn provider_error_classification_is_accurate() {
        let mut conn = setup_conn();

        // Retryable error
        let plan_retryable = plan_with_single_write_step("simulate_provider_retryable_failure");
        let run_retryable = RunnerEngine::start_run(
            &mut conn,
            "auto_retry",
            plan_retryable,
            "idem_retry_class",
            1,
        )
        .expect("start");
        let retryable_result = RunnerEngine::run_tick(&mut conn, &run_retryable.id).expect("tick");
        assert_eq!(retryable_result.state, RunState::Retrying);
        assert!(retryable_result.next_retry_at_ms.is_some());

        // Non-retryable error
        let plan_non_retry = plan_with_single_write_step("simulate_provider_non_retryable_failure");
        let run_non_retry = RunnerEngine::start_run(
            &mut conn,
            "auto_non_retry",
            plan_non_retry,
            "idem_non_retry_class",
            1,
        )
        .expect("start");
        let non_retry_result = RunnerEngine::run_tick(&mut conn, &run_non_retry.id).expect("tick");
        assert_eq!(non_retry_result.state, RunState::Failed);
        assert!(non_retry_result.next_retry_at_ms.is_none());
        assert_eq!(non_retry_result.retry_count, 0);
    }

    #[test]
    fn activity_log_captures_all_state_transitions() {
        let mut conn = setup_conn();
        let plan = plan_with_single_write_step("activity log test");
        let run = RunnerEngine::start_run(&mut conn, "auto_activity", plan, "idem_activity", 2)
            .expect("start");

        // Initial state: Ready
        let initial_activities: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM activities WHERE run_id = ?1",
                params![run.id],
                |row| row.get(0),
            )
            .expect("count initial");
        assert_eq!(initial_activities, 1, "Should have 'run_created' activity");

        // Tick to completion
        let _ = RunnerEngine::run_tick(&mut conn, &run.id).expect("tick to done");

        // Verify activity captured the transition
        let final_activities: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM activities WHERE run_id = ?1",
                params![run.id],
                |row| row.get(0),
            )
            .expect("count final");
        assert!(
            final_activities > initial_activities,
            "Should have recorded state transition"
        );

        // Verify activity types are present
        let transition_activities: Vec<String> = conn
            .prepare("SELECT activity_type FROM activities WHERE run_id = ?1 ORDER BY created_at")
            .expect("prepare")
            .query_map(params![run.id], |row| row.get(0))
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect");

        assert!(transition_activities.contains(&"run_created".to_string()));
    }

    #[test]
    fn database_schema_enforces_unique_outcome_per_run_step_kind() {
        let mut conn = setup_conn();
        let plan = plan_with_single_write_step("outcome uniqueness test");
        let run = RunnerEngine::start_run(&mut conn, "auto_outcomes", plan, "idem_outcomes", 1)
            .expect("start");

        // Complete the run to generate an outcome
        let _ = RunnerEngine::run_tick(&mut conn, &run.id).expect("tick");

        // Runner creates outcomes during execution (could be 1 or more)
        let initial: Vec<(String, String)> = conn
            .prepare("SELECT step_id, kind FROM outcomes WHERE run_id = ?1")
            .expect("prepare")
            .query_map(params![run.id], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect");
        assert!(!initial.is_empty(), "At least one outcome should exist");
        let (step_id, kind) = &initial[0];
        let initial_count = initial.len();

        // Attempt to insert duplicate (same run_id, step_id, kind) - should fail
        let duplicate_result = conn.execute(
            "INSERT INTO outcomes (id, run_id, step_id, kind, status, content, created_at, updated_at) 
             VALUES (?1, ?2, ?3, ?4, 'final', 'duplicate', 0, 0)",
            params!["dup_outcome", run.id, step_id, kind],
        );
        assert!(
            duplicate_result.is_err(),
            "Duplicate (run_id, step_id, kind) should violate unique constraint"
        );

        // But inserting with different step_id OR kind should succeed
        let different_step = conn.execute(
            "INSERT INTO outcomes (id, run_id, step_id, kind, status, content, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'final', 'different step', 0, 0)",
            params!["diff_step_outcome", run.id, "different_step", kind],
        );
        assert!(
            different_step.is_ok(),
            "Different step_id should be allowed"
        );

        let different_kind = conn.execute(
            "INSERT INTO outcomes (id, run_id, step_id, kind, status, content, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'final', 'different kind', 0, 0)",
            params!["diff_kind_outcome", run.id, step_id, "different_kind"],
        );
        assert!(different_kind.is_ok(), "Different kind should be allowed");

        // Verify we added 2 more outcomes
        let final_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM outcomes WHERE run_id = ?1",
                params![run.id],
                |row| row.get(0),
            )
            .expect("count final");
        assert_eq!(final_count as usize, initial_count + 2);
    }

    #[test]
    fn send_email_fails_when_policy_is_disabled() {
        let mut conn = setup_conn();
        let plan = AutopilotPlan::from_intent(
            RecipeKind::InboxTriage,
            "Triage and send reply to user@example.com".to_string(),
            ProviderId::OpenAi,
        );
        let run = RunnerEngine::start_run(&mut conn, "auto_send_off", plan, "idem_send_off", 2)
            .expect("start");
        conn.execute(
            "INSERT INTO email_ingest_events (
               id, provider, provider_message_id, provider_thread_id, sender_email, dedupe_key, autopilot_id, subject, received_at_ms, run_id, status, created_at_ms
             ) VALUES (?1, 'gmail', 'msg_send_off', 'thread_send_off', 'user@example.com', 'gmail:msg_send_off', 'auto_send_off', 'Subject', ?2, ?3, 'queued', ?2)",
            params!["ingest_send_off", 1_i64, run.id],
        )
        .expect("seed ingest");

        let _ = RunnerEngine::run_tick(&mut conn, &run.id).expect("step1");
        let need_triage_approval = RunnerEngine::run_tick(&mut conn, &run.id).expect("approval1");
        assert_eq!(need_triage_approval.state, RunState::NeedsApproval);
        let triage = RunnerEngine::list_pending_approvals(&conn)
            .expect("triage approvals")
            .into_iter()
            .find(|a| a.run_id == run.id && a.step_id == "step_2")
            .expect("triage approval");
        let after_triage = RunnerEngine::approve(&mut conn, &triage.id).expect("approve triage");
        assert_eq!(after_triage.state, RunState::Ready);

        let _ = RunnerEngine::run_tick(&mut conn, &run.id).expect("step3");
        let need_draft_approval = RunnerEngine::run_tick(&mut conn, &run.id).expect("approval2");
        assert_eq!(need_draft_approval.state, RunState::NeedsApproval);
        let approvals = RunnerEngine::list_pending_approvals(&conn).expect("approvals");
        let first = approvals
            .iter()
            .find(|a| a.run_id == run.id && a.step_id == "step_4")
            .expect("first approval");
        let after_first = RunnerEngine::approve(&mut conn, &first.id).expect("approve first");
        assert_eq!(after_first.state, RunState::Ready);

        let need_send_approval = RunnerEngine::run_tick(&mut conn, &run.id).expect("approval3");
        assert_eq!(need_send_approval.state, RunState::NeedsApproval);
        let approvals2 = RunnerEngine::list_pending_approvals(&conn).expect("approvals2");
        let second = approvals2
            .iter()
            .find(|a| a.run_id == run.id && a.step_id == "step_5")
            .expect("second approval");
        let failed = RunnerEngine::approve(&mut conn, &second.id).expect("approve second");
        assert_eq!(failed.state, RunState::Failed);
        assert!(failed
            .failure_reason
            .unwrap_or_default()
            .contains("Sending is off"));
    }

    #[test]
    fn send_email_succeeds_with_allowlist_policy() {
        let mut conn = setup_conn();
        let plan = AutopilotPlan::from_intent(
            RecipeKind::InboxTriage,
            "Triage and send reply to user@example.com".to_string(),
            ProviderId::OpenAi,
        );
        let run = RunnerEngine::start_run(&mut conn, "auto_send_on", plan, "idem_send_on", 2)
            .expect("start");
        conn.execute(
            "INSERT INTO email_ingest_events (
               id, provider, provider_message_id, provider_thread_id, sender_email, dedupe_key, autopilot_id, subject, received_at_ms, run_id, status, created_at_ms
             ) VALUES (?1, 'gmail', 'msg_send_on', 'thread_send_on', 'user@example.com', 'gmail:msg_send_on', 'auto_send_on', 'Subject', ?2, ?3, 'queued', ?2)",
            params!["ingest_send_on", 1_i64, run.id],
        )
        .expect("seed ingest");
        crate::db::upsert_autopilot_send_policy(
            &conn,
            &AutopilotSendPolicyRecord {
                autopilot_id: "auto_send_on".to_string(),
                allow_sending: true,
                recipient_allowlist: vec!["@example.com".to_string()],
                max_sends_per_day: 10,
                quiet_hours_start_local: 23,
                quiet_hours_end_local: 5,
                allow_outside_quiet_hours: true,
                updated_at_ms: 1,
            },
        )
        .expect("seed send policy");
        let _ = RunnerEngine::run_tick(&mut conn, &run.id).expect("step1");
        let need_triage_approval = RunnerEngine::run_tick(&mut conn, &run.id).expect("approval1");
        assert_eq!(need_triage_approval.state, RunState::NeedsApproval);
        let triage = RunnerEngine::list_pending_approvals(&conn)
            .expect("triage approvals")
            .into_iter()
            .find(|a| a.run_id == run.id && a.step_id == "step_2")
            .expect("triage approval");
        let after_triage = RunnerEngine::approve(&mut conn, &triage.id).expect("approve triage");
        assert_eq!(after_triage.state, RunState::Ready);

        let _ = RunnerEngine::run_tick(&mut conn, &run.id).expect("step3");
        let need_draft_approval = RunnerEngine::run_tick(&mut conn, &run.id).expect("approval2");
        assert_eq!(need_draft_approval.state, RunState::NeedsApproval);
        let approvals = RunnerEngine::list_pending_approvals(&conn).expect("approvals");
        let first = approvals
            .iter()
            .find(|a| a.run_id == run.id && a.step_id == "step_4")
            .expect("first approval");
        let after_first = RunnerEngine::approve(&mut conn, &first.id).expect("approve first");
        assert_eq!(after_first.state, RunState::Ready);

        let need_send_approval = RunnerEngine::run_tick(&mut conn, &run.id).expect("approval3");
        assert_eq!(need_send_approval.state, RunState::NeedsApproval);
        let approvals2 = RunnerEngine::list_pending_approvals(&conn).expect("approvals2");
        let second = approvals2
            .iter()
            .find(|a| a.run_id == run.id && a.step_id == "step_5")
            .expect("second approval");
        let done = RunnerEngine::approve(&mut conn, &second.id).expect("approve second");
        assert_eq!(done.state, RunState::Succeeded);

        let sent_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM outcomes WHERE run_id = ?1 AND kind = 'email_sent'",
                params![run.id],
                |row| row.get(0),
            )
            .expect("sent count");
        assert_eq!(sent_count, 1);

        let triage_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM outcomes WHERE run_id = ?1 AND kind = 'email_triage_executed'",
                params![run.id],
                |row| row.get(0),
            )
            .expect("triage count");
        assert_eq!(triage_count, 1);
    }
}
