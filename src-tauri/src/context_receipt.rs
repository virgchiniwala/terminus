use crate::db;
use crate::learning;
use crate::runner::{RunReceipt, RunnerEngine};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextReceipt {
    pub run_id: String,
    pub autopilot_id: String,
    pub recipe: String,
    pub provider_kind: String,
    pub provider_tier: String,
    pub run_state: String,
    pub terminal_receipt_found: bool,
    pub sources: Vec<ContextSourceRecord>,
    pub memory_titles_used: Vec<String>,
    pub memory_cards_used: Vec<learning::MemoryCardRecord>,
    pub policy_constraints: PolicyConstraintsView,
    pub runtime_profile_overlay: RuntimeProfileOverlayView,
    pub redaction_flags: Vec<String>,
    pub rationale_codes: Vec<String>,
    pub key_signals: Vec<String>,
    pub provider_calls: Vec<ProviderCallView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSourceRecord {
    pub source_kind: String,
    pub source_id: Option<String>,
    pub url: Option<String>,
    pub status: String,
    pub fetched_at_ms: Option<i64>,
    pub content_hash: Option<String>,
    pub excerpt_chars: Option<usize>,
    pub changed: Option<bool>,
    pub diff_score: Option<f64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyConstraintsView {
    pub deny_by_default_primitives: bool,
    pub allowed_primitives: Vec<String>,
    pub web_allowed_domains: Vec<String>,
    pub approval_required_steps: Vec<String>,
    pub send_policy: SendPolicyView,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendPolicyView {
    pub allow_sending: bool,
    pub recipient_allowlist_count: usize,
    pub max_sends_per_day: i64,
    pub quiet_hours_start_local: i64,
    pub quiet_hours_end_local: i64,
    pub allow_outside_quiet_hours: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileOverlayView {
    pub learning_enabled: bool,
    pub mode: String,
    pub suppress_until_ms: Option<i64>,
    pub min_diff_score_to_notify: f64,
    pub max_sources: usize,
    pub max_bullets: usize,
    pub reply_length_hint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCallView {
    pub provider: String,
    pub model: String,
    pub request_kind: String,
    pub input_chars: Option<i64>,
    pub output_chars: Option<i64>,
    pub latency_ms: Option<i64>,
    pub cost_cents_est: Option<i64>,
    pub created_at_ms: i64,
}

pub fn get_context_receipt(
    connection: &Connection,
    run_id: &str,
) -> Result<ContextReceipt, String> {
    let run = RunnerEngine::get_run(connection, run_id).map_err(|e| e.to_string())?;
    let terminal_receipt =
        RunnerEngine::get_terminal_receipt(connection, run_id).map_err(|e| e.to_string())?;
    let memory_cards = learning::list_memory_cards_for_autopilot(connection, &run.autopilot_id)
        .map_err(|e| e.to_string())?;
    let runtime_profile =
        learning::get_runtime_profile(connection, &run.autopilot_id).map_err(|e| e.to_string())?;
    let send_policy =
        db::get_autopilot_send_policy(connection, &run.autopilot_id).map_err(|e| e.to_string())?;
    let provider_calls = load_provider_calls(connection, run_id)?;
    let sources = load_context_sources(connection, run_id)?;

    let (memory_titles_used, rationale_codes, key_signals, redaction_flags) =
        derive_context_metadata(&terminal_receipt);
    let memory_cards_used = memory_titles_used
        .iter()
        .filter_map(|title| memory_cards.iter().find(|c| c.title == *title).cloned())
        .collect::<Vec<_>>();

    let approval_required_steps = run
        .plan
        .steps
        .iter()
        .filter(|s| s.requires_approval)
        .map(|s| s.label.clone())
        .collect::<Vec<_>>();
    let allowed_primitives = run
        .plan
        .allowed_primitives
        .iter()
        .map(|p| format!("{p:?}"))
        .collect::<Vec<_>>();

    Ok(ContextReceipt {
        run_id: run.id.clone(),
        autopilot_id: run.autopilot_id.clone(),
        recipe: format!("{:?}", run.plan.recipe),
        provider_kind: format!("{:?}", run.provider_kind).to_lowercase(),
        provider_tier: format!("{:?}", run.provider_tier).to_lowercase(),
        run_state: format!("{:?}", run.state).to_lowercase(),
        terminal_receipt_found: terminal_receipt.is_some(),
        sources,
        memory_titles_used,
        memory_cards_used,
        policy_constraints: PolicyConstraintsView {
            deny_by_default_primitives: true,
            allowed_primitives,
            web_allowed_domains: run.plan.web_allowed_domains.clone(),
            approval_required_steps,
            send_policy: SendPolicyView {
                allow_sending: send_policy.allow_sending,
                recipient_allowlist_count: send_policy.recipient_allowlist.len(),
                max_sends_per_day: send_policy.max_sends_per_day,
                quiet_hours_start_local: send_policy.quiet_hours_start_local,
                quiet_hours_end_local: send_policy.quiet_hours_end_local,
                allow_outside_quiet_hours: send_policy.allow_outside_quiet_hours,
            },
        },
        runtime_profile_overlay: RuntimeProfileOverlayView {
            learning_enabled: runtime_profile.learning_enabled,
            mode: format!("{:?}", runtime_profile.mode).to_lowercase(),
            suppress_until_ms: runtime_profile.suppress_until_ms,
            min_diff_score_to_notify: runtime_profile.min_diff_score_to_notify,
            max_sources: runtime_profile.max_sources,
            max_bullets: runtime_profile.max_bullets,
            reply_length_hint: runtime_profile.reply_length_hint,
        },
        redaction_flags,
        rationale_codes,
        key_signals,
        provider_calls,
    })
}

fn derive_context_metadata(
    terminal_receipt: &Option<RunReceipt>,
) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
    let mut redaction_flags = vec![
        "memory_content_omitted".to_string(),
        "source_content_excerpts_omitted".to_string(),
    ];
    let Some(receipt) = terminal_receipt else {
        redaction_flags.push("terminal_receipt_missing".to_string());
        return (Vec::new(), Vec::new(), Vec::new(), redaction_flags);
    };
    if receipt.redacted {
        redaction_flags.push("receipt_redacted".to_string());
    }
    let rationale_codes = receipt
        .adaptation
        .as_ref()
        .map(|a| a.rationale_codes.clone())
        .unwrap_or_default();
    let key_signals = receipt
        .evaluation
        .as_ref()
        .map(|e| e.key_signals.clone())
        .unwrap_or_default();
    (
        receipt.memory_titles_used.clone(),
        rationale_codes,
        key_signals,
        redaction_flags,
    )
}

fn load_provider_calls(
    connection: &Connection,
    run_id: &str,
) -> Result<Vec<ProviderCallView>, String> {
    let mut stmt = connection
        .prepare(
            "
            SELECT provider, model, request_kind, input_chars, output_chars, latency_ms, cost_cents_est, created_at_ms
            FROM provider_calls
            WHERE run_id = ?1
            ORDER BY created_at_ms ASC
            ",
        )
        .map_err(|e| format!("Failed to prepare provider calls query: {e}"))?;
    let rows = stmt
        .query_map(params![run_id], |row| {
            Ok(ProviderCallView {
                provider: row.get(0)?,
                model: row.get(1)?,
                request_kind: row.get(2)?,
                input_chars: row.get(3)?,
                output_chars: row.get(4)?,
                latency_ms: row.get(5)?,
                cost_cents_est: row.get(6)?,
                created_at_ms: row.get(7)?,
            })
        })
        .map_err(|e| format!("Failed to query provider calls: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("Failed to parse provider call row: {e}"))?);
    }
    Ok(out)
}

fn load_context_sources(
    connection: &Connection,
    run_id: &str,
) -> Result<Vec<ContextSourceRecord>, String> {
    let mut stmt = connection
        .prepare(
            "
            SELECT kind, content
            FROM outcomes
            WHERE run_id = ?1 AND kind IN ('web_read', 'daily_sources', 'inbox_read')
            ORDER BY created_at ASC
            ",
        )
        .map_err(|e| format!("Failed to prepare context sources query: {e}"))?;
    let rows = stmt
        .query_map(params![run_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| format!("Failed to query context sources: {e}"))?;

    let mut out = Vec::new();
    for row in rows {
        let (kind, raw) = row.map_err(|e| format!("Failed to read context source row: {e}"))?;
        let value: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
        match kind.as_str() {
            "web_read" => out.push(parse_web_source(value)),
            "inbox_read" => out.push(parse_inbox_source(value)),
            "daily_sources" => out.extend(parse_daily_sources(value)),
            _ => {}
        }
    }
    Ok(out)
}

fn parse_web_source(value: Value) -> ContextSourceRecord {
    ContextSourceRecord {
        source_kind: "web_read".to_string(),
        source_id: None,
        url: value
            .get("url")
            .and_then(|v| v.as_str())
            .map(ToString::to_string),
        status: "captured".to_string(),
        fetched_at_ms: value.get("fetched_at_ms").and_then(|v| v.as_i64()),
        content_hash: value
            .get("content_hash")
            .and_then(|v| v.as_str())
            .map(ToString::to_string),
        excerpt_chars: value
            .get("current_excerpt")
            .and_then(|v| v.as_str())
            .map(|s| s.chars().count()),
        changed: value.get("changed").and_then(|v| v.as_bool()),
        diff_score: value.get("diff_score").and_then(|v| v.as_f64()),
        error: None,
    }
}

fn parse_inbox_source(value: Value) -> ContextSourceRecord {
    ContextSourceRecord {
        source_kind: "forwarded_email".to_string(),
        source_id: value
            .get("item_id")
            .and_then(|v| v.as_str())
            .map(ToString::to_string),
        url: None,
        status: if value
            .get("deduped_existing")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            "deduped".to_string()
        } else {
            "captured".to_string()
        },
        fetched_at_ms: value.get("created_at_ms").and_then(|v| v.as_i64()),
        content_hash: value
            .get("content_hash")
            .and_then(|v| v.as_str())
            .map(ToString::to_string),
        excerpt_chars: value
            .get("text_excerpt")
            .and_then(|v| v.as_str())
            .map(|s| s.chars().count()),
        changed: None,
        diff_score: None,
        error: None,
    }
}

fn parse_daily_sources(value: Value) -> Vec<ContextSourceRecord> {
    value
        .get("source_results")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|item| ContextSourceRecord {
                    source_kind: "daily_source".to_string(),
                    source_id: item
                        .get("source_id")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string),
                    url: item
                        .get("url")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string),
                    status: if item.get("fetch_error").and_then(|v| v.as_str()).is_some() {
                        "fetch_error".to_string()
                    } else {
                        "captured".to_string()
                    },
                    fetched_at_ms: item.get("fetched_at_ms").and_then(|v| v.as_i64()),
                    content_hash: None,
                    excerpt_chars: item
                        .get("text_excerpt")
                        .and_then(|v| v.as_str())
                        .map(|s| s.chars().count()),
                    changed: None,
                    diff_score: None,
                    error: item
                        .get("fetch_error")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}
