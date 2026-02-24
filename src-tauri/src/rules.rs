use crate::learning::RuntimeProfile;
use crate::runner::RunRecord;
use crate::schema::RecipeKind;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};

static RULE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

const MAX_ACTIVE_RULES_PER_AUTOPILOT: i64 = 20;
const MAX_RULE_PROPOSALS_PER_DAY: i64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleStatus {
    Active,
    Disabled,
    PendingApproval,
    Rejected,
    Superseded,
}

impl RuleStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
            Self::PendingApproval => "pending_approval",
            Self::Rejected => "rejected",
            Self::Superseded => "superseded",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleSourceKind {
    Guidance,
    BehaviorSuggestion,
    Manual,
}

impl RuleSourceKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Guidance => "guidance",
            Self::BehaviorSuggestion => "behavior_suggestion",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleType {
    NoiseSuppression,
    DailyBriefScope,
    ReplyStyle,
    DeliveryDefaults,
    ApprovalPreference,
}

impl RuleType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoiseSuppression => "noise_suppression",
            Self::DailyBriefScope => "daily_brief_scope",
            Self::ReplyStyle => "reply_style",
            Self::DeliveryDefaults => "delivery_defaults",
            Self::ApprovalPreference => "approval_preference",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "noise_suppression" => Some(Self::NoiseSuppression),
            "daily_brief_scope" => Some(Self::DailyBriefScope),
            "reply_style" => Some(Self::ReplyStyle),
            "delivery_defaults" => Some(Self::DeliveryDefaults),
            "approval_preference" => Some(Self::ApprovalPreference),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleCardRecord {
    pub id: String,
    pub autopilot_id: String,
    pub title: String,
    pub rule_type: String,
    pub status: String,
    pub trigger_json: String,
    pub effect_json: String,
    pub source_kind: String,
    pub source_run_id: Option<String>,
    pub version: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleProposalDraft {
    pub id: String,
    pub autopilot_id: String,
    pub title: String,
    pub rule_type: String,
    pub scope: String,
    pub safety_summary: String,
    pub preview_impact: String,
    pub trigger_json: String,
    pub effect_json: String,
    pub source_kind: String,
    pub source_run_id: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleMatchRecord {
    pub rule_id: String,
    pub rule_title: String,
    pub match_reason_code: String,
    pub effect_summary: String,
}

#[derive(Debug, Clone)]
pub struct GuidanceRuleScope {
    pub scope_type: String,
    pub scope_id: String,
    pub autopilot_id: Option<String>,
    pub run_id: Option<String>,
}

#[derive(Debug, Clone)]
struct ParsedRuleProposal {
    title: String,
    rule_type: RuleType,
    trigger: Value,
    effect: Value,
    safety_summary: String,
    preview_impact: String,
}

#[derive(Debug, Clone)]
pub struct RuntimeRuleApplication {
    pub runtime_profile: RuntimeProfile,
    pub applied_rules: Vec<RuleMatchRecord>,
}

pub fn propose_rule_from_guidance(
    connection: &Connection,
    scope: GuidanceRuleScope,
    instruction: &str,
) -> Result<RuleProposalDraft, String> {
    let autopilot_id = scope.autopilot_id.clone().ok_or_else(|| {
        "Guidance must resolve to an Autopilot before it can become a rule.".to_string()
    })?;
    let recipe = resolve_recipe_for_scope(connection, &scope)?;
    ensure_proposal_rate_limit(connection, &autopilot_id)?;
    let parsed = parse_guidance_to_rule(recipe, instruction)?;
    validate_rule_effect(parsed.rule_type, &parsed.effect)?;
    enforce_active_rule_limit(connection, &autopilot_id)?;

    let now = now_ms();
    let id = make_rule_id("rule");
    let trigger_json = bounded_json(&parsed.trigger, 1000)?;
    let effect_json = bounded_json(&parsed.effect, 1000)?;
    connection
        .execute(
            "
            INSERT INTO rule_cards (
              id, autopilot_id, title, rule_type, status, trigger_json, effect_json, source_kind, source_run_id, version, created_at_ms, updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, 'pending_approval', ?5, ?6, 'guidance', ?7, 1, ?8, ?8)
            ",
            params![
                id,
                autopilot_id,
                parsed.title,
                parsed.rule_type.as_str(),
                trigger_json,
                effect_json,
                scope.run_id,
                now
            ],
        )
        .map_err(|e| format!("Failed to store rule proposal: {e}"))?;

    Ok(RuleProposalDraft {
        id,
        autopilot_id,
        title: parsed.title,
        rule_type: parsed.rule_type.as_str().to_string(),
        scope: "autopilot".to_string(),
        safety_summary: parsed.safety_summary,
        preview_impact: parsed.preview_impact,
        trigger_json,
        effect_json,
        source_kind: RuleSourceKind::Guidance.as_str().to_string(),
        source_run_id: scope.run_id,
        status: RuleStatus::PendingApproval.as_str().to_string(),
    })
}

pub fn list_rule_cards_for_autopilot(
    connection: &Connection,
    autopilot_id: &str,
) -> Result<Vec<RuleCardRecord>, String> {
    let mut stmt = connection
        .prepare(
            "
            SELECT id, autopilot_id, title, rule_type, status, trigger_json, effect_json,
                   source_kind, source_run_id, version, created_at_ms, updated_at_ms
            FROM rule_cards
            WHERE autopilot_id = ?1
            ORDER BY updated_at_ms DESC
            ",
        )
        .map_err(|e| format!("Failed to prepare rules query: {e}"))?;
    let rows = stmt
        .query_map(params![autopilot_id], |row| {
            Ok(RuleCardRecord {
                id: row.get(0)?,
                autopilot_id: row.get(1)?,
                title: row.get(2)?,
                rule_type: row.get(3)?,
                status: row.get(4)?,
                trigger_json: row.get(5)?,
                effect_json: row.get(6)?,
                source_kind: row.get(7)?,
                source_run_id: row.get(8)?,
                version: row.get(9)?,
                created_at_ms: row.get(10)?,
                updated_at_ms: row.get(11)?,
            })
        })
        .map_err(|e| format!("Failed to query rules: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("Failed to parse rule row: {e}"))?);
    }
    Ok(out)
}

pub fn get_rule_card(
    connection: &Connection,
    rule_id: &str,
) -> Result<Option<RuleCardRecord>, String> {
    connection
        .query_row(
            "
            SELECT id, autopilot_id, title, rule_type, status, trigger_json, effect_json,
                   source_kind, source_run_id, version, created_at_ms, updated_at_ms
            FROM rule_cards WHERE id = ?1 LIMIT 1
            ",
            params![rule_id],
            |row| {
                Ok(RuleCardRecord {
                    id: row.get(0)?,
                    autopilot_id: row.get(1)?,
                    title: row.get(2)?,
                    rule_type: row.get(3)?,
                    status: row.get(4)?,
                    trigger_json: row.get(5)?,
                    effect_json: row.get(6)?,
                    source_kind: row.get(7)?,
                    source_run_id: row.get(8)?,
                    version: row.get(9)?,
                    created_at_ms: row.get(10)?,
                    updated_at_ms: row.get(11)?,
                })
            },
        )
        .optional()
        .map_err(|e| format!("Failed to fetch rule card: {e}"))
}

pub fn approve_rule_proposal(
    connection: &Connection,
    rule_id: &str,
) -> Result<RuleCardRecord, String> {
    transition_rule_status(
        connection,
        rule_id,
        RuleStatus::PendingApproval,
        RuleStatus::Active,
    )
}

pub fn reject_rule_proposal(
    connection: &Connection,
    rule_id: &str,
) -> Result<RuleCardRecord, String> {
    transition_rule_status(
        connection,
        rule_id,
        RuleStatus::PendingApproval,
        RuleStatus::Rejected,
    )
}

pub fn disable_rule_card(connection: &Connection, rule_id: &str) -> Result<RuleCardRecord, String> {
    transition_rule_status(
        connection,
        rule_id,
        RuleStatus::Active,
        RuleStatus::Disabled,
    )
}

pub fn enable_rule_card(connection: &Connection, rule_id: &str) -> Result<RuleCardRecord, String> {
    transition_rule_status(
        connection,
        rule_id,
        RuleStatus::Disabled,
        RuleStatus::Active,
    )
}

pub fn apply_runtime_rules(
    connection: &Connection,
    run: &RunRecord,
    runtime_profile: &RuntimeProfile,
) -> Result<RuntimeRuleApplication, String> {
    let cards = load_active_rules(connection, &run.autopilot_id)?;
    let mut next = runtime_profile.clone();
    let mut applied = Vec::new();

    for card in cards {
        let Some(rule_type) = RuleType::parse(&card.rule_type) else {
            continue;
        };
        let trigger: Value = serde_json::from_str(&card.trigger_json).unwrap_or_else(|_| json!({}));
        let effect: Value = serde_json::from_str(&card.effect_json).unwrap_or_else(|_| json!({}));
        if !rule_matches_run(rule_type, &trigger, run) {
            continue;
        }
        let mut effect_summary = String::new();
        match rule_type {
            RuleType::NoiseSuppression => {
                if let Some(v) = effect
                    .get("min_diff_score_to_notify")
                    .and_then(|v| v.as_f64())
                {
                    next.min_diff_score_to_notify = v.clamp(0.1, 0.9);
                    effect_summary = format!(
                        "min_diff_score_to_notify={:.2}",
                        next.min_diff_score_to_notify
                    );
                }
            }
            RuleType::DailyBriefScope => {
                if let Some(v) = effect.get("max_sources").and_then(|v| v.as_u64()) {
                    next.max_sources = (v as usize).clamp(2, 10);
                }
                if let Some(v) = effect.get("max_bullets").and_then(|v| v.as_u64()) {
                    next.max_bullets = (v as usize).clamp(3, 10);
                }
                effect_summary = format!(
                    "max_sources={}, max_bullets={}",
                    next.max_sources, next.max_bullets
                );
            }
            RuleType::ReplyStyle => {
                if let Some(hint) = effect.get("reply_length_hint").and_then(|v| v.as_str()) {
                    if matches!(hint, "short" | "medium") {
                        next.reply_length_hint = hint.to_string();
                        effect_summary = format!("reply_length_hint={hint}");
                    }
                }
            }
            RuleType::DeliveryDefaults | RuleType::ApprovalPreference => {
                continue;
            }
        }

        if effect_summary.is_empty() {
            continue;
        }

        let match_reason = "preflight_rule_overlay";
        let _ = connection.execute(
            "
            INSERT INTO rule_match_events (
              id, run_id, step_id, rule_id, rule_title, match_reason_code, effect_applied_json, created_at_ms
            ) VALUES (?1, ?2, '', ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(run_id, step_id, rule_id) DO NOTHING
            ",
            params![
                make_rule_id("rule_match"),
                run.id,
                card.id,
                card.title,
                match_reason,
                bounded_json(&json!({"summary": effect_summary}), 400).unwrap_or_else(|_| "{\"summary\":\"applied\"}".to_string()),
                now_ms()
            ],
        );

        applied.push(RuleMatchRecord {
            rule_id: card.id,
            rule_title: card.title,
            match_reason_code: match_reason.to_string(),
            effect_summary,
        });
    }

    Ok(RuntimeRuleApplication {
        runtime_profile: next,
        applied_rules: applied,
    })
}

pub fn list_applied_rules_for_run(
    connection: &Connection,
    run_id: &str,
) -> Result<Vec<RuleMatchRecord>, String> {
    let mut stmt = connection
        .prepare(
            "
            SELECT rule_id, rule_title, match_reason_code, effect_applied_json
            FROM rule_match_events
            WHERE run_id = ?1
            ORDER BY created_at_ms ASC
            ",
        )
        .map_err(|e| format!("Failed to prepare rule match query: {e}"))?;
    let rows = stmt
        .query_map(params![run_id], |row| {
            let effect_json: String = row.get(3)?;
            let effect_summary = serde_json::from_str::<Value>(&effect_json)
                .ok()
                .and_then(|v| {
                    v.get("summary")
                        .and_then(|s| s.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| "applied".to_string());
            Ok(RuleMatchRecord {
                rule_id: row.get(0)?,
                rule_title: row.get(1)?,
                match_reason_code: row.get(2)?,
                effect_summary,
            })
        })
        .map_err(|e| format!("Failed to query rule matches: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        let record = row.map_err(|e| format!("Failed to parse rule match row: {e}"))?;
        if !out
            .iter()
            .any(|x: &RuleMatchRecord| x.rule_id == record.rule_id)
        {
            out.push(record);
        }
    }
    Ok(out)
}

fn load_active_rules(
    connection: &Connection,
    autopilot_id: &str,
) -> Result<Vec<RuleCardRecord>, String> {
    let mut all = list_rule_cards_for_autopilot(connection, autopilot_id)?;
    all.retain(|r| r.status == RuleStatus::Active.as_str());
    Ok(all)
}

fn transition_rule_status(
    connection: &Connection,
    rule_id: &str,
    from: RuleStatus,
    to: RuleStatus,
) -> Result<RuleCardRecord, String> {
    let changed = connection
        .execute(
            "UPDATE rule_cards SET status = ?1, updated_at_ms = ?2 WHERE id = ?3 AND status = ?4",
            params![to.as_str(), now_ms(), rule_id, from.as_str()],
        )
        .map_err(|e| format!("Failed to update rule status: {e}"))?;
    if changed == 0 {
        return Err("Rule is not in the expected state for this action.".to_string());
    }
    get_rule_card(connection, rule_id)?
        .ok_or_else(|| "Rule could not be reloaded after update.".to_string())
}

fn resolve_recipe_for_scope(
    connection: &Connection,
    scope: &GuidanceRuleScope,
) -> Result<RecipeKind, String> {
    if let Some(run_id) = &scope.run_id {
        return recipe_for_run(connection, run_id);
    }
    let Some(autopilot_id) = &scope.autopilot_id else {
        return Err("Could not determine which Autopilot this rule belongs to.".to_string());
    };
    let latest_run_id: Option<String> = connection
        .query_row(
            "SELECT id FROM runs WHERE autopilot_id = ?1 ORDER BY created_at DESC LIMIT 1",
            params![autopilot_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("Failed to resolve Autopilot recipe: {e}"))?;
    let Some(run_id) = latest_run_id else {
        return Err("Run this Autopilot once before teaching a reusable rule.".to_string());
    };
    recipe_for_run(connection, &run_id)
}

fn recipe_for_run(connection: &Connection, run_id: &str) -> Result<RecipeKind, String> {
    let plan_json: String = connection
        .query_row(
            "SELECT plan_json FROM runs WHERE id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to load run plan for rule parsing: {e}"))?;
    let plan: crate::schema::AutopilotPlan =
        serde_json::from_str(&plan_json).map_err(|e| format!("Invalid run plan JSON: {e}"))?;
    Ok(plan.recipe)
}

fn parse_guidance_to_rule(
    recipe: RecipeKind,
    instruction: &str,
) -> Result<ParsedRuleProposal, String> {
    let lowered = instruction.to_ascii_lowercase();
    reject_prohibited_effect_text(&lowered)?;

    match recipe {
        RecipeKind::WebsiteMonitor => {
            if lowered.contains("ignore") || lowered.contains("minor") || lowered.contains("tiny") {
                let threshold = if lowered.contains("tiny") { 0.4 } else { 0.3 };
                let suppress_hours = if lowered.contains("24h") || lowered.contains("24 hour") {
                    24
                } else {
                    24
                };
                return Ok(ParsedRuleProposal {
                    title: "Ignore minor website changes".to_string(),
                    rule_type: RuleType::NoiseSuppression,
                    trigger: json!({"recipe":"website_monitor"}),
                    effect: json!({"min_diff_score_to_notify": threshold, "suppress_hours": suppress_hours}),
                    safety_summary: "Narrows when Terminus notifies you about changes. Does not enable sending or new capabilities.".to_string(),
                    preview_impact: "Likely fewer low-value website change approvals and notifications.".to_string(),
                });
            }
        }
        RecipeKind::DailyBrief => {
            if lowered.contains("brief")
                || lowered.contains("sources")
                || lowered.contains("bullet")
                || lowered.contains("short")
            {
                let max_sources = extract_small_number(&lowered).unwrap_or(4).clamp(2, 10);
                let max_bullets = if lowered.contains("bullet") {
                    extract_small_number_after_keyword(&lowered, "bullet")
                        .unwrap_or(max_sources)
                        .clamp(3, 10)
                } else {
                    4
                };
                let prefer_official_sources = lowered.contains("official");
                return Ok(ParsedRuleProposal {
                    title: format!("Daily brief keep to {max_sources} sources"),
                    rule_type: RuleType::DailyBriefScope,
                    trigger: json!({"recipe":"daily_brief"}),
                    effect: json!({"max_sources": max_sources, "max_bullets": max_bullets, "prefer_official_sources": prefer_official_sources}),
                    safety_summary: "Reduces Daily Brief scope and output length only. Does not change send policy or allowlists.".to_string(),
                    preview_impact: "Likely lower cost and shorter briefs on future runs.".to_string(),
                });
            }
        }
        RecipeKind::InboxTriage => {
            if lowered.contains("shorter repl")
                || lowered.contains("keep replies short")
                || lowered.contains("concise repl")
                || lowered.contains("short replies")
            {
                let tone_hint = if lowered.contains("warm") {
                    Some("warm")
                } else if lowered.contains("professional") {
                    Some("professional")
                } else {
                    None
                };
                return Ok(ParsedRuleProposal {
                    title: "Keep replies shorter".to_string(),
                    rule_type: RuleType::ReplyStyle,
                    trigger: json!({"recipe":"inbox_triage"}),
                    effect: json!({"reply_length_hint":"short","tone_hint": tone_hint.unwrap_or("neutral")}),
                    safety_summary:
                        "Changes reply style only. Does not enable sending or change recipients."
                            .to_string(),
                    preview_impact: "Future reply drafts will be more concise for this Autopilot."
                        .to_string(),
                });
            }
        }
    }

    Err("Terminus couldn't turn that into a safe reusable rule yet. Try a more specific request like “keep replies short” or “ignore minor website changes.”".to_string())
}

fn reject_prohibited_effect_text(lowered: &str) -> Result<(), String> {
    let prohibited = [
        "enable send",
        "enable sending",
        "disable approval",
        "add recipient",
        "allowlist",
        "domain allowlist",
        "send to anyone",
        "new primitive",
        "run shell",
        "execute code",
    ];
    if prohibited.iter().any(|t| lowered.contains(t)) {
        return Err(
            "That request would expand protected capabilities. Terminus can’t create a reusable rule for it."
                .to_string(),
        );
    }
    Ok(())
}

fn validate_rule_effect(rule_type: RuleType, effect: &Value) -> Result<(), String> {
    match rule_type {
        RuleType::NoiseSuppression => {
            let threshold = effect
                .get("min_diff_score_to_notify")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| "Rule is missing min_diff_score_to_notify.".to_string())?;
            if !(0.1..=0.9).contains(&threshold) {
                return Err("min_diff_score_to_notify must be between 0.1 and 0.9.".to_string());
            }
            let suppress_hours = effect
                .get("suppress_hours")
                .and_then(|v| v.as_i64())
                .unwrap_or(24);
            if !(1..=168).contains(&suppress_hours) {
                return Err("suppress_hours must be between 1 and 168.".to_string());
            }
        }
        RuleType::DailyBriefScope => {
            let max_sources = effect
                .get("max_sources")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| "Rule is missing max_sources.".to_string())?;
            let max_bullets = effect
                .get("max_bullets")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| "Rule is missing max_bullets.".to_string())?;
            if !(2..=10).contains(&max_sources) {
                return Err("max_sources must be between 2 and 10.".to_string());
            }
            if !(3..=10).contains(&max_bullets) {
                return Err("max_bullets must be between 3 and 10.".to_string());
            }
        }
        RuleType::ReplyStyle => {
            let hint = effect
                .get("reply_length_hint")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Rule is missing reply_length_hint.".to_string())?;
            if !matches!(hint, "short" | "medium") {
                return Err("reply_length_hint must be short or medium.".to_string());
            }
            if let Some(tone) = effect.get("tone_hint").and_then(|v| v.as_str()) {
                if !matches!(tone, "neutral" | "warm" | "professional") {
                    return Err("tone_hint must be neutral, warm, or professional.".to_string());
                }
            }
        }
        RuleType::DeliveryDefaults | RuleType::ApprovalPreference => {
            return Err("This rule type is not implemented in MVP yet.".to_string())
        }
    }
    Ok(())
}

fn rule_matches_run(rule_type: RuleType, trigger: &Value, run: &RunRecord) -> bool {
    let recipe_ok = trigger
        .get("recipe")
        .and_then(|v| v.as_str())
        .map(|recipe| recipe == recipe_name(run.plan.recipe))
        .unwrap_or(true);
    if !recipe_ok {
        return false;
    }
    match rule_type {
        RuleType::NoiseSuppression => matches!(run.plan.recipe, RecipeKind::WebsiteMonitor),
        RuleType::DailyBriefScope => matches!(run.plan.recipe, RecipeKind::DailyBrief),
        RuleType::ReplyStyle => matches!(run.plan.recipe, RecipeKind::InboxTriage),
        RuleType::DeliveryDefaults | RuleType::ApprovalPreference => false,
    }
}

fn recipe_name(recipe: RecipeKind) -> &'static str {
    match recipe {
        RecipeKind::WebsiteMonitor => "website_monitor",
        RecipeKind::InboxTriage => "inbox_triage",
        RecipeKind::DailyBrief => "daily_brief",
    }
}

fn ensure_proposal_rate_limit(connection: &Connection, autopilot_id: &str) -> Result<(), String> {
    let cutoff = now_ms().saturating_sub(86_400_000);
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM rule_cards WHERE autopilot_id = ?1 AND status = 'pending_approval' AND created_at_ms >= ?2",
            params![autopilot_id, cutoff],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to check rule proposal rate limit: {e}"))?;
    if count >= MAX_RULE_PROPOSALS_PER_DAY {
        return Err("Too many rule proposals for this Autopilot today. Review or reject existing ones first.".to_string());
    }
    Ok(())
}

fn enforce_active_rule_limit(connection: &Connection, autopilot_id: &str) -> Result<(), String> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM rule_cards WHERE autopilot_id = ?1 AND status = 'active'",
            params![autopilot_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to count active rules: {e}"))?;
    if count >= MAX_ACTIVE_RULES_PER_AUTOPILOT {
        return Err("This Autopilot already has the maximum number of active rules.".to_string());
    }
    Ok(())
}

fn bounded_json(value: &Value, max_bytes: usize) -> Result<String, String> {
    let encoded =
        serde_json::to_string(value).map_err(|e| format!("Failed to encode JSON: {e}"))?;
    if encoded.as_bytes().len() > max_bytes {
        return Err("Rule payload is too large.".to_string());
    }
    Ok(encoded)
}

fn extract_small_number(text: &str) -> Option<i64> {
    text.split(|c: char| !c.is_ascii_digit()).find_map(|part| {
        if part.is_empty() {
            None
        } else {
            part.parse::<i64>().ok()
        }
    })
}

fn extract_small_number_after_keyword(text: &str, keyword: &str) -> Option<i64> {
    text.find(keyword)
        .and_then(|idx| extract_small_number(&text[idx..]))
}

fn make_rule_id(prefix: &str) -> String {
    let ts = now_ms();
    let n = RULE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{ts}_{n}")
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_millis() as i64,
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{bootstrap_schema, configure_connection};
    use crate::schema::{AutopilotPlan, ProviderId};

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        configure_connection(&conn).unwrap();
        bootstrap_schema(&mut conn).unwrap();
        conn
    }

    #[test]
    fn parses_safe_daily_brief_rule() {
        let parsed = parse_guidance_to_rule(
            RecipeKind::DailyBrief,
            "Keep Daily Brief to 4 sources and 4 bullets",
        )
        .expect("parse");
        assert_eq!(parsed.rule_type.as_str(), "daily_brief_scope");
    }

    #[test]
    fn rejects_capability_expanding_rule_text() {
        let err = parse_guidance_to_rule(
            RecipeKind::InboxTriage,
            "Enable sending to anyone automatically",
        )
        .expect_err("must reject");
        assert!(err.contains("protected capabilities"));
    }

    #[test]
    fn rule_proposal_and_activation_round_trip() {
        let conn = test_conn();
        conn.execute(
            "INSERT INTO autopilots (id, name, created_at) VALUES ('auto_1', 'Auto', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO runs (id, autopilot_id, idempotency_key, plan_json, state, created_at, updated_at)
             VALUES ('run_1','auto_1','k1',?1,'succeeded',1,1)",
            params![serde_json::to_string(&AutopilotPlan::from_intent(
                RecipeKind::DailyBrief,
                "x".to_string(),
                ProviderId::OpenAi,
            ))
            .unwrap()],
        )
        .unwrap();
        let proposal = propose_rule_from_guidance(
            &conn,
            GuidanceRuleScope {
                scope_type: "autopilot".to_string(),
                scope_id: "auto_1".to_string(),
                autopilot_id: Some("auto_1".to_string()),
                run_id: None,
            },
            "Keep Daily Brief to 4 sources and 4 bullets",
        )
        .expect("proposal");
        assert_eq!(proposal.status, "pending_approval");
        let active = approve_rule_proposal(&conn, &proposal.id).expect("approve");
        assert_eq!(active.status, "active");
    }
}
