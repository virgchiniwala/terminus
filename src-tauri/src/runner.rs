use crate::primitives::PrimitiveGuard;
use crate::schema::{AutopilotPlan, PlanStep, PrimitiveId};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

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
            Self::Canceled => "canceled",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Canceled)
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
    pub state: RunState,
    pub current_step_index: i64,
    pub retry_count: i64,
    pub max_retries: i64,
    pub next_retry_backoff_ms: Option<i64>,
    pub next_retry_at_ms: Option<i64>,
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
        let plan_json = serde_json::to_string(&plan).map_err(|e| RunnerError::Serde(e.to_string()))?;

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
              id, autopilot_id, idempotency_key, plan_json, state,
              current_step_index, retry_count, max_retries,
              next_retry_backoff_ms, next_retry_at_ms,
              failure_reason, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, ?6, NULL, NULL, NULL, ?7, ?7)
            ",
            params![
                run_id,
                autopilot_id,
                idempotency_key,
                plan_json,
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

    pub fn approve(connection: &mut Connection, approval_id: &str) -> Result<RunRecord, RunnerError> {
        let approval = Self::get_approval(connection, approval_id)?;
        if approval.status != "pending" {
            return Err(RunnerError::Human("Approval is no longer pending.".to_string()));
        }

        let tx = connection
            .transaction()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let now = now_ms();

        tx.execute(
            "
            UPDATE approvals
            SET status = 'approved', updated_at = ?1, decided_at = ?1
            WHERE id = ?2
            ",
            params![now, approval_id],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.execute(
            "
            UPDATE runs
            SET state = 'ready', failure_reason = NULL, next_retry_backoff_ms = NULL,
                next_retry_at_ms = NULL, updated_at = ?1
            WHERE id = ?2
            ",
            params![now, approval.run_id],
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
                "Approval granted. Run is ready for the next tick.",
                now
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.commit().map_err(|e| RunnerError::Db(e.to_string()))?;
        Self::run_tick_internal(connection, &approval.run_id, Some(&approval.step_id))
    }

    pub fn reject(
        connection: &mut Connection,
        approval_id: &str,
        reason: Option<String>,
    ) -> Result<RunRecord, RunnerError> {
        let approval = Self::get_approval(connection, approval_id)?;
        if approval.status != "pending" {
            return Err(RunnerError::Human("Approval is no longer pending.".to_string()));
        }

        let reject_reason = reason.unwrap_or_else(|| "Approval was rejected by the user.".to_string());
        let tx = connection
            .transaction()
            .map_err(|e| RunnerError::Db(e.to_string()))?;
        let now = now_ms();

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
            SET state = 'canceled', failure_reason = ?1, updated_at = ?2
            WHERE id = ?3
            ",
            params![reject_reason, now, approval.run_id],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.execute(
            "
            INSERT INTO activities (id, run_id, activity_type, from_state, to_state, user_message, created_at)
            VALUES (?1, ?2, 'approval_rejected', 'needs_approval', 'canceled', ?3, ?4)
            ",
            params![make_id("activity"), approval.run_id, reject_reason, now],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.commit().map_err(|e| RunnerError::Db(e.to_string()))?;
        Self::get_run(connection, &approval.run_id)
    }

    pub fn list_pending_approvals(connection: &Connection) -> Result<Vec<ApprovalRecord>, RunnerError> {
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
                SELECT id, autopilot_id, idempotency_key, state,
                       current_step_index, retry_count, max_retries,
                       next_retry_backoff_ms, next_retry_at_ms,
                       failure_reason, plan_json
                FROM runs
                WHERE id = ?1
                ",
                params![run_id],
                |row| {
                    let state_text: String = row.get(3)?;
                    let plan_json: String = row.get(10)?;
                    let plan: AutopilotPlan = serde_json::from_str(&plan_json)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                    Ok(RunRecord {
                        id: row.get(0)?,
                        autopilot_id: row.get(1)?,
                        idempotency_key: row.get(2)?,
                        state: RunState::from_str(&state_text)
                            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                        current_step_index: row.get(4)?,
                        retry_count: row.get(5)?,
                        max_retries: row.get(6)?,
                        next_retry_backoff_ms: row.get(7)?,
                        next_retry_at_ms: row.get(8)?,
                        failure_reason: row.get(9)?,
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

    fn run_tick_internal(
        connection: &mut Connection,
        run_id: &str,
        approved_step_id: Option<&str>,
    ) -> Result<RunRecord, RunnerError> {
        let run = Self::get_run(connection, run_id)?;

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
            return Self::get_run(connection, run_id);
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
            return Self::get_run(connection, run_id);
        }

        match Self::execute_step(connection, &run, &step) {
            Ok(message) => {
                let next_idx = (current_idx as i64) + 1;
                let next_state = if next_idx as usize >= run.plan.steps.len() {
                    RunState::Succeeded
                } else {
                    RunState::Ready
                };
                let activity = if next_state == RunState::Succeeded {
                    "run_succeeded"
                } else {
                    "step_completed"
                };

                Self::transition_state_with_activity(
                    connection,
                    run_id,
                    run.state,
                    next_state,
                    activity,
                    &message,
                    None,
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
                        run.state,
                        next_retry,
                        backoff_ms,
                        next_retry_at_ms,
                        &error.user_reason,
                    )?;
                    return Self::get_run(connection, run_id);
                }

                Self::transition_state_with_activity(
                    connection,
                    run_id,
                    run.state,
                    RunState::Failed,
                    "run_failed",
                    &error.user_reason,
                    Some(&error.user_reason),
                    Some(current_idx as i64),
                )?;
            }
        }

        Self::get_run(connection, run_id)
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

    fn get_approval(connection: &Connection, approval_id: &str) -> Result<ApprovalRecord, RunnerError> {
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

    fn execute_step(
        connection: &Connection,
        run: &RunRecord,
        step: &PlanStep,
    ) -> Result<String, StepExecutionError> {
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
                user_reason: "Sending is disabled right now. Drafts are allowed, sends are blocked."
                    .to_string(),
            });
        }

        if run.plan.intent.contains("simulate_retryable_failure") {
            return Err(StepExecutionError {
                retryable: true,
                user_reason: "Source is temporarily unavailable.".to_string(),
            });
        }

        match step.primitive {
            PrimitiveId::WriteOutcomeDraft => {
                connection
                    .execute(
                        "
                        INSERT INTO outcomes (
                          id, run_id, step_id, kind, status, content,
                          created_at, updated_at
                        ) VALUES (?1, ?2, ?3, 'outcome_draft', 'drafted', ?4, ?5, ?5)
                        ON CONFLICT(run_id, step_id, kind) DO NOTHING
                        ",
                        params![
                            make_id("outcome"),
                            run.id,
                            step.id,
                            format!("Draft outcome for step: {}", step.label),
                            now_ms()
                        ],
                    )
                    .map_err(|_| StepExecutionError {
                        retryable: true,
                        user_reason: "Couldn't write the draft outcome yet.".to_string(),
                    })?;

                Ok("Draft outcome saved.".to_string())
            }
            PrimitiveId::WriteEmailDraft => {
                connection
                    .execute(
                        "
                        INSERT INTO outcomes (
                          id, run_id, step_id, kind, status, content,
                          created_at, updated_at
                        ) VALUES (?1, ?2, ?3, 'email_draft', 'drafted', ?4, ?5, ?5)
                        ON CONFLICT(run_id, step_id, kind) DO NOTHING
                        ",
                        params![
                            make_id("outcome"),
                            run.id,
                            step.id,
                            format!("Draft email for step: {}", step.label),
                            now_ms()
                        ],
                    )
                    .map_err(|_| StepExecutionError {
                        retryable: true,
                        user_reason: "Couldn't write the draft email yet.".to_string(),
                    })?;

                Ok("Draft email created and queued for approval.".to_string())
            }
            PrimitiveId::ReadWeb
            | PrimitiveId::ReadForwardedEmail
            | PrimitiveId::ReadVaultFile
            | PrimitiveId::ScheduleRun
            | PrimitiveId::NotifyUser => Ok("Step completed.".to_string()),
            PrimitiveId::SendEmail => Err(StepExecutionError {
                retryable: false,
                user_reason: "Sending is disabled right now. Drafts are allowed, sends are blocked."
                    .to_string(),
            }),
        }
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
            params![retry_count, backoff_ms, next_retry_at_ms, reason, now, run_id],
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
                format!("Retry scheduled in {} ms. {}", backoff_ms, reason),
                now
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.commit().map_err(|e| RunnerError::Db(e.to_string()))
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
                user_message,
                now
            ],
        )
        .map_err(|e| RunnerError::Db(e.to_string()))?;

        tx.commit().map_err(|e| RunnerError::Db(e.to_string()))
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

fn compute_backoff_ms(retry_attempt: u32) -> u32 {
    let base: u32 = 200;
    base.saturating_mul(2u32.saturating_pow(retry_attempt.saturating_sub(1))).min(2_000)
}

#[cfg(test)]
mod tests {
    use super::{ApprovalRecord, RunState, RunnerEngine};
    use crate::db::bootstrap_schema;
    use crate::schema::{AutopilotPlan, PlanStep, PrimitiveId, ProviderId, RecipeKind, RiskTier};
    use rusqlite::{params, Connection};

    fn setup_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open memory db");
        bootstrap_schema(&mut conn).expect("bootstrap schema");
        conn
    }

    fn plan_with_single_write_step(intent: &str) -> AutopilotPlan {
        AutopilotPlan {
            schema_version: "1.0".to_string(),
            recipe: RecipeKind::WebsiteMonitor,
            intent: intent.to_string(),
            provider: crate::schema::ProviderMetadata::from_provider_id(ProviderId::OpenAi),
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

    #[test]
    fn idempotency_key_reuses_existing_run_and_prevents_duplicate_outcomes() {
        let mut conn = setup_conn();
        let plan = plan_with_single_write_step("idempotency test");

        let first = RunnerEngine::start_run(&mut conn, "auto_1", plan.clone(), "idem_1", 2)
            .expect("first run starts");
        let second = RunnerEngine::start_run(&mut conn, "auto_1", plan, "idem_1", 2)
            .expect("second run reuses");

        assert_eq!(first.id, second.id);
        let completed = RunnerEngine::run_tick(&mut conn, &first.id).expect("tick");
        assert_eq!(completed.state, RunState::Succeeded);

        let outcome_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM outcomes WHERE run_id = ?1",
                params![first.id],
                |row| row.get(0),
            )
            .expect("count outcomes");
        assert_eq!(outcome_count, 1);
    }

    #[test]
    fn retry_scheduling_does_not_retry_immediately_and_resumes_when_due() {
        let mut conn = setup_conn();
        let mut plan = plan_with_single_write_step("simulate_retryable_failure");
        plan.allowed_primitives = vec![PrimitiveId::ReadWeb];
        plan.steps = vec![PlanStep {
            id: "step_1".to_string(),
            label: "Read source".to_string(),
            primitive: PrimitiveId::ReadWeb,
            requires_approval: false,
            risk_tier: RiskTier::Low,
        }];

        let run = RunnerEngine::start_run(&mut conn, "auto_2", plan, "idem_retry", 1)
            .expect("run starts ready");
        let scheduled = RunnerEngine::run_tick(&mut conn, &run.id).expect("first tick schedules retry");
        assert_eq!(scheduled.state, RunState::Retrying);
        assert_eq!(scheduled.retry_count, 1);
        assert!(scheduled.next_retry_at_ms.is_some());

        let still_waiting = RunnerEngine::run_tick(&mut conn, &run.id).expect("not due yet");
        assert_eq!(still_waiting.state, RunState::Retrying);

        conn.execute(
            "UPDATE runs SET next_retry_at_ms = ?1 WHERE id = ?2",
            params![0_i64, run.id],
        )
        .expect("force due");

        let resumed = RunnerEngine::resume_due_runs(&mut conn, 10).expect("resume due");
        assert_eq!(resumed.len(), 1);
        assert_eq!(resumed[0].state, RunState::Failed);
        assert_eq!(
            resumed[0].failure_reason.as_deref(),
            Some("Source is temporarily unavailable.")
        );
    }

    #[test]
    fn transition_and_activity_are_atomic_in_single_transaction() {
        let mut conn = setup_conn();
        let plan = plan_with_single_write_step("atomicity test");
        let run = RunnerEngine::start_run(&mut conn, "auto_3", plan, "idem_atomic", 1)
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

        let forced_events: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM activities WHERE run_id = ?1 AND activity_type = 'forced_test'",
                params![run.id],
                |row| row.get(0),
            )
            .expect("count forced events");
        assert_eq!(forced_events, 0);
    }

    #[test]
    fn retry_metadata_and_activity_are_atomic() {
        let mut conn = setup_conn();
        let plan = plan_with_single_write_step("atomic retry test");
        let run = RunnerEngine::start_run(&mut conn, "auto_5", plan, "idem_atomic_retry", 2)
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
    fn approval_flow_still_works_with_tick_execution() {
        let mut conn = setup_conn();
        let plan = AutopilotPlan {
            schema_version: "1.0".to_string(),
            recipe: RecipeKind::WebsiteMonitor,
            intent: "needs approval".to_string(),
            provider: crate::schema::ProviderMetadata::from_provider_id(ProviderId::OpenAi),
            allowed_primitives: vec![PrimitiveId::WriteEmailDraft],
            steps: vec![PlanStep {
                id: "step_1".to_string(),
                label: "Draft email".to_string(),
                primitive: PrimitiveId::WriteEmailDraft,
                requires_approval: true,
                risk_tier: RiskTier::Medium,
            }],
        };

        let run = RunnerEngine::start_run(&mut conn, "auto_4", plan, "idem_approval", 1)
            .expect("run starts");
        let paused = RunnerEngine::run_tick(&mut conn, &run.id).expect("run pauses");
        assert_eq!(paused.state, RunState::NeedsApproval);

        let pending: Vec<ApprovalRecord> =
            RunnerEngine::list_pending_approvals(&conn).expect("list approvals");
        assert_eq!(pending.len(), 1);

        let resumed = RunnerEngine::approve(&mut conn, &pending[0].id).expect("approve resumes");
        assert_eq!(resumed.state, RunState::Succeeded);
    }
}
