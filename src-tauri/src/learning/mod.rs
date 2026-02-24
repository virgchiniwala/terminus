use crate::db::{
    self, AdaptationLogInsert, AutopilotProfileUpsert, DecisionEventInsert, MemoryCardUpsert,
    RunEvaluationInsert,
};
use crate::schema::RecipeKind;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

const MAX_METADATA_JSON_BYTES: usize = 2048;
const MAX_SIGNALS_JSON_BYTES: usize = 2000;
const MAX_ADAPTATION_JSON_BYTES: usize = 2000;
const MAX_MEMORY_CARD_CONTENT_BYTES: usize = 4096;
const MAX_MEMORY_CARD_TITLE_CHARS: usize = 80;
const MAX_MEMORY_CONTEXT_CARDS: usize = 5;
const MAX_MEMORY_CONTEXT_CHARS: usize = 1500;
const DECISION_EVENTS_RATE_LIMIT_PER_MINUTE: i64 = 30;
const DECISION_EVENTS_RETENTION_MAX_PER_AUTOPILOT: i64 = 500;
const ADAPTATION_LOG_RETENTION_MAX_PER_AUTOPILOT: i64 = 200;
const RUN_EVALUATIONS_RETENTION_MAX_PER_AUTOPILOT: i64 = 500;
const DECISION_EVENTS_RETENTION_DAYS: i64 = 90;
const RUN_EVALUATIONS_RETENTION_DAYS: i64 = 180;
const PROTECTED_RECENT_RUNS_FOR_ADAPTATION: i64 = 10;
const COMPACTION_TRIGGER_EVENT_INTERVAL: i64 = 25;
const COMPACTION_DELETE_CHUNK: i64 = 200;
static LEARNING_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

const REDACTION_FORBIDDEN_SUBSTRINGS: [&str; 6] = [
    "bearer ",
    "sk-",
    "api_key",
    "authorization",
    "x-api-key",
    "openai_api_key",
];

#[derive(Debug, Error)]
pub enum LearningError {
    #[error("database error: {0}")]
    Db(String),
    #[error("serialization error: {0}")]
    Serde(String),
    #[error("invalid input: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionEventType {
    ApprovalApproved,
    ApprovalRejected,
    ApprovalExpired,
    OutcomeOpened,
    OutcomeIgnored,
    DraftEdited,
    DraftCopied,
}

impl DecisionEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ApprovalApproved => "approval_approved",
            Self::ApprovalRejected => "approval_rejected",
            Self::ApprovalExpired => "approval_expired",
            Self::OutcomeOpened => "outcome_opened",
            Self::OutcomeIgnored => "outcome_ignored",
            Self::DraftEdited => "draft_edited",
            Self::DraftCopied => "draft_copied",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "approval_approved" => Some(Self::ApprovalApproved),
            "approval_rejected" => Some(Self::ApprovalRejected),
            "approval_expired" => Some(Self::ApprovalExpired),
            "outcome_opened" => Some(Self::OutcomeOpened),
            "outcome_ignored" => Some(Self::OutcomeIgnored),
            "draft_edited" => Some(Self::DraftEdited),
            "draft_copied" => Some(Self::DraftCopied),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct DecisionEventMetadata {
    pub latency_ms: Option<i64>,
    pub reason_code: Option<String>,
    pub provider_kind: Option<String>,
    pub usd_cents_actual: Option<i64>,
    pub diff_score: Option<f64>,
    pub content_hash: Option<String>,
    pub content_length: Option<i64>,
    pub draft_length: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunEvaluationSummary {
    pub quality_score: i64,
    pub noise_score: i64,
    pub cost_score: i64,
    pub key_signals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdaptationSummary {
    pub applied: bool,
    pub rationale_codes: Vec<String>,
    pub changed_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LearningMode {
    MaxSavings,
    #[default]
    Balanced,
    BestQuality,
}

impl LearningMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MaxSavings => "max_savings",
            Self::Balanced => "balanced",
            Self::BestQuality => "best_quality",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "max_savings" => Some(Self::MaxSavings),
            "balanced" => Some(Self::Balanced),
            "best_quality" => Some(Self::BestQuality),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ProfileKnobs {
    pub min_diff_score_to_notify: Option<f64>,
    pub max_sources: Option<i64>,
    pub max_bullets: Option<i64>,
    pub reply_length_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ProfileSuppression {
    pub suppress_until_ms: Option<i64>,
    pub quiet_until_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutopilotProfile {
    pub autopilot_id: String,
    pub learning_enabled: bool,
    pub mode: LearningMode,
    pub knobs: ProfileKnobs,
    pub suppression: ProfileSuppression,
    pub updated_at_ms: i64,
    pub version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeProfile {
    pub learning_enabled: bool,
    pub mode: LearningMode,
    pub suppress_until_ms: Option<i64>,
    pub min_diff_score_to_notify: f64,
    pub max_sources: usize,
    pub max_bullets: usize,
    pub reply_length_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryContext {
    pub titles: Vec<String>,
    pub prompt_block: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LearningCompactionSummary {
    pub autopilot_id: Option<String>,
    pub dry_run: bool,
    pub decision_events_deleted: i64,
    pub adaptation_log_deleted: i64,
    pub run_evaluations_deleted: i64,
}

#[derive(Debug, Clone)]
struct DecisionEventRow {
    run_id: String,
    event_type: DecisionEventType,
    metadata: DecisionEventMetadata,
}

#[derive(Debug, Clone)]
struct EvaluationRow {
    quality_score: i64,
    noise_score: i64,
    cost_score: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemoryCardType {
    FormatPreference,
    SourcePreference,
    SuppressionRationale,
    RecurringEntities,
}

impl MemoryCardType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::FormatPreference => "format_preference",
            Self::SourcePreference => "source_preference",
            Self::SuppressionRationale => "suppression_rationale",
            Self::RecurringEntities => "recurring_entities",
        }
    }
}

pub fn record_decision_event(
    connection: &Connection,
    autopilot_id: &str,
    run_id: &str,
    step_id: Option<&str>,
    event_type: DecisionEventType,
    metadata: DecisionEventMetadata,
    client_event_id: Option<&str>,
) -> Result<(), LearningError> {
    enforce_decision_event_rate_limit(connection, autopilot_id)?;
    let metadata_json = serialize_bounded_json(
        &validate_and_sanitize_metadata(event_type, metadata)?,
        MAX_METADATA_JSON_BYTES,
    )?;
    let sanitized_client_event_id = sanitize_client_event_id(client_event_id)?;
    let payload = DecisionEventInsert {
        event_id: make_learning_id("decision"),
        client_event_id: sanitized_client_event_id,
        autopilot_id: autopilot_id.to_string(),
        run_id: run_id.to_string(),
        step_id: step_id.map(|v| v.to_string()),
        event_type: event_type.as_str().to_string(),
        metadata_json,
        created_at_ms: now_ms(),
    };
    let inserted = db::insert_decision_event(connection, &payload).map_err(LearningError::Db)?;
    if inserted {
        maybe_compact_after_event_insert(connection, autopilot_id)?;
    }
    Ok(())
}

pub fn record_decision_event_from_json(
    connection: &Connection,
    autopilot_id: &str,
    run_id: &str,
    step_id: Option<&str>,
    event_type: &str,
    metadata_json: Option<&str>,
    client_event_id: Option<&str>,
) -> Result<(), LearningError> {
    let parsed_event = DecisionEventType::parse(event_type).ok_or_else(|| {
        LearningError::Invalid(format!("Unsupported decision event type: {event_type}"))
    })?;
    let metadata = match metadata_json {
        Some(raw) if !raw.trim().is_empty() => {
            let value: Value = serde_json::from_str(raw).map_err(|e| {
                LearningError::Invalid(format!("metadata_json must be valid JSON: {e}"))
            })?;
            parse_and_validate_metadata_value(parsed_event, value)?
        }
        _ => DecisionEventMetadata::default(),
    };
    record_decision_event(
        connection,
        autopilot_id,
        run_id,
        step_id,
        parsed_event,
        metadata,
        client_event_id,
    )
}

pub fn ensure_autopilot_profile(
    connection: &Connection,
    autopilot_id: &str,
) -> Result<AutopilotProfile, LearningError> {
    if let Some(existing) = load_autopilot_profile(connection, autopilot_id)? {
        return Ok(existing);
    }

    let default = default_profile(autopilot_id);
    persist_profile(connection, &default)?;
    Ok(default)
}

pub fn get_runtime_profile(
    connection: &Connection,
    autopilot_id: &str,
) -> Result<RuntimeProfile, LearningError> {
    let profile = ensure_autopilot_profile(connection, autopilot_id)?;
    Ok(RuntimeProfile {
        learning_enabled: profile.learning_enabled,
        mode: profile.mode,
        suppress_until_ms: profile.suppression.suppress_until_ms,
        min_diff_score_to_notify: profile
            .knobs
            .min_diff_score_to_notify
            .unwrap_or(0.2)
            .clamp(0.1, 0.9),
        max_sources: profile.knobs.max_sources.unwrap_or(5).clamp(2, 10) as usize,
        max_bullets: profile.knobs.max_bullets.unwrap_or(6).clamp(3, 10) as usize,
        reply_length_hint: normalize_reply_length_hint(profile.knobs.reply_length_hint.as_deref()),
    })
}

pub fn set_autopilot_suppression_until(
    connection: &Connection,
    autopilot_id: &str,
    suppress_until_ms: Option<i64>,
) -> Result<(), LearningError> {
    let mut profile = ensure_autopilot_profile(connection, autopilot_id)?;
    profile.suppression.suppress_until_ms = suppress_until_ms;
    profile.updated_at_ms = now_ms();
    profile.version = profile.version.saturating_add(1);
    persist_profile(connection, &profile)
}

pub fn evaluate_run(
    connection: &Connection,
    run_id: &str,
) -> Result<RunEvaluationSummary, LearningError> {
    if let Some(existing) = get_run_evaluation(connection, run_id)? {
        return Ok(existing);
    }

    let run = load_run_snapshot(connection, run_id)?;
    if !is_terminal_state(&run.state) {
        return Err(LearningError::Invalid(
            "evaluate_run requires a terminal run".to_string(),
        ));
    }

    let approval_counts = load_approval_counts(connection, run_id)?;
    let events = load_decision_events_for_run(connection, run_id)?;

    let approved_events = events
        .iter()
        .filter(|e| e.event_type == DecisionEventType::ApprovalApproved)
        .count() as i64;
    let rejected_events = events
        .iter()
        .filter(|e| e.event_type == DecisionEventType::ApprovalRejected)
        .count() as i64;
    let ignored_events = events
        .iter()
        .filter(|e| e.event_type == DecisionEventType::OutcomeIgnored)
        .count() as i64;
    let edited_events = events
        .iter()
        .filter(|e| e.event_type == DecisionEventType::DraftEdited)
        .count() as i64;

    let latency_samples = events
        .iter()
        .filter_map(|e| {
            if e.event_type == DecisionEventType::ApprovalApproved {
                e.metadata.latency_ms
            } else {
                None
            }
        })
        .collect::<Vec<i64>>();
    let avg_latency = if latency_samples.is_empty() {
        None
    } else {
        Some(latency_samples.iter().sum::<i64>() / latency_samples.len() as i64)
    };

    let mut quality_score = 60 + approval_counts.approved * 15 + approved_events * 5;
    quality_score -= approval_counts.rejected * 20;
    quality_score -= rejected_events * 5;
    if edited_events > 0 {
        quality_score -= 10;
    }
    if let Some(latency_ms) = avg_latency {
        if latency_ms <= 120_000 {
            quality_score += 10;
        } else if latency_ms > 900_000 {
            quality_score -= 10;
        }
    }
    quality_score = clamp_score(quality_score);

    let no_change_runs = is_no_change_run(connection, run_id)?;
    let mut noise_score = 10 + ignored_events * 25 + rejected_events * 15;
    if no_change_runs && ignored_events > 0 {
        noise_score += 15;
    }
    noise_score = clamp_score(noise_score);

    let mut cost_score = 100;
    if run.usd_cents_actual > 40 {
        cost_score -= 30;
    }
    if run.usd_cents_actual > 80 {
        cost_score -= 50;
    }
    cost_score -= run.retry_count * 10;
    if run.provider_tier == "experimental" {
        cost_score -= 5;
    }
    cost_score = clamp_score(cost_score);

    let mut key_signals = Vec::new();
    if approval_counts.approved > 0 {
        key_signals.push("approvals_granted".to_string());
    }
    if approval_counts.rejected > 0 {
        key_signals.push("approvals_rejected".to_string());
    }
    if ignored_events > 0 {
        key_signals.push("outcomes_ignored".to_string());
    }
    if edited_events > 0 {
        key_signals.push("drafts_edited".to_string());
    }
    if run.retry_count > 0 {
        key_signals.push("retries_used".to_string());
    }
    if no_change_runs {
        key_signals.push("no_change_notification".to_string());
    }

    let signals_json = serialize_bounded_json(
        &json!({
            "approval_approved_count": approval_counts.approved,
            "approval_rejected_count": approval_counts.rejected,
            "event_approval_approved_count": approved_events,
            "event_approval_rejected_count": rejected_events,
            "outcome_ignored_count": ignored_events,
            "draft_edited_count": edited_events,
            "retry_count": run.retry_count,
            "usd_cents_actual": run.usd_cents_actual,
            "provider_tier": run.provider_tier,
            "avg_approval_latency_ms": avg_latency,
            "no_change_run": no_change_runs,
            "key_signals": key_signals,
        }),
        MAX_SIGNALS_JSON_BYTES,
    )?;

    let summary = RunEvaluationSummary {
        quality_score,
        noise_score,
        cost_score,
        key_signals,
    };

    let insert = RunEvaluationInsert {
        run_id: run_id.to_string(),
        autopilot_id: run.autopilot_id,
        quality_score,
        noise_score,
        cost_score,
        signals_json,
        created_at_ms: now_ms(),
    };
    db::insert_run_evaluation_if_missing(connection, &insert).map_err(LearningError::Db)?;

    Ok(summary)
}

pub fn adapt_autopilot(
    connection: &Connection,
    autopilot_id: &str,
    run_id: &str,
    recipe: RecipeKind,
) -> Result<AdaptationSummary, LearningError> {
    let exists: Option<String> = connection
        .query_row(
            "SELECT id FROM adaptation_log WHERE autopilot_id = ?1 AND run_id = ?2 LIMIT 1",
            params![autopilot_id, run_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| LearningError::Db(e.to_string()))?;
    if exists.is_some() {
        return Ok(AdaptationSummary {
            applied: false,
            rationale_codes: Vec::new(),
            changed_fields: Vec::new(),
        });
    }

    let mut profile = ensure_autopilot_profile(connection, autopilot_id)?;
    let recent = load_recent_evaluations(connection, autopilot_id, 10)?;
    let recent_events = load_recent_decision_events(connection, autopilot_id, 120)?;
    let now = now_ms();

    let mut changed_fields = Vec::new();
    let mut rationale_codes = Vec::new();

    if recipe == RecipeKind::WebsiteMonitor {
        let noisy_recent = last_n_runs_match(
            connection,
            autopilot_id,
            3,
            &[
                DecisionEventType::OutcomeIgnored,
                DecisionEventType::ApprovalRejected,
            ],
        )?;
        if noisy_recent {
            let next = profile
                .knobs
                .min_diff_score_to_notify
                .unwrap_or(0.2)
                .clamp(0.1, 0.9)
                + 0.1;
            let next = next.clamp(0.1, 0.9);
            set_knob_min_diff(&mut profile, next);
            profile.suppression.suppress_until_ms = Some(now + 24 * 60 * 60 * 1000);
            changed_fields.push("knobs.min_diff_score_to_notify".to_string());
            changed_fields.push("suppression.suppress_until_ms".to_string());
            rationale_codes.push("noise_suppression_website".to_string());
        }
    }

    if recipe == RecipeKind::DailyBrief {
        let last_three_bad = recent
            .iter()
            .take(3)
            .filter(|row| row.noise_score >= 70 || row.cost_score <= 30)
            .count()
            >= 3;
        if last_three_bad {
            let next_sources = (profile.knobs.max_sources.unwrap_or(5) - 1).clamp(2, 10);
            let next_bullets = (profile.knobs.max_bullets.unwrap_or(6) - 1).clamp(3, 10);
            profile.knobs.max_sources = Some(next_sources);
            profile.knobs.max_bullets = Some(next_bullets);
            changed_fields.push("knobs.max_sources".to_string());
            changed_fields.push("knobs.max_bullets".to_string());
            rationale_codes.push("scope_reduction_daily_brief".to_string());
        }
    }

    let soft_cap_approvals = recent_events
        .iter()
        .filter(|e| {
            e.event_type == DecisionEventType::ApprovalApproved
                && e.metadata.reason_code.as_deref() == Some("soft_cap")
        })
        .count();
    let approval_decisions = recent_events
        .iter()
        .filter(|e| {
            matches!(
                e.event_type,
                DecisionEventType::ApprovalApproved | DecisionEventType::ApprovalRejected
            )
        })
        .collect::<Vec<&DecisionEventRow>>();
    let approval_approved_rate = if approval_decisions.is_empty() {
        0.0
    } else {
        approval_decisions
            .iter()
            .filter(|e| e.event_type == DecisionEventType::ApprovalApproved)
            .count() as f64
            / approval_decisions.len() as f64
    };

    if soft_cap_approvals >= 3 && approval_approved_rate >= 0.8 {
        if profile.mode != LearningMode::MaxSavings {
            profile.mode = LearningMode::MaxSavings;
            changed_fields.push("mode".to_string());
            rationale_codes.push("frequent_soft_cap_approvals".to_string());
        }
    }

    let recent_five = approval_decisions
        .iter()
        .take(5)
        .cloned()
        .collect::<Vec<&DecisionEventRow>>();
    let recent_five_rate = if recent_five.is_empty() {
        0.0
    } else {
        recent_five
            .iter()
            .filter(|e| e.event_type == DecisionEventType::ApprovalApproved)
            .count() as f64
            / recent_five.len() as f64
    };
    if recent_five.len() >= 5 && recent_five_rate >= 0.8 {
        if profile.suppression.suppress_until_ms.is_some() {
            profile.suppression.suppress_until_ms = None;
            changed_fields.push("suppression.suppress_until_ms".to_string());
            rationale_codes.push("suppression_recovery".to_string());
        }
        if recipe == RecipeKind::WebsiteMonitor {
            let relaxed =
                (profile.knobs.min_diff_score_to_notify.unwrap_or(0.2) - 0.05).clamp(0.1, 0.9);
            set_knob_min_diff(&mut profile, relaxed);
            changed_fields.push("knobs.min_diff_score_to_notify".to_string());
        }
    }

    if changed_fields.is_empty() {
        return Ok(AdaptationSummary {
            applied: false,
            rationale_codes,
            changed_fields,
        });
    }

    sanitize_profile(&mut profile, recipe);

    let change_patch = json!({
        "mode": profile.mode.as_str(),
        "knobs": profile.knobs,
        "suppression": profile.suppression,
        "changed_fields": changed_fields,
    });
    let adaptation_hash = fnv1a_64_hex(
        &serde_json::to_string(&change_patch).map_err(|e| LearningError::Serde(e.to_string()))?,
    );
    if let Some(last_hash) = latest_adaptation_hash(connection, autopilot_id)? {
        if last_hash == adaptation_hash {
            return Ok(AdaptationSummary {
                applied: false,
                rationale_codes,
                changed_fields,
            });
        }
    }

    persist_profile(connection, &profile)?;
    let changes_json = serialize_bounded_json(&change_patch, MAX_ADAPTATION_JSON_BYTES)?;
    let rationale_json = serialize_bounded_json(&rationale_codes, 800)?;
    let inserted = db::insert_adaptation_log(
        connection,
        &AdaptationLogInsert {
            id: make_learning_id("adapt"),
            autopilot_id: autopilot_id.to_string(),
            run_id: run_id.to_string(),
            adaptation_hash,
            changes_json,
            rationale_codes_json: rationale_json,
            created_at_ms: now,
        },
    )
    .map_err(LearningError::Db)?;

    Ok(AdaptationSummary {
        applied: inserted && !changed_fields.is_empty(),
        rationale_codes,
        changed_fields,
    })
}

pub fn update_memory_cards(
    connection: &Connection,
    autopilot_id: &str,
    run_id: &str,
    recipe: RecipeKind,
) -> Result<(), LearningError> {
    let events = load_recent_decision_events(connection, autopilot_id, 200)?;
    let profile = ensure_autopilot_profile(connection, autopilot_id)?;
    let now = now_ms();

    let draft_edited_count = events
        .iter()
        .filter(|e| e.event_type == DecisionEventType::DraftEdited)
        .count();
    if draft_edited_count >= 2 {
        let content = json!({
            "tone": "concise",
            "structure": ["greeting", "2 bullets", "closing"],
            "avoid": ["long preamble"]
        });
        upsert_memory_card_internal(
            connection,
            autopilot_id,
            MemoryCardType::FormatPreference,
            "Preferred reply style",
            &content,
            (50 + (draft_edited_count as i64 * 10)).clamp(50, 95),
            Some(run_id),
            now,
        )?;
    }

    let minor_ignored = events
        .iter()
        .filter(|e| {
            e.event_type == DecisionEventType::OutcomeIgnored
                && e.metadata.diff_score.unwrap_or(1.0) <= 0.2
        })
        .count();
    if minor_ignored >= 2 {
        let content = json!({
            "pattern": "minor changes ignored",
            "min_diff_score_to_notify": profile.knobs.min_diff_score_to_notify.unwrap_or(0.2),
            "suppress_hours": 24
        });
        upsert_memory_card_internal(
            connection,
            autopilot_id,
            MemoryCardType::SuppressionRationale,
            "Minor changes can be suppressed",
            &content,
            70,
            Some(run_id),
            now,
        )?;
    }

    if recipe == RecipeKind::DailyBrief {
        if let Some(max_sources) = profile.knobs.max_sources {
            if max_sources < 5 {
                let content = json!({
                    "max_sources": max_sources,
                    "prefer": ["official sources"],
                    "avoid": ["duplicates"]
                });
                upsert_memory_card_internal(
                    connection,
                    autopilot_id,
                    MemoryCardType::SourcePreference,
                    "Preferred source scope",
                    &content,
                    75,
                    Some(run_id),
                    now,
                )?;
            }
        }
    }

    Ok(())
}

pub fn build_memory_context(
    connection: &Connection,
    autopilot_id: &str,
    _recipe: RecipeKind,
) -> Result<MemoryContext, LearningError> {
    let mut stmt = connection
        .prepare(
            "
            SELECT card_type, title, content_json
            FROM memory_cards
            WHERE autopilot_id = ?1
            ORDER BY updated_at_ms DESC
            LIMIT 20
            ",
        )
        .map_err(|e| LearningError::Db(e.to_string()))?;

    let rows = stmt
        .query_map(params![autopilot_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| LearningError::Db(e.to_string()))?;

    let mut snippets = Vec::new();
    for row in rows {
        let (card_type, title, content_json) = row.map_err(|e| LearningError::Db(e.to_string()))?;
        let content: Value =
            serde_json::from_str(&content_json).map_err(|e| LearningError::Serde(e.to_string()))?;
        let snippet = summarize_card(&card_type, &content)?;
        if !snippet.is_empty() {
            snippets.push((title, snippet));
        }
    }

    let mut titles = Vec::new();
    let mut lines = Vec::new();
    let mut total_chars = 0usize;
    for (title, snippet) in snippets.into_iter().take(MAX_MEMORY_CONTEXT_CARDS) {
        let line = format!("- {}: {}", title, snippet);
        let next_len = total_chars + line.chars().count() + 1;
        if next_len > MAX_MEMORY_CONTEXT_CHARS {
            break;
        }
        total_chars = next_len;
        titles.push(title);
        lines.push(line);
    }

    if lines.is_empty() {
        return Ok(MemoryContext {
            titles,
            prompt_block: String::new(),
        });
    }

    Ok(MemoryContext {
        titles,
        prompt_block: format!("Preferences:\n{}", lines.join("\n")),
    })
}

pub fn get_run_evaluation(
    connection: &Connection,
    run_id: &str,
) -> Result<Option<RunEvaluationSummary>, LearningError> {
    let row: Option<(i64, i64, i64, String)> = connection
        .query_row(
            "SELECT quality_score, noise_score, cost_score, signals_json FROM run_evaluations WHERE run_id = ?1",
            params![run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|e| LearningError::Db(e.to_string()))?;

    let Some((quality_score, noise_score, cost_score, signals_json)) = row else {
        return Ok(None);
    };
    let signals: Value =
        serde_json::from_str(&signals_json).map_err(|e| LearningError::Serde(e.to_string()))?;
    let key_signals = signals
        .get("key_signals")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    Ok(Some(RunEvaluationSummary {
        quality_score,
        noise_score,
        cost_score,
        key_signals,
    }))
}

pub fn get_latest_adaptation_for_run(
    connection: &Connection,
    autopilot_id: &str,
    run_id: &str,
) -> Result<Option<AdaptationSummary>, LearningError> {
    let row: Option<(String, String)> = connection
        .query_row(
            "SELECT changes_json, rationale_codes_json FROM adaptation_log WHERE autopilot_id = ?1 AND run_id = ?2 LIMIT 1",
            params![autopilot_id, run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| LearningError::Db(e.to_string()))?;

    let Some((changes_json, rationale_json)) = row else {
        return Ok(None);
    };

    let changes: Value =
        serde_json::from_str(&changes_json).map_err(|e| LearningError::Serde(e.to_string()))?;
    let changed_fields = changes
        .get("changed_fields")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();
    let rationale_codes = serde_json::from_str::<Vec<String>>(&rationale_json)
        .map_err(|e| LearningError::Serde(e.to_string()))?;

    Ok(Some(AdaptationSummary {
        applied: !changed_fields.is_empty(),
        rationale_codes,
        changed_fields,
    }))
}

pub fn list_memory_titles_for_run(
    connection: &Connection,
    run_id: &str,
) -> Result<Vec<String>, LearningError> {
    let mut stmt = connection
        .prepare(
            "SELECT content FROM outcomes WHERE run_id = ?1 AND kind = 'memory_usage' ORDER BY created_at ASC",
        )
        .map_err(|e| LearningError::Db(e.to_string()))?;
    let rows = stmt
        .query_map(params![run_id], |row| row.get::<_, String>(0))
        .map_err(|e| LearningError::Db(e.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        let raw = row.map_err(|e| LearningError::Db(e.to_string()))?;
        let value: Value =
            serde_json::from_str(&raw).map_err(|e| LearningError::Serde(e.to_string()))?;
        let titles = value
            .get("titles")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        for title in titles {
            if !out.contains(&title) {
                out.push(title);
            }
        }
    }
    Ok(out)
}

pub fn persist_memory_usage(
    connection: &Connection,
    run_id: &str,
    step_id: &str,
    titles: &[String],
) -> Result<(), LearningError> {
    if titles.is_empty() {
        return Ok(());
    }

    let payload = serialize_bounded_json(&json!({ "titles": titles }), 1200)?;
    connection
        .execute(
            "
            INSERT INTO outcomes (
              id, run_id, step_id, kind, status, content, created_at, updated_at
            ) VALUES (?1, ?2, ?3, 'memory_usage', 'used', ?4, ?5, ?5)
            ON CONFLICT(run_id, step_id, kind)
            DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at
            ",
            params![
                make_learning_id("memory_usage"),
                run_id,
                step_id,
                payload,
                now_ms()
            ],
        )
        .map_err(|e| LearningError::Db(e.to_string()))?;
    Ok(())
}

fn compact_decision_events_for_autopilot(
    connection: &Connection,
    autopilot_id: &str,
    protected_runs: &[String],
    dry_run: bool,
) -> Result<i64, LearningError> {
    let cutoff = now_ms() - DECISION_EVENTS_RETENTION_DAYS * 24 * 60 * 60 * 1000;
    let mut stmt = connection
        .prepare(
            "
            SELECT event_id, run_id, created_at_ms
            FROM decision_events
            WHERE autopilot_id = ?1
            ORDER BY created_at_ms DESC
            ",
        )
        .map_err(|e| LearningError::Db(e.to_string()))?;
    let rows = stmt
        .query_map(params![autopilot_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|e| LearningError::Db(e.to_string()))?;
    let protected: std::collections::HashSet<String> = protected_runs.iter().cloned().collect();
    let mut to_delete = Vec::new();
    for (idx, row) in rows.enumerate() {
        let (event_id, run_id, created_at_ms) =
            row.map_err(|e| LearningError::Db(e.to_string()))?;
        let rank = idx as i64 + 1;
        let keep_by_rank = rank <= DECISION_EVENTS_RETENTION_MAX_PER_AUTOPILOT;
        let keep_by_age = created_at_ms >= cutoff;
        let keep_by_protection = protected.contains(&run_id);
        if !(keep_by_rank && keep_by_age) && !keep_by_protection {
            to_delete.push(event_id);
        }
    }
    delete_ids_chunked(
        connection,
        "decision_events",
        "event_id",
        &to_delete,
        dry_run,
    )
}

fn compact_adaptation_log_for_autopilot(
    connection: &Connection,
    autopilot_id: &str,
    dry_run: bool,
) -> Result<i64, LearningError> {
    let mut stmt = connection
        .prepare(
            "
            SELECT id
            FROM adaptation_log
            WHERE autopilot_id = ?1
            ORDER BY created_at_ms DESC
            ",
        )
        .map_err(|e| LearningError::Db(e.to_string()))?;
    let rows = stmt
        .query_map(params![autopilot_id], |row| row.get::<_, String>(0))
        .map_err(|e| LearningError::Db(e.to_string()))?;
    let mut to_delete = Vec::new();
    for (idx, row) in rows.enumerate() {
        let id = row.map_err(|e| LearningError::Db(e.to_string()))?;
        if idx as i64 >= ADAPTATION_LOG_RETENTION_MAX_PER_AUTOPILOT {
            to_delete.push(id);
        }
    }
    delete_ids_chunked(connection, "adaptation_log", "id", &to_delete, dry_run)
}

fn compact_run_evaluations_for_autopilot(
    connection: &Connection,
    autopilot_id: &str,
    protected_runs: &[String],
    dry_run: bool,
) -> Result<i64, LearningError> {
    let cutoff = now_ms() - RUN_EVALUATIONS_RETENTION_DAYS * 24 * 60 * 60 * 1000;
    let mut stmt = connection
        .prepare(
            "
            SELECT run_id, created_at_ms
            FROM run_evaluations
            WHERE autopilot_id = ?1
            ORDER BY created_at_ms DESC
            ",
        )
        .map_err(|e| LearningError::Db(e.to_string()))?;
    let rows = stmt
        .query_map(params![autopilot_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|e| LearningError::Db(e.to_string()))?;
    let protected: std::collections::HashSet<String> = protected_runs.iter().cloned().collect();
    let mut to_delete = Vec::new();
    for (idx, row) in rows.enumerate() {
        let (run_id, created_at_ms) = row.map_err(|e| LearningError::Db(e.to_string()))?;
        let rank = idx as i64 + 1;
        let keep_by_rank = rank <= RUN_EVALUATIONS_RETENTION_MAX_PER_AUTOPILOT;
        let keep_by_age = created_at_ms >= cutoff;
        let keep_by_protection = protected.contains(&run_id);
        if !(keep_by_rank && keep_by_age) && !keep_by_protection {
            to_delete.push(run_id);
        }
    }
    delete_ids_chunked(connection, "run_evaluations", "run_id", &to_delete, dry_run)
}

fn recent_terminal_run_ids(
    connection: &Connection,
    autopilot_id: &str,
    limit: i64,
) -> Result<Vec<String>, LearningError> {
    let mut stmt = connection
        .prepare(
            "
            SELECT id FROM runs
            WHERE autopilot_id = ?1
              AND state IN ('succeeded','failed','blocked','canceled')
            ORDER BY updated_at DESC
            LIMIT ?2
            ",
        )
        .map_err(|e| LearningError::Db(e.to_string()))?;
    let rows = stmt
        .query_map(params![autopilot_id, limit], |row| row.get::<_, String>(0))
        .map_err(|e| LearningError::Db(e.to_string()))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| LearningError::Db(e.to_string()))?);
    }
    Ok(out)
}

fn write_compaction_activity(
    connection: &Connection,
    autopilot_id: Option<&str>,
    summary: &LearningCompactionSummary,
) -> Result<(), LearningError> {
    let event = format!(
        "learning_compaction: decision_events_deleted={}, adaptation_log_deleted={}, run_evaluations_deleted={}",
        summary.decision_events_deleted, summary.adaptation_log_deleted, summary.run_evaluations_deleted
    );
    let created_at = now_ms();
    if let Some(ap_id) = autopilot_id {
        let latest_run_id: Option<String> = connection
            .query_row(
                "SELECT id FROM runs WHERE autopilot_id = ?1 ORDER BY updated_at DESC LIMIT 1",
                params![ap_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| LearningError::Db(e.to_string()))?;
        if let Some(run_id) = latest_run_id {
            let _ = connection.execute(
                "INSERT INTO activities (id, run_id, activity_type, user_message, created_at)
                 VALUES (?1, ?2, 'learning_compaction', ?3, ?4)",
                params![make_learning_id("activity"), run_id, event, created_at],
            );
        }
    }
    connection
        .execute(
            "INSERT INTO activity (id, autopilot_id, event, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                make_learning_id("learning_compact"),
                autopilot_id,
                event,
                created_at
            ],
        )
        .map_err(|e| LearningError::Db(e.to_string()))?;
    Ok(())
}
fn delete_ids_chunked(
    connection: &Connection,
    table: &str,
    id_col: &str,
    ids: &[String],
    dry_run: bool,
) -> Result<i64, LearningError> {
    if dry_run || ids.is_empty() {
        return Ok(ids.len() as i64);
    }
    let mut deleted_total = 0_i64;
    for chunk in ids.chunks(COMPACTION_DELETE_CHUNK as usize) {
        let placeholders = (0..chunk.len())
            .map(|_| "?")
            .collect::<Vec<&str>>()
            .join(",");
        let sql = format!("DELETE FROM {table} WHERE {id_col} IN ({placeholders})");
        connection
            .execute(
                &sql,
                rusqlite::params_from_iter(chunk.iter().cloned().map(rusqlite::types::Value::from)),
            )
            .map_err(|e| LearningError::Db(e.to_string()))?;
        deleted_total += chunk.len() as i64;
    }
    Ok(deleted_total)
}

pub fn compact_learning_data(
    connection: &Connection,
    autopilot_id: Option<&str>,
    dry_run: bool,
) -> Result<LearningCompactionSummary, LearningError> {
    let autopilot_ids = if let Some(id) = autopilot_id {
        vec![id.to_string()]
    } else {
        let mut stmt = connection
            .prepare("SELECT id FROM autopilots ORDER BY created_at DESC")
            .map_err(|e| LearningError::Db(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| LearningError::Db(e.to_string()))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row.map_err(|e| LearningError::Db(e.to_string()))?);
        }
        ids
    };

    let mut summary = LearningCompactionSummary {
        autopilot_id: autopilot_id.map(|s| s.to_string()),
        dry_run,
        ..Default::default()
    };

    for id in autopilot_ids {
        let protected_runs =
            recent_terminal_run_ids(connection, &id, PROTECTED_RECENT_RUNS_FOR_ADAPTATION)?;
        summary.decision_events_deleted +=
            compact_decision_events_for_autopilot(connection, &id, &protected_runs, dry_run)?;
        summary.adaptation_log_deleted +=
            compact_adaptation_log_for_autopilot(connection, &id, dry_run)?;
        summary.run_evaluations_deleted +=
            compact_run_evaluations_for_autopilot(connection, &id, &protected_runs, dry_run)?;
    }

    if !dry_run {
        write_compaction_activity(connection, autopilot_id, &summary)?;
    }
    Ok(summary)
}

fn load_autopilot_profile(
    connection: &Connection,
    autopilot_id: &str,
) -> Result<Option<AutopilotProfile>, LearningError> {
    let row: Option<(i64, String, String, String, i64, i64)> = connection
        .query_row(
            "
            SELECT learning_enabled, mode, knobs_json, suppression_json, updated_at_ms, version
            FROM autopilot_profile
            WHERE autopilot_id = ?1
            ",
            params![autopilot_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()
        .map_err(|e| LearningError::Db(e.to_string()))?;

    let Some((learning_enabled, mode, knobs_json, suppression_json, updated_at_ms, version)) = row
    else {
        return Ok(None);
    };

    let mode = LearningMode::parse(&mode).unwrap_or_default();
    let knobs = serde_json::from_str::<ProfileKnobs>(&knobs_json).unwrap_or_default();
    let suppression =
        serde_json::from_str::<ProfileSuppression>(&suppression_json).unwrap_or_default();

    Ok(Some(AutopilotProfile {
        autopilot_id: autopilot_id.to_string(),
        learning_enabled: learning_enabled == 1,
        mode,
        knobs,
        suppression,
        updated_at_ms,
        version,
    }))
}

fn persist_profile(
    connection: &Connection,
    profile: &AutopilotProfile,
) -> Result<(), LearningError> {
    let knobs_json = serialize_bounded_json(&profile.knobs, 1200)?;
    let suppression_json = serialize_bounded_json(&profile.suppression, 800)?;

    db::upsert_autopilot_profile(
        connection,
        &AutopilotProfileUpsert {
            autopilot_id: profile.autopilot_id.clone(),
            learning_enabled: profile.learning_enabled,
            mode: profile.mode.as_str().to_string(),
            knobs_json,
            suppression_json,
            updated_at_ms: profile.updated_at_ms,
            version: profile.version,
        },
    )
    .map_err(LearningError::Db)
}

fn default_profile(autopilot_id: &str) -> AutopilotProfile {
    AutopilotProfile {
        autopilot_id: autopilot_id.to_string(),
        learning_enabled: true,
        mode: LearningMode::Balanced,
        knobs: ProfileKnobs {
            min_diff_score_to_notify: Some(0.2),
            max_sources: Some(5),
            max_bullets: Some(6),
            reply_length_hint: Some("medium".to_string()),
        },
        suppression: ProfileSuppression::default(),
        updated_at_ms: now_ms(),
        version: 1,
    }
}

fn sanitize_profile(profile: &mut AutopilotProfile, recipe: RecipeKind) {
    profile.knobs.min_diff_score_to_notify = Some(
        profile
            .knobs
            .min_diff_score_to_notify
            .unwrap_or(0.2)
            .clamp(0.1, 0.9),
    );

    profile.knobs.max_sources = Some(profile.knobs.max_sources.unwrap_or(5).clamp(2, 10));
    profile.knobs.max_bullets = Some(profile.knobs.max_bullets.unwrap_or(6).clamp(3, 10));

    profile.knobs.reply_length_hint = Some(normalize_reply_length_hint(
        profile.knobs.reply_length_hint.as_deref(),
    ));

    if recipe != RecipeKind::WebsiteMonitor {
        profile.knobs.min_diff_score_to_notify =
            Some(profile.knobs.min_diff_score_to_notify.unwrap_or(0.2));
    }

    profile.updated_at_ms = now_ms();
    profile.version = profile.version.max(1) + 1;
}

fn set_knob_min_diff(profile: &mut AutopilotProfile, value: f64) {
    profile.knobs.min_diff_score_to_notify = Some(value.clamp(0.1, 0.9));
}

fn normalize_reply_length_hint(value: Option<&str>) -> String {
    match value.unwrap_or("medium") {
        "short" => "short".to_string(),
        _ => "medium".to_string(),
    }
}

fn upsert_memory_card_internal(
    connection: &Connection,
    autopilot_id: &str,
    card_type: MemoryCardType,
    title: &str,
    content: &Value,
    confidence: i64,
    created_from_run_id: Option<&str>,
    now: i64,
) -> Result<(), LearningError> {
    if title.chars().count() > MAX_MEMORY_CARD_TITLE_CHARS {
        return Err(LearningError::Invalid(
            "memory card title exceeded max length".to_string(),
        ));
    }
    let content_json = serialize_bounded_json(content, MAX_MEMORY_CARD_CONTENT_BYTES)?;
    db::upsert_memory_card(
        connection,
        &MemoryCardUpsert {
            card_id: format!("mem_{}_{}", autopilot_id, card_type.as_str()),
            autopilot_id: autopilot_id.to_string(),
            card_type: card_type.as_str().to_string(),
            title: title.to_string(),
            content_json,
            confidence: confidence.clamp(0, 100),
            created_from_run_id: created_from_run_id.map(|s| s.to_string()),
            updated_at_ms: now,
            version: 1,
        },
    )
    .map_err(LearningError::Db)
}

fn summarize_card(card_type: &str, content: &Value) -> Result<String, LearningError> {
    let text = match card_type {
        "format_preference" => {
            let tone = content
                .get("tone")
                .and_then(|v| v.as_str())
                .unwrap_or("concise");
            format!("Keep the draft tone {} and structured.", tone)
        }
        "suppression_rationale" => {
            let threshold = content
                .get("min_diff_score_to_notify")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.2);
            format!(
                "Ignore minor change alerts below diff score {:.2}.",
                threshold
            )
        }
        "source_preference" => {
            let max_sources = content
                .get("max_sources")
                .and_then(|v| v.as_i64())
                .unwrap_or(5);
            format!(
                "Use up to {} high-signal sources and skip duplicates.",
                max_sources
            )
        }
        "recurring_entities" => "Track recurring entities called out by the user.".to_string(),
        _ => String::new(),
    };
    if text.chars().count() > 220 {
        return Err(LearningError::Invalid(
            "memory card summary exceeded size limit".to_string(),
        ));
    }
    Ok(text)
}

fn parse_and_validate_metadata_value(
    event_type: DecisionEventType,
    value: Value,
) -> Result<DecisionEventMetadata, LearningError> {
    let obj = value
        .as_object()
        .ok_or_else(|| LearningError::Invalid("metadata_json must be an object".to_string()))?;

    let allowed = allowed_metadata_keys_for_event(event_type);
    for key in obj.keys() {
        if !allowed
            .iter()
            .any(|allowed_key| allowed_key == &key.as_str())
        {
            return Err(LearningError::Invalid(format!(
                "Unsupported metadata key: {key}"
            )));
        }
    }

    let metadata: DecisionEventMetadata = serde_json::from_value(Value::Object(obj.clone()))
        .map_err(|e| LearningError::Invalid(format!("Invalid metadata shape: {e}")))?;
    validate_and_sanitize_metadata(event_type, metadata)
}

fn validate_and_sanitize_metadata(
    event_type: DecisionEventType,
    mut metadata: DecisionEventMetadata,
) -> Result<DecisionEventMetadata, LearningError> {
    if let Some(code) = metadata.reason_code.as_mut() {
        ensure_text_is_safe(code, "reason_code")?;
        *code = code.chars().take(40).collect::<String>();
    }
    if let Some(kind) = metadata.provider_kind.as_mut() {
        ensure_text_is_safe(kind, "provider_kind")?;
        *kind = kind
            .chars()
            .take(20)
            .collect::<String>()
            .to_ascii_lowercase();
    }
    if let Some(hash) = metadata.content_hash.as_mut() {
        ensure_text_is_safe(hash, "content_hash")?;
        *hash = hash.chars().take(32).collect::<String>();
    }
    if let Some(diff) = metadata.diff_score {
        metadata.diff_score = Some(diff.clamp(0.0, 1.0));
    }
    if let Some(length) = metadata.content_length {
        metadata.content_length = Some(length.clamp(0, 50_000));
    }
    if let Some(length) = metadata.draft_length {
        metadata.draft_length = Some(length.clamp(0, 20_000));
    }
    validate_event_metadata_semantics(event_type, &metadata)?;
    Ok(metadata)
}

fn allowed_metadata_keys_for_event(event_type: DecisionEventType) -> &'static [&'static str] {
    match event_type {
        DecisionEventType::ApprovalApproved | DecisionEventType::ApprovalRejected => &[
            "latency_ms",
            "reason_code",
            "provider_kind",
            "usd_cents_actual",
        ],
        DecisionEventType::ApprovalExpired => &["reason_code"],
        DecisionEventType::OutcomeOpened => &["reason_code"],
        DecisionEventType::OutcomeIgnored => &[
            "reason_code",
            "diff_score",
            "content_hash",
            "content_length",
        ],
        DecisionEventType::DraftEdited | DecisionEventType::DraftCopied => &[
            "reason_code",
            "content_hash",
            "content_length",
            "draft_length",
        ],
    }
}

fn validate_event_metadata_semantics(
    event_type: DecisionEventType,
    metadata: &DecisionEventMetadata,
) -> Result<(), LearningError> {
    let has_latency = metadata.latency_ms.is_some();
    let has_provider = metadata.provider_kind.is_some();
    let has_spend = metadata.usd_cents_actual.is_some();
    let has_diff = metadata.diff_score.is_some();
    let has_draft = metadata.draft_length.is_some();

    let allowed = allowed_metadata_keys_for_event(event_type);
    if !allowed.contains(&"latency_ms") && has_latency {
        return Err(LearningError::Invalid(
            "latency_ms is not allowed for this event type".to_string(),
        ));
    }
    if !allowed.contains(&"provider_kind") && has_provider {
        return Err(LearningError::Invalid(
            "provider_kind is not allowed for this event type".to_string(),
        ));
    }
    if !allowed.contains(&"usd_cents_actual") && has_spend {
        return Err(LearningError::Invalid(
            "usd_cents_actual is not allowed for this event type".to_string(),
        ));
    }
    if !allowed.contains(&"diff_score") && has_diff {
        return Err(LearningError::Invalid(
            "diff_score is not allowed for this event type".to_string(),
        ));
    }
    if !allowed.contains(&"draft_length") && has_draft {
        return Err(LearningError::Invalid(
            "draft_length is not allowed for this event type".to_string(),
        ));
    }
    Ok(())
}

fn sanitize_client_event_id(value: Option<&str>) -> Result<Option<String>, LearningError> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > 80 {
        return Err(LearningError::Invalid(
            "client_event_id must be 80 chars or less".to_string(),
        ));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ':')
    {
        return Err(LearningError::Invalid(
            "client_event_id contains unsupported characters".to_string(),
        ));
    }
    Ok(Some(trimmed.to_string()))
}

fn ensure_text_is_safe(value: &str, field_name: &str) -> Result<(), LearningError> {
    if value.chars().count() > 256 {
        return Err(LearningError::Invalid(format!(
            "{field_name} exceeds max length"
        )));
    }
    let lower = value.to_ascii_lowercase();
    for forbidden in REDACTION_FORBIDDEN_SUBSTRINGS {
        if lower.contains(forbidden) {
            return Err(LearningError::Invalid(format!(
                "{field_name} contains disallowed secret-like content"
            )));
        }
    }
    if looks_like_email_dump(value) {
        return Err(LearningError::Invalid(format!(
            "{field_name} appears to contain raw message content"
        )));
    }
    Ok(())
}

fn looks_like_email_dump(value: &str) -> bool {
    let line_count = value.lines().count();
    if line_count >= 5 && value.lines().any(|line| line.chars().count() > 200) {
        return true;
    }
    let lower = value.to_ascii_lowercase();
    let header_hits = ["subject:", "from:", "to:", "cc:", "bcc:", "date:"]
        .iter()
        .filter(|h| lower.contains(**h))
        .count();
    header_hits >= 3
}

fn enforce_decision_event_rate_limit(
    connection: &Connection,
    autopilot_id: &str,
) -> Result<(), LearningError> {
    let cutoff = now_ms() - 60_000;
    let count_in_window: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM decision_events WHERE autopilot_id = ?1 AND created_at_ms >= ?2",
            params![autopilot_id, cutoff],
            |row| row.get(0),
        )
        .map_err(|e| LearningError::Db(e.to_string()))?;
    if count_in_window >= DECISION_EVENTS_RATE_LIMIT_PER_MINUTE {
        return Err(LearningError::Invalid(
            "Too many learning signals in a short window. Try again in a minute.".to_string(),
        ));
    }
    Ok(())
}

fn maybe_compact_after_event_insert(
    connection: &Connection,
    autopilot_id: &str,
) -> Result<(), LearningError> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM decision_events WHERE autopilot_id = ?1",
            params![autopilot_id],
            |row| row.get(0),
        )
        .map_err(|e| LearningError::Db(e.to_string()))?;
    if count > 0 && count % COMPACTION_TRIGGER_EVENT_INTERVAL == 0 {
        let _ = compact_learning_data(connection, Some(autopilot_id), false)?;
    }
    Ok(())
}

fn serialize_bounded_json<T: Serialize>(
    value: &T,
    max_bytes: usize,
) -> Result<String, LearningError> {
    let json = serde_json::to_string(value).map_err(|e| LearningError::Serde(e.to_string()))?;
    if json.len() > max_bytes {
        return Err(LearningError::Invalid(format!(
            "JSON payload exceeded {} bytes",
            max_bytes
        )));
    }
    Ok(json)
}

fn load_recent_decision_events(
    connection: &Connection,
    autopilot_id: &str,
    limit: usize,
) -> Result<Vec<DecisionEventRow>, LearningError> {
    let mut stmt = connection
        .prepare(
            "
            SELECT run_id, event_type, metadata_json
            FROM decision_events
            WHERE autopilot_id = ?1
            ORDER BY created_at_ms DESC
            LIMIT ?2
            ",
        )
        .map_err(|e| LearningError::Db(e.to_string()))?;

    let rows = stmt
        .query_map(params![autopilot_id, limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| LearningError::Db(e.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        let (run_id, event_type, metadata_json) =
            row.map_err(|e| LearningError::Db(e.to_string()))?;
        if let Some(kind) = DecisionEventType::parse(&event_type) {
            let metadata =
                serde_json::from_str::<DecisionEventMetadata>(&metadata_json).unwrap_or_default();
            out.push(DecisionEventRow {
                run_id,
                event_type: kind,
                metadata,
            });
        }
    }
    Ok(out)
}

fn load_decision_events_for_run(
    connection: &Connection,
    run_id: &str,
) -> Result<Vec<DecisionEventRow>, LearningError> {
    let mut stmt = connection
        .prepare(
            "SELECT run_id, event_type, metadata_json FROM decision_events WHERE run_id = ?1 ORDER BY created_at_ms ASC",
        )
        .map_err(|e| LearningError::Db(e.to_string()))?;
    let rows = stmt
        .query_map(params![run_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| LearningError::Db(e.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        let (row_run_id, event_type, metadata_json) =
            row.map_err(|e| LearningError::Db(e.to_string()))?;
        if let Some(kind) = DecisionEventType::parse(&event_type) {
            let metadata =
                serde_json::from_str::<DecisionEventMetadata>(&metadata_json).unwrap_or_default();
            out.push(DecisionEventRow {
                run_id: row_run_id,
                event_type: kind,
                metadata,
            });
        }
    }
    Ok(out)
}

fn load_recent_evaluations(
    connection: &Connection,
    autopilot_id: &str,
    limit: usize,
) -> Result<Vec<EvaluationRow>, LearningError> {
    let mut stmt = connection
        .prepare(
            "
            SELECT quality_score, noise_score, cost_score
            FROM run_evaluations
            WHERE autopilot_id = ?1
            ORDER BY created_at_ms DESC
            LIMIT ?2
            ",
        )
        .map_err(|e| LearningError::Db(e.to_string()))?;
    let rows = stmt
        .query_map(params![autopilot_id, limit as i64], |row| {
            Ok(EvaluationRow {
                quality_score: row.get(0)?,
                noise_score: row.get(1)?,
                cost_score: row.get(2)?,
            })
        })
        .map_err(|e| LearningError::Db(e.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| LearningError::Db(e.to_string()))?);
    }
    Ok(out)
}

fn last_n_runs_match(
    connection: &Connection,
    autopilot_id: &str,
    n: usize,
    accepted_events: &[DecisionEventType],
) -> Result<bool, LearningError> {
    let mut stmt = connection
        .prepare(
            "
            SELECT id
            FROM runs
            WHERE autopilot_id = ?1 AND state IN ('succeeded','failed','blocked','canceled')
            ORDER BY created_at DESC
            LIMIT ?2
            ",
        )
        .map_err(|e| LearningError::Db(e.to_string()))?;
    let rows = stmt
        .query_map(params![autopilot_id, n as i64], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| LearningError::Db(e.to_string()))?;

    let mut run_ids = Vec::new();
    for row in rows {
        run_ids.push(row.map_err(|e| LearningError::Db(e.to_string()))?);
    }
    if run_ids.len() < n {
        return Ok(false);
    }

    let events = load_recent_decision_events(connection, autopilot_id, 300)?;
    for run_id in run_ids {
        let hit = events.iter().any(|event| {
            event.run_id == run_id
                && accepted_events
                    .iter()
                    .any(|allowed| event.event_type == *allowed)
        });
        if !hit {
            return Ok(false);
        }
    }

    Ok(true)
}

#[derive(Debug, Clone)]
struct ApprovalCounts {
    approved: i64,
    rejected: i64,
}

fn load_approval_counts(
    connection: &Connection,
    run_id: &str,
) -> Result<ApprovalCounts, LearningError> {
    let approved: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM approvals WHERE run_id = ?1 AND status = 'approved'",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(|e| LearningError::Db(e.to_string()))?;
    let rejected: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM approvals WHERE run_id = ?1 AND status = 'rejected'",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(|e| LearningError::Db(e.to_string()))?;
    Ok(ApprovalCounts { approved, rejected })
}

#[derive(Debug, Clone)]
struct RunSnapshot {
    autopilot_id: String,
    state: String,
    provider_tier: String,
    retry_count: i64,
    usd_cents_actual: i64,
}

fn load_run_snapshot(connection: &Connection, run_id: &str) -> Result<RunSnapshot, LearningError> {
    connection
        .query_row(
            "
            SELECT autopilot_id, state, provider_tier, retry_count, usd_cents_actual
            FROM runs WHERE id = ?1
            ",
            params![run_id],
            |row| {
                Ok(RunSnapshot {
                    autopilot_id: row.get(0)?,
                    state: row.get(1)?,
                    provider_tier: row.get(2)?,
                    retry_count: row.get(3)?,
                    usd_cents_actual: row.get(4)?,
                })
            },
        )
        .map_err(|e| LearningError::Db(e.to_string()))
}

fn is_terminal_state(state: &str) -> bool {
    matches!(state, "succeeded" | "failed" | "blocked" | "canceled")
}

fn is_no_change_run(connection: &Connection, run_id: &str) -> Result<bool, LearningError> {
    let summary: Option<String> = connection
        .query_row(
            "SELECT content FROM outcomes WHERE run_id = ?1 AND kind = 'receipt' LIMIT 1",
            params![run_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| LearningError::Db(e.to_string()))?;
    let Some(content) = summary else {
        return Ok(false);
    };

    let value: Value = serde_json::from_str(&content).unwrap_or(Value::Null);
    let summary = value
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    Ok(summary.contains("no changes"))
}

fn clamp_score(score: i64) -> i64 {
    score.clamp(0, 100)
}

fn latest_adaptation_hash(
    connection: &Connection,
    autopilot_id: &str,
) -> Result<Option<String>, LearningError> {
    connection
        .query_row(
            "SELECT adaptation_hash FROM adaptation_log WHERE autopilot_id = ?1 ORDER BY created_at_ms DESC LIMIT 1",
            params![autopilot_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| LearningError::Db(e.to_string()))
}

fn fnv1a_64_hex(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in input.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn make_learning_id(prefix: &str) -> String {
    let n = LEARNING_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}_{}_{}", prefix, now_ms(), n)
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{AutopilotPlan, ProviderId};

    fn setup_conn() -> Connection {
        let mut connection = Connection::open_in_memory().expect("open in-memory sqlite");
        crate::db::bootstrap_schema(&mut connection).expect("bootstrap schema");
        connection
    }

    fn insert_terminal_run(connection: &Connection, autopilot_id: &str, run_id: &str) {
        let plan = AutopilotPlan::from_intent(
            RecipeKind::WebsiteMonitor,
            "monitor https://example.com".to_string(),
            ProviderId::OpenAi,
        );
        let plan_json = serde_json::to_string(&plan).expect("plan json");
        connection
            .execute(
                "INSERT OR IGNORE INTO autopilots (id, name, created_at) VALUES (?1, 'Test', 1)",
                params![autopilot_id],
            )
            .expect("insert autopilot");
        connection
            .execute(
                "
                INSERT INTO runs (
                    id, autopilot_id, idempotency_key, plan_json, provider_kind, provider_tier,
                    state, current_step_index, retry_count, max_retries,
                    soft_cap_approved, spend_usd_estimate, spend_usd_actual,
                    usd_cents_estimate, usd_cents_actual,
                    created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, 'openai', 'supported', 'succeeded', 3, 0, 2, 0, 0.0, 0.0, 0, 40, 1, 1)
                ",
                params![run_id, autopilot_id, format!("idem_{run_id}"), plan_json],
            )
            .expect("insert run");
        connection
            .execute(
                "INSERT INTO outcomes (id, run_id, step_id, kind, status, content, created_at, updated_at) VALUES (?1, ?2, 'terminal', 'receipt', 'final', '{\"summary\":\"Run completed\"}', 1, 1)",
                params![format!("out_receipt_{run_id}"), run_id],
            )
            .expect("insert receipt");
    }

    #[test]
    fn decision_event_metadata_rejects_unknown_keys() {
        let connection = setup_conn();
        insert_terminal_run(&connection, "auto_eval_1", "run_eval_1");

        let result = record_decision_event_from_json(
            &connection,
            "auto_eval_1",
            "run_eval_1",
            Some("step_1"),
            "outcome_opened",
            Some("{\"raw_text\":\"should fail\"}"),
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn evaluate_run_is_idempotent() {
        let connection = setup_conn();
        insert_terminal_run(&connection, "auto_eval_2", "run_eval_2");
        record_decision_event(
            &connection,
            "auto_eval_2",
            "run_eval_2",
            Some("step_2"),
            DecisionEventType::ApprovalApproved,
            DecisionEventMetadata {
                latency_ms: Some(1000),
                ..Default::default()
            },
            None,
        )
        .expect("event");

        let first = evaluate_run(&connection, "run_eval_2").expect("first eval");
        let second = evaluate_run(&connection, "run_eval_2").expect("second eval");
        assert_eq!(first, second);

        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM run_evaluations WHERE run_id = 'run_eval_2'",
                [],
                |row| row.get(0),
            )
            .expect("count eval");
        assert_eq!(count, 1);
    }

    #[test]
    fn adaptation_stays_within_allowed_bounds() {
        let connection = setup_conn();
        insert_terminal_run(&connection, "auto_adapt", "run_adapt");
        evaluate_run(&connection, "run_adapt").expect("eval");

        for i in 0..4 {
            let run_id = format!("run_adapt_event_{i}");
            insert_terminal_run(&connection, "auto_adapt", &run_id);
            record_decision_event(
                &connection,
                "auto_adapt",
                &run_id,
                Some("step_2"),
                DecisionEventType::ApprovalRejected,
                DecisionEventMetadata::default(),
                None,
            )
            .expect("event");
            evaluate_run(&connection, &run_id).expect("eval run");
        }

        let summary = adapt_autopilot(
            &connection,
            "auto_adapt",
            "run_adapt",
            RecipeKind::WebsiteMonitor,
        )
        .expect("adapt");
        assert!(summary
            .rationale_codes
            .iter()
            .all(|code| !code.contains("primitive")));

        let runtime = get_runtime_profile(&connection, "auto_adapt").expect("runtime profile");
        assert!(runtime.min_diff_score_to_notify >= 0.1);
        assert!(runtime.min_diff_score_to_notify <= 0.9);
    }

    #[test]
    fn memory_context_is_bounded_and_no_raw_content() {
        let connection = setup_conn();
        insert_terminal_run(&connection, "auto_mem", "run_mem");

        for _ in 0..3 {
            record_decision_event(
                &connection,
                "auto_mem",
                "run_mem",
                Some("step_3"),
                DecisionEventType::DraftEdited,
                DecisionEventMetadata {
                    draft_length: Some(800),
                    ..Default::default()
                },
                None,
            )
            .expect("draft edited event");
        }

        evaluate_run(&connection, "run_mem").expect("eval");
        update_memory_cards(&connection, "auto_mem", "run_mem", RecipeKind::InboxTriage)
            .expect("update memory cards");

        let context = build_memory_context(&connection, "auto_mem", RecipeKind::InboxTriage)
            .expect("build memory context");
        assert!(context.prompt_block.chars().count() <= MAX_MEMORY_CONTEXT_CHARS);
        assert!(context.titles.len() <= MAX_MEMORY_CONTEXT_CARDS);
        assert!(!context.prompt_block.contains("Forwarded email"));
    }

    #[test]
    fn decision_event_client_event_id_is_idempotent() {
        let connection = setup_conn();
        insert_terminal_run(&connection, "auto_event_dedupe", "run_event_dedupe");

        let payload =
            Some(r#"{"reason_code":"opened","content_hash":"abc123","content_length":20}"#);
        record_decision_event_from_json(
            &connection,
            "auto_event_dedupe",
            "run_event_dedupe",
            Some("step_1"),
            "outcome_ignored",
            payload,
            Some("client_evt_1"),
        )
        .expect("first insert");
        record_decision_event_from_json(
            &connection,
            "auto_event_dedupe",
            "run_event_dedupe",
            Some("step_1"),
            "outcome_ignored",
            payload,
            Some("client_evt_1"),
        )
        .expect("duplicate insert should be ignored");

        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM decision_events WHERE autopilot_id = 'auto_event_dedupe'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 1);
    }

    #[test]
    fn decision_event_rejects_unsafe_or_oversized_metadata() {
        let connection = setup_conn();
        insert_terminal_run(&connection, "auto_event_safe", "run_event_safe");

        let unsafe_result = record_decision_event_from_json(
            &connection,
            "auto_event_safe",
            "run_event_safe",
            Some("step_1"),
            "approval_approved",
            Some(r#"{"reason_code":"Authorization: Bearer secret"}"#),
            Some("unsafe_1"),
        );
        assert!(unsafe_result.is_err());

        let too_long = "x".repeat(300);
        let too_long_result = record_decision_event_from_json(
            &connection,
            "auto_event_safe",
            "run_event_safe",
            Some("step_1"),
            "approval_approved",
            Some(&format!(r#"{{"reason_code":"{too_long}"}}"#)),
            Some("unsafe_2"),
        );
        assert!(too_long_result.is_err());
    }

    #[test]
    fn decision_event_rate_limit_is_enforced() {
        let connection = setup_conn();
        insert_terminal_run(&connection, "auto_rate", "run_rate");

        for i in 0..DECISION_EVENTS_RATE_LIMIT_PER_MINUTE {
            record_decision_event(
                &connection,
                "auto_rate",
                "run_rate",
                Some("step_1"),
                DecisionEventType::OutcomeOpened,
                DecisionEventMetadata {
                    reason_code: Some("opened".to_string()),
                    ..Default::default()
                },
                Some(&format!("rate_evt_{i}")),
            )
            .expect("under limit");
        }

        let blocked = record_decision_event(
            &connection,
            "auto_rate",
            "run_rate",
            Some("step_1"),
            DecisionEventType::OutcomeOpened,
            DecisionEventMetadata {
                reason_code: Some("opened".to_string()),
                ..Default::default()
            },
            Some("rate_evt_blocked"),
        );
        assert!(blocked.is_err());
    }

    #[test]
    fn compaction_retains_latest_events_and_respects_limit() {
        let connection = setup_conn();
        connection
            .execute(
                "INSERT OR IGNORE INTO autopilots (id, name, created_at) VALUES ('auto_compact', 'Compact', 1)",
                [],
            )
            .expect("insert autopilot");

        for run_idx in 0..30_i64 {
            let run_id = format!("run_compact_{run_idx:02}");
            insert_terminal_run(&connection, "auto_compact", &run_id);
            connection
                .execute(
                    "UPDATE runs SET updated_at = ?1 WHERE id = ?2",
                    params![10_000 + run_idx, run_id],
                )
                .expect("set updated_at");
        }

        let mut counter = 0_i64;
        for run_idx in 0..30_i64 {
            for _ in 0..20_i64 {
                let run_id = format!("run_compact_{run_idx:02}");
                db::insert_decision_event(
                    &connection,
                    &DecisionEventInsert {
                        event_id: format!("evt_compact_{counter}"),
                        client_event_id: Some(format!("client_evt_compact_{counter}")),
                        autopilot_id: "auto_compact".to_string(),
                        run_id,
                        step_id: Some("step_1".to_string()),
                        event_type: DecisionEventType::OutcomeOpened.as_str().to_string(),
                        metadata_json: "{}".to_string(),
                        created_at_ms: now_ms() + counter,
                    },
                )
                .expect("insert event");
                counter += 1;
            }
        }

        let before: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM decision_events WHERE autopilot_id = 'auto_compact'",
                [],
                |row| row.get(0),
            )
            .expect("count before");
        assert!(before > DECISION_EVENTS_RETENTION_MAX_PER_AUTOPILOT);

        let summary =
            compact_learning_data(&connection, Some("auto_compact"), false).expect("compact now");
        assert!(summary.decision_events_deleted > 0);

        let after: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM decision_events WHERE autopilot_id = 'auto_compact'",
                [],
                |row| row.get(0),
            )
            .expect("count after");
        assert_eq!(after, DECISION_EVENTS_RETENTION_MAX_PER_AUTOPILOT);
    }

    #[test]
    fn repeated_memory_updates_do_not_create_unbounded_cards() {
        let connection = setup_conn();
        insert_terminal_run(&connection, "auto_mem_bound", "run_mem_bound");

        for i in 0..8 {
            record_decision_event(
                &connection,
                "auto_mem_bound",
                "run_mem_bound",
                Some("step_1"),
                DecisionEventType::DraftEdited,
                DecisionEventMetadata {
                    draft_length: Some(450 + i),
                    ..Default::default()
                },
                Some(&format!("mem_evt_{i}")),
            )
            .expect("insert event");
        }

        evaluate_run(&connection, "run_mem_bound").expect("evaluate");
        for _ in 0..4 {
            update_memory_cards(
                &connection,
                "auto_mem_bound",
                "run_mem_bound",
                RecipeKind::InboxTriage,
            )
            .expect("update cards");
        }

        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM memory_cards WHERE autopilot_id = 'auto_mem_bound' AND card_type = 'format_preference'",
                [],
                |row| row.get(0),
            )
            .expect("count cards");
        assert_eq!(count, 1);
    }
}
