use crate::db;
use crate::learning;
use crate::runner::{RunState, RunnerEngine};
use crate::schema::{AutopilotPlan, ProviderId, ProviderMetadata, RecipeKind};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static DIAG_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunHealthStatus {
    HealthyRunning,
    WaitingForApproval,
    WaitingForClarification,
    RetryingTransient,
    RetryingStuck,
    PolicyBlocked,
    ProviderMisconfigured,
    SourceUnreachable,
    ResourceThrottled,
    Completed,
    FailedUnclassified,
}

impl RunHealthStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::HealthyRunning => "healthy_running",
            Self::WaitingForApproval => "waiting_for_approval",
            Self::WaitingForClarification => "waiting_for_clarification",
            Self::RetryingTransient => "retrying_transient",
            Self::RetryingStuck => "retrying_stuck",
            Self::PolicyBlocked => "policy_blocked",
            Self::ProviderMisconfigured => "provider_misconfigured",
            Self::SourceUnreachable => "source_unreachable",
            Self::ResourceThrottled => "resource_throttled",
            Self::Completed => "completed",
            Self::FailedUnclassified => "failed_unclassified",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterventionSuggestion {
    pub kind: String,
    pub label: String,
    pub reason: String,
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDiagnosticRecord {
    pub id: String,
    pub run_id: String,
    pub autopilot_id: String,
    pub run_state: String,
    pub health_status: String,
    pub reason_code: String,
    pub summary: String,
    pub suggestions: Vec<InterventionSuggestion>,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyInterventionInput {
    pub run_id: String,
    pub kind: String,
    pub answer_text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyInterventionResult {
    pub ok: bool,
    pub run_id: String,
    pub message: String,
    pub updated_run_state: Option<String>,
}

#[derive(Debug, Clone)]
struct RunDiagnosticSeed {
    run_id: String,
    autopilot_id: String,
    state: String,
    retry_count: i64,
    max_retries: i64,
    next_retry_at_ms: Option<i64>,
    failure_reason: Option<String>,
    provider_kind: String,
    provider_tier: String,
    plan: AutopilotPlan,
    pending_approval_id: Option<String>,
    pending_clarification_id: Option<String>,
}

pub fn list_run_diagnostics(
    connection: &Connection,
    limit: usize,
) -> Result<Vec<RunDiagnosticRecord>, String> {
    let seeds = load_run_diagnostic_seeds(connection, limit)?;
    let mut out = Vec::new();
    for seed in seeds {
        let record = derive_run_diagnostic(&seed);
        let _ = persist_diagnostic_snapshot(connection, &record);
        out.push(record);
    }
    Ok(out)
}

pub fn apply_intervention(
    connection: &mut Connection,
    input: ApplyInterventionInput,
) -> Result<ApplyInterventionResult, String> {
    let run_id = input.run_id.trim().to_string();
    if run_id.is_empty() {
        return Err("Run ID is required.".to_string());
    }
    let kind = input.kind.trim().to_string();
    if kind.is_empty() {
        return Err("Intervention kind is required.".to_string());
    }

    let run = RunnerEngine::get_run(connection, &run_id).map_err(|e| e.to_string())?;
    let mut updated_state = None;
    let message = match kind.as_str() {
        "approve_pending_action" => {
            let approval_id: Option<String> = connection
                .query_row(
                    "SELECT id FROM approvals WHERE run_id = ?1 AND status = 'pending' ORDER BY created_at ASC LIMIT 1",
                    params![&run_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| format!("Failed to load pending approval: {e}"))?;
            let Some(approval_id) = approval_id else {
                return Err("No pending approval found for this run.".to_string());
            };
            let updated =
                RunnerEngine::approve(connection, &approval_id).map_err(|e| e.to_string())?;
            updated_state = Some(updated.state.as_str().to_string());
            "Approved the pending action and resumed the run.".to_string()
        }
        "answer_clarification" => {
            let clarification_id: Option<String> = connection
                .query_row(
                    "SELECT id FROM clarifications WHERE run_id = ?1 AND status = 'pending' ORDER BY created_at_ms ASC LIMIT 1",
                    params![&run_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| format!("Failed to load pending clarification: {e}"))?;
            let Some(clarification_id) = clarification_id else {
                return Err("No pending clarification found for this run.".to_string());
            };
            let answer = input
                .answer_text
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| {
                    "Add an answer in the Clarifications panel, then retry.".to_string()
                })?;
            let payload = serde_json::json!({ "value": answer });
            let updated = RunnerEngine::submit_clarification_answer(
                connection,
                &clarification_id,
                &payload.to_string(),
            )
            .map_err(|e| e.to_string())?;
            updated_state = Some(updated.state.as_str().to_string());
            "Answered the clarification and resumed the run.".to_string()
        }
        "retry_now_if_due" => {
            if run.state == RunState::Retrying {
                if let Some(next_retry_at_ms) = run.next_retry_at_ms {
                    if next_retry_at_ms > now_ms() {
                        return Err(
                            "Retry is not due yet. Terminus will resume it automatically."
                                .to_string(),
                        );
                    }
                }
            }
            if run.state.is_terminal() {
                return Err("Terminal runs cannot be retried from this shortcut.".to_string());
            }
            let updated = RunnerEngine::run_tick(connection, &run_id).map_err(|e| e.to_string())?;
            updated_state = Some(updated.state.as_str().to_string());
            "Triggered one bounded retry/resume tick.".to_string()
        }
        "pause_autopilot_15m" => {
            let until = now_ms() + 15 * 60 * 1000;
            learning::set_autopilot_suppression_until(connection, &run.autopilot_id, Some(until))
                .map_err(|e| e.to_string())?;
            format!(
                "Paused Autopilot learning notifications for 15 minutes (until {}).",
                until
            )
        }
        "reduce_source_scope" => {
            let mut plan = run.plan.clone();
            if plan.recipe != RecipeKind::DailyBrief || plan.daily_sources.len() <= 3 {
                return Err("This run does not have a reducible source set.".to_string());
            }
            plan.daily_sources.truncate(3);
            persist_run_plan(connection, &run_id, &plan)?;
            "Reduced daily brief sources to the first 3 and saved the run plan.".to_string()
        }
        "switch_provider_supported_default" => {
            let mut plan = run.plan.clone();
            plan.provider = ProviderMetadata::from_provider_id(ProviderId::OpenAi);
            persist_run_plan(connection, &run_id, &plan)?;
            connection
                .execute(
                    "UPDATE runs
                     SET provider_kind = 'openai',
                         provider_tier = 'supported',
                         updated_at = ?2
                     WHERE id = ?1",
                    params![&run_id, now_ms()],
                )
                .map_err(|e| format!("Failed to switch provider for run: {e}"))?;
            "Switched the run to the supported OpenAI default provider.".to_string()
        }
        "open_receipt" => "Receipt is available in the run details panel.".to_string(),
        "open_activity_log" => "Open the Activity view to inspect this run timeline.".to_string(),
        _ => return Err("That intervention is not allowed in Terminus.".to_string()),
    };

    log_intervention(connection, &run_id, &run.autopilot_id, &kind, &message)?;
    Ok(ApplyInterventionResult {
        ok: true,
        run_id,
        message,
        updated_run_state: updated_state,
    })
}

fn load_run_diagnostic_seeds(
    connection: &Connection,
    limit: usize,
) -> Result<Vec<RunDiagnosticSeed>, String> {
    let mut stmt = connection
        .prepare(
            "
            SELECT
              r.id,
              r.autopilot_id,
              r.state,
              r.retry_count,
              r.max_retries,
              r.next_retry_at_ms,
              r.failure_reason,
              r.provider_kind,
              r.provider_tier,
              r.plan_json,
              (
                SELECT a.id FROM approvals a
                WHERE a.run_id = r.id AND a.status = 'pending'
                ORDER BY a.created_at ASC LIMIT 1
              ) AS pending_approval_id,
              (
                SELECT c.id FROM clarifications c
                WHERE c.run_id = r.id AND c.status = 'pending'
                ORDER BY c.created_at_ms ASC LIMIT 1
              ) AS pending_clarification_id
            FROM runs r
            ORDER BY r.updated_at DESC
            LIMIT ?1
            ",
        )
        .map_err(|e| format!("Failed to prepare diagnostics query: {e}"))?;
    let rows = stmt
        .query_map(params![limit as i64], |row| {
            let plan_json: String = row.get(9)?;
            let plan = serde_json::from_str::<AutopilotPlan>(&plan_json)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            Ok(RunDiagnosticSeed {
                run_id: row.get(0)?,
                autopilot_id: row.get(1)?,
                state: row.get(2)?,
                retry_count: row.get(3)?,
                max_retries: row.get(4)?,
                next_retry_at_ms: row.get(5)?,
                failure_reason: row.get(6)?,
                provider_kind: row.get(7)?,
                provider_tier: row.get(8)?,
                plan,
                pending_approval_id: row.get(10)?,
                pending_clarification_id: row.get(11)?,
            })
        })
        .map_err(|e| format!("Failed to query diagnostics: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("Failed to parse diagnostics row: {e}"))?);
    }
    Ok(out)
}

fn derive_run_diagnostic(seed: &RunDiagnosticSeed) -> RunDiagnosticRecord {
    let state = seed.state.as_str();
    let failure = seed.failure_reason.clone().unwrap_or_default();
    let failure_lower = failure.to_ascii_lowercase();
    let (health, reason_code, summary) =
        if state == "needs_approval" || seed.pending_approval_id.is_some() {
            (
                RunHealthStatus::WaitingForApproval,
                "approval_pending".to_string(),
                "A write/send action is waiting for your approval.".to_string(),
            )
        } else if state == "needs_clarification" || seed.pending_clarification_id.is_some() {
            (
                RunHealthStatus::WaitingForClarification,
                "clarification_pending".to_string(),
                "One missing detail is blocking progress until you answer.".to_string(),
            )
        } else if matches!(state, "succeeded" | "canceled") {
            (
                RunHealthStatus::Completed,
                "terminal_complete".to_string(),
                "Run reached a terminal state.".to_string(),
            )
        } else if state == "retrying" && is_rate_limited(&failure_lower) {
            (
                RunHealthStatus::ResourceThrottled,
                "provider_rate_limited".to_string(),
                "Provider or source is throttling requests. Terminus will retry with backoff."
                    .to_string(),
            )
        } else if state == "retrying" {
            if seed.retry_count >= 2 || seed.retry_count >= seed.max_retries.saturating_sub(1) {
                (
                    RunHealthStatus::RetryingStuck,
                    "retrying_stuck".to_string(),
                    "The run is retrying repeatedly and may need intervention.".to_string(),
                )
            } else {
                (
                    RunHealthStatus::RetryingTransient,
                    "retrying_transient".to_string(),
                    "The run hit a retryable failure and is waiting for the next retry window."
                        .to_string(),
                )
            }
        } else if matches!(state, "failed" | "blocked") && is_provider_auth(&failure_lower) {
            (
                RunHealthStatus::ProviderMisconfigured,
                "provider_auth".to_string(),
                "Provider credentials or configuration look invalid.".to_string(),
            )
        } else if matches!(state, "failed" | "blocked") && is_source_unreachable(&failure_lower) {
            (
                RunHealthStatus::SourceUnreachable,
                "source_unreachable".to_string(),
                "A configured web/source input could not be reached.".to_string(),
            )
        } else if matches!(state, "failed" | "blocked") && is_policy_blocked(&failure_lower) {
            (
                RunHealthStatus::PolicyBlocked,
                "policy_block".to_string(),
                "Terminus blocked an action due to a safety or policy rule.".to_string(),
            )
        } else if matches!(state, "failed" | "blocked") {
            (
                RunHealthStatus::FailedUnclassified,
                "failed_unclassified".to_string(),
                if failure.is_empty() {
                    "The run failed for a reason that could not be classified yet.".to_string()
                } else {
                    truncate_summary(&failure)
                },
            )
        } else {
            (
                RunHealthStatus::HealthyRunning,
                "in_progress".to_string(),
                "Run is progressing within normal bounds.".to_string(),
            )
        };

    let suggestions = build_suggestions(seed, health);
    RunDiagnosticRecord {
        id: make_id("diag"),
        run_id: seed.run_id.clone(),
        autopilot_id: seed.autopilot_id.clone(),
        run_state: seed.state.clone(),
        health_status: health.as_str().to_string(),
        reason_code,
        summary,
        suggestions,
        created_at_ms: now_ms(),
    }
}

fn build_suggestions(
    seed: &RunDiagnosticSeed,
    health: RunHealthStatus,
) -> Vec<InterventionSuggestion> {
    let mut suggestions = Vec::new();

    match health {
        RunHealthStatus::WaitingForApproval => suggestions.push(InterventionSuggestion {
            kind: "approve_pending_action".to_string(),
            label: "Approve Pending Action".to_string(),
            reason: "Resume the run by approving the oldest pending action.".to_string(),
            disabled: seed.pending_approval_id.is_none(),
        }),
        RunHealthStatus::WaitingForClarification => suggestions.push(InterventionSuggestion {
            kind: "answer_clarification".to_string(),
            label: "Answer Clarification".to_string(),
            reason: "Use the Clarifications panel to answer and resume the run.".to_string(),
            disabled: seed.pending_clarification_id.is_none(),
        }),
        RunHealthStatus::RetryingTransient
        | RunHealthStatus::RetryingStuck
        | RunHealthStatus::ResourceThrottled => {
            suggestions.push(InterventionSuggestion {
                kind: "retry_now_if_due".to_string(),
                label: "Retry Now (If Due)".to_string(),
                reason: "Trigger one bounded resume tick when the retry window is due.".to_string(),
                disabled: seed.next_retry_at_ms.map(|t| t > now_ms()).unwrap_or(false),
            });
            suggestions.push(InterventionSuggestion {
                kind: "pause_autopilot_15m".to_string(),
                label: "Pause Autopilot 15m".to_string(),
                reason: "Temporarily suppress noisy retries while you investigate.".to_string(),
                disabled: false,
            });
        }
        RunHealthStatus::ProviderMisconfigured => suggestions.push(InterventionSuggestion {
            kind: "switch_provider_supported_default".to_string(),
            label: "Switch Provider".to_string(),
            reason: "Switch the run to the supported OpenAI default provider.".to_string(),
            disabled: seed.provider_tier == "supported" && seed.provider_kind == "openai",
        }),
        RunHealthStatus::SourceUnreachable => {
            if seed.plan.recipe == RecipeKind::DailyBrief && seed.plan.daily_sources.len() > 3 {
                suggestions.push(InterventionSuggestion {
                    kind: "reduce_source_scope".to_string(),
                    label: "Reduce Source Scope".to_string(),
                    reason: "Trim the daily brief to fewer sources to improve reliability."
                        .to_string(),
                    disabled: false,
                });
            }
        }
        RunHealthStatus::PolicyBlocked => suggestions.push(InterventionSuggestion {
            kind: "open_activity_log".to_string(),
            label: "Review Policy Block".to_string(),
            reason: "Read the activity timeline to see which guardrail blocked the run."
                .to_string(),
            disabled: false,
        }),
        RunHealthStatus::Completed
        | RunHealthStatus::HealthyRunning
        | RunHealthStatus::FailedUnclassified => {}
    }

    suggestions.push(InterventionSuggestion {
        kind: "open_activity_log".to_string(),
        label: "Open Activity Log".to_string(),
        reason: "Inspect the run timeline and receipts for detailed context.".to_string(),
        disabled: false,
    });

    if matches!(
        health,
        RunHealthStatus::Completed
            | RunHealthStatus::FailedUnclassified
            | RunHealthStatus::PolicyBlocked
            | RunHealthStatus::ProviderMisconfigured
            | RunHealthStatus::SourceUnreachable
    ) {
        suggestions.push(InterventionSuggestion {
            kind: "open_receipt".to_string(),
            label: "Open Receipt".to_string(),
            reason: "Review the terminal receipt and recovery options for this run.".to_string(),
            disabled: false,
        });
    }

    suggestions
}

fn persist_diagnostic_snapshot(
    connection: &Connection,
    record: &RunDiagnosticRecord,
) -> Result<(), String> {
    let suggestions_json = serde_json::to_string(&record.suggestions)
        .map_err(|e| format!("Failed to serialize suggestions: {e}"))?;
    connection
        .execute(
            "INSERT INTO run_diagnostics (id, run_id, health_status, reason_code, summary, suggestions_json, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                &record.id,
                &record.run_id,
                &record.health_status,
                &record.reason_code,
                &record.summary,
                suggestions_json,
                record.created_at_ms
            ],
        )
        .map_err(|e| format!("Failed to persist run diagnostics: {e}"))?;
    Ok(())
}

fn persist_run_plan(
    connection: &Connection,
    run_id: &str,
    plan: &AutopilotPlan,
) -> Result<(), String> {
    let plan_json =
        serde_json::to_string(plan).map_err(|e| format!("Failed to serialize run plan: {e}"))?;
    connection
        .execute(
            "UPDATE runs SET plan_json = ?2, updated_at = ?3 WHERE id = ?1",
            params![run_id, plan_json, now_ms()],
        )
        .map_err(|e| format!("Failed to update run plan: {e}"))?;
    Ok(())
}

fn log_intervention(
    connection: &Connection,
    run_id: &str,
    autopilot_id: &str,
    kind: &str,
    message: &str,
) -> Result<(), String> {
    let created_at_ms = now_ms();
    let result_json = serde_json::json!({
        "kind": kind,
        "message": message
    })
    .to_string();
    db::insert_guidance_event(
        connection,
        &db::GuidanceEventInsert {
            id: make_id("guide"),
            scope_type: "run".to_string(),
            scope_id: run_id.to_string(),
            autopilot_id: Some(autopilot_id.to_string()),
            run_id: Some(run_id.to_string()),
            approval_id: None,
            outcome_id: None,
            mode: "applied".to_string(),
            instruction: format!("Intervention applied: {kind}"),
            result_json,
            created_at_ms,
        },
    )?;
    let _ = connection.execute(
        "INSERT INTO activities (id, run_id, activity_type, from_state, to_state, user_message, created_at)
         VALUES (?1, ?2, 'intervention_applied', NULL, NULL, ?3, ?4)",
        params![make_id("activity"), run_id, truncate_summary(message), created_at_ms],
    );
    Ok(())
}

fn is_rate_limited(reason: &str) -> bool {
    let reason = reason.to_ascii_lowercase();
    reason.contains("rate-limit")
        || reason.contains("rate limit")
        || reason.contains("429")
        || reason.contains("throttle")
}

fn is_provider_auth(reason: &str) -> bool {
    let reason = reason.to_ascii_lowercase();
    reason.contains("invalid api key")
        || reason.contains("api key")
        || reason.contains("unauthorized")
        || reason.contains("401")
        || reason.contains("model not found")
}

fn is_source_unreachable(reason: &str) -> bool {
    let reason = reason.to_ascii_lowercase();
    reason.contains("could not read")
        || reason.contains("unreachable")
        || reason.contains("timeout")
        || reason.contains("dns")
        || reason.contains("network")
}

fn is_policy_blocked(reason: &str) -> bool {
    let reason = reason.to_ascii_lowercase();
    reason.contains("not allowed")
        || reason.contains("blocked")
        || reason.contains("approval")
        || reason.contains("policy")
        || reason.contains("guard")
}

fn truncate_summary(input: &str) -> String {
    let max = 180;
    let mut out = String::new();
    for ch in input.chars().take(max) {
        out.push(ch);
    }
    if input.chars().count() > max {
        out.push_str("...");
    }
    out
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn make_id(prefix: &str) -> String {
    let counter = DIAG_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{}_{}", now_ms(), counter)
}

#[cfg(test)]
mod tests {
    use super::{is_policy_blocked, is_provider_auth, is_rate_limited, is_source_unreachable};

    #[test]
    fn classifies_reason_patterns() {
        assert!(is_rate_limited("HTTP 429 rate limit"));
        assert!(is_provider_auth("Invalid API key"));
        assert!(is_source_unreachable(
            "Could not read source due to timeout"
        ));
        assert!(is_policy_blocked("This action is not allowed by policy"));
    }
}
