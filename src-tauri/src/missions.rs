use crate::runner::{RunState, RunnerEngine};
use crate::schema::{AutopilotPlan, PlanStep, PrimitiveId, ProviderId, RecipeKind};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static MISSION_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissionTemplateKind {
    DailyBriefMultiSource,
}

impl MissionTemplateKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::DailyBriefMultiSource => "daily_brief_multi_source",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "daily_brief_multi_source" => Ok(Self::DailyBriefMultiSource),
            _ => Err("Unsupported mission template.".to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    Draft,
    Running,
    WaitingChildren,
    Aggregating,
    Succeeded,
    Failed,
    Blocked,
}

impl MissionStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Running => "running",
            Self::WaitingChildren => "waiting_children",
            Self::Aggregating => "aggregating",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "draft" => Ok(Self::Draft),
            "running" => Ok(Self::Running),
            "waiting_children" => Ok(Self::WaitingChildren),
            "aggregating" => Ok(Self::Aggregating),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "blocked" => Ok(Self::Blocked),
            _ => Err(format!("Unknown mission state: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionDraft {
    pub template_kind: MissionTemplateKind,
    pub provider: String,
    pub intent: String,
    pub source_groups: Vec<MissionSourceGroup>,
    pub preview: MissionDraftPreview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionDraftPreview {
    pub child_runs: usize,
    pub contract: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionSourceGroup {
    pub child_key: String,
    pub label: String,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMissionDraftInput {
    pub template_kind: String,
    pub intent: String,
    pub provider: Option<String>,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartMissionInput {
    pub draft: MissionDraft,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionRecord {
    pub id: String,
    pub template_kind: MissionTemplateKind,
    pub status: MissionStatus,
    pub provider: String,
    pub failure_reason: Option<String>,
    pub child_runs_count: i64,
    pub terminal_children_count: i64,
    pub summary_json: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionRunLink {
    pub child_key: String,
    pub source_label: Option<String>,
    pub run_id: String,
    pub run_role: String,
    pub status: String,
    pub run_state: Option<String>,
    pub run_failure_reason: Option<String>,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionEventRecord {
    pub id: String,
    pub event_type: String,
    pub summary: String,
    pub details_json: String,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionContractStatus {
    pub all_children_terminal: bool,
    pub has_blocked_or_pending_child: bool,
    pub aggregation_summary_exists: bool,
    pub ready_to_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionDetail {
    pub mission: MissionRecord,
    pub child_runs: Vec<MissionRunLink>,
    pub events: Vec<MissionEventRecord>,
    pub contract: MissionContractStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionTickResult {
    pub mission: MissionDetail,
    pub child_runs_ticked: usize,
}

pub fn create_mission_draft(input: CreateMissionDraftInput) -> Result<MissionDraft, String> {
    let template_kind = MissionTemplateKind::parse(input.template_kind.trim())?;
    let intent = input.intent.trim().to_string();
    if intent.is_empty() {
        return Err("Add a mission intent first.".to_string());
    }
    let provider = input
        .provider
        .as_deref()
        .unwrap_or("openai")
        .trim()
        .to_string();
    let cleaned_sources = input
        .sources
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<String>>();
    if cleaned_sources.is_empty() {
        return Err("Add at least one source for this mission.".to_string());
    }
    if cleaned_sources.len() > 10 {
        return Err("Keep mission sources to 10 or fewer for MVP.".to_string());
    }

    let source_groups = match template_kind {
        MissionTemplateKind::DailyBriefMultiSource => cleaned_sources
            .into_iter()
            .enumerate()
            .map(|(idx, source)| MissionSourceGroup {
                child_key: format!("child_{}", idx + 1),
                label: summarize_source_label(&source),
                sources: vec![source],
            })
            .collect::<Vec<_>>(),
    };

    Ok(MissionDraft {
        template_kind,
        provider,
        intent,
        preview: MissionDraftPreview {
            child_runs: source_groups.len(),
            contract: "All child runs must finish without blocked/pending states before aggregation completes.".to_string(),
            note: "This MVP mission fans out into child runs, then aggregates a deterministic summary.".to_string(),
        },
        source_groups,
    })
}

pub fn start_mission(
    connection: &mut Connection,
    input: StartMissionInput,
) -> Result<MissionDetail, String> {
    validate_mission_draft(&input.draft)?;
    let mission_id = make_id("mission");
    let mission_key = input.idempotency_key.unwrap_or_else(|| {
        format!(
            "mission:{}:{}",
            input.draft.template_kind.as_str(),
            mission_id
        )
    });
    let now = now_ms();
    let provider_id = parse_provider(&input.draft.provider)?;
    let config_json = serde_json::to_string(&input.draft).map_err(|e| e.to_string())?;

    let tx = connection
        .transaction()
        .map_err(|e| format!("Failed to start mission transaction: {e}"))?;
    tx.execute(
        "INSERT INTO missions (id, template_kind, idempotency_key, status, provider_kind, config_json, summary_json, failure_reason, created_at_ms, updated_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, ?7, ?7)",
        params![
            mission_id,
            input.draft.template_kind.as_str(),
            mission_key,
            MissionStatus::Running.as_str(),
            input.draft.provider,
            config_json,
            now
        ],
    )
    .map_err(|e| format!("Failed to create mission: {e}"))?;
    insert_mission_event_tx(
        &tx,
        &mission_id,
        "mission_started",
        "Mission created. Preparing child runs.",
        json!({"childCount": input.draft.source_groups.len()}),
        now,
    )?;
    tx.commit()
        .map_err(|e| format!("Failed to commit mission creation: {e}"))?;

    let mut created_children = 0usize;
    for group in &input.draft.source_groups {
        let child_autopilot_id = format!("{}_{}", mission_id, group.child_key);
        let child_idempotency_key = format!("mission:{}:{}", mission_id, group.child_key);
        let plan = build_daily_brief_child_plan(&input.draft.intent, provider_id, &group.sources);
        let run = RunnerEngine::start_run(
            connection,
            &child_autopilot_id,
            plan,
            &child_idempotency_key,
            2,
        )
        .map_err(|e| e.to_string())?;

        connection
            .execute(
                "INSERT INTO mission_runs (id, mission_id, child_key, run_id, run_role, source_label, status, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, 'child', ?5, ?6, ?7, ?7)",
                params![
                    make_id("mission_run"),
                    mission_id,
                    group.child_key,
                    run.id,
                    group.label,
                    run.state.as_str(),
                    now_ms()
                ],
            )
            .map_err(|e| format!("Failed to link child run to mission: {e}"))?;
        created_children += 1;
    }

    update_mission_status(
        connection,
        &mission_id,
        MissionStatus::WaitingChildren,
        None,
        None,
        "Child runs created. Waiting for child completion.",
        json!({"childRunsCreated": created_children}),
    )?;

    get_mission(connection, &mission_id)
}

pub fn list_missions(connection: &Connection, limit: usize) -> Result<Vec<MissionRecord>, String> {
    let mut stmt = connection
        .prepare(
            "
            SELECT m.id, m.template_kind, m.status, m.provider_kind, m.failure_reason, m.summary_json,
                   m.created_at_ms, m.updated_at_ms,
                   COALESCE((SELECT COUNT(*) FROM mission_runs mr WHERE mr.mission_id = m.id), 0) AS child_count,
                   COALESCE((SELECT COUNT(*) FROM mission_runs mr
                             JOIN runs r ON r.id = mr.run_id
                             WHERE mr.mission_id = m.id AND r.state IN ('succeeded','failed','blocked','canceled')), 0) AS terminal_count
            FROM missions m
            ORDER BY m.updated_at_ms DESC
            LIMIT ?1
            ",
        )
        .map_err(|e| format!("Failed to prepare missions list: {e}"))?;

    let rows = stmt
        .query_map(params![limit as i64], map_mission_row)
        .map_err(|e| format!("Failed to query missions: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("Failed to parse mission row: {e}"))?);
    }
    Ok(out)
}

pub fn get_mission(connection: &Connection, mission_id: &str) -> Result<MissionDetail, String> {
    let mission = connection
        .query_row(
            "
            SELECT m.id, m.template_kind, m.status, m.provider_kind, m.failure_reason, m.summary_json,
                   m.created_at_ms, m.updated_at_ms,
                   COALESCE((SELECT COUNT(*) FROM mission_runs mr WHERE mr.mission_id = m.id), 0) AS child_count,
                   COALESCE((SELECT COUNT(*) FROM mission_runs mr
                             JOIN runs r ON r.id = mr.run_id
                             WHERE mr.mission_id = m.id AND r.state IN ('succeeded','failed','blocked','canceled')), 0) AS terminal_count
            FROM missions m
            WHERE m.id = ?1
            ",
            params![mission_id],
            map_mission_row,
        )
        .map_err(|e| format!("Mission not found: {e}"))?;

    let mut child_stmt = connection
        .prepare(
            "
            SELECT mr.child_key, mr.source_label, mr.run_id, mr.run_role, mr.status, mr.updated_at_ms,
                   r.state, r.failure_reason
            FROM mission_runs mr
            LEFT JOIN runs r ON r.id = mr.run_id
            WHERE mr.mission_id = ?1
            ORDER BY mr.child_key ASC
            ",
        )
        .map_err(|e| format!("Failed to prepare mission child list: {e}"))?;
    let child_iter = child_stmt
        .query_map(params![mission_id], |row| {
            Ok(MissionRunLink {
                child_key: row.get(0)?,
                source_label: row.get(1)?,
                run_id: row.get(2)?,
                run_role: row.get(3)?,
                status: row.get(4)?,
                updated_at_ms: row.get(5)?,
                run_state: row.get::<_, Option<String>>(6)?,
                run_failure_reason: row.get(7)?,
            })
        })
        .map_err(|e| format!("Failed to query mission children: {e}"))?;
    let mut child_runs = Vec::new();
    for row in child_iter {
        child_runs.push(row.map_err(|e| format!("Failed to parse mission child: {e}"))?);
    }

    let mut events_stmt = connection
        .prepare(
            "
            SELECT id, event_type, summary, details_json, created_at_ms
            FROM mission_events
            WHERE mission_id = ?1
            ORDER BY created_at_ms DESC
            LIMIT 25
            ",
        )
        .map_err(|e| format!("Failed to prepare mission events list: {e}"))?;
    let events_iter = events_stmt
        .query_map(params![mission_id], |row| {
            Ok(MissionEventRecord {
                id: row.get(0)?,
                event_type: row.get(1)?,
                summary: row.get(2)?,
                details_json: row.get(3)?,
                created_at_ms: row.get(4)?,
            })
        })
        .map_err(|e| format!("Failed to query mission events: {e}"))?;
    let mut events = Vec::new();
    for row in events_iter {
        events.push(row.map_err(|e| format!("Failed to parse mission event: {e}"))?);
    }

    let contract = build_contract_status(&mission, &child_runs);

    Ok(MissionDetail {
        mission,
        child_runs,
        events,
        contract,
    })
}

pub fn run_mission_tick(
    connection: &mut Connection,
    mission_id: &str,
) -> Result<MissionTickResult, String> {
    let mission = get_mission(connection, mission_id)?;
    let mut child_runs_ticked = 0usize;

    if matches!(
        mission.mission.status,
        MissionStatus::Succeeded | MissionStatus::Failed | MissionStatus::Blocked
    ) {
        return Ok(MissionTickResult {
            mission,
            child_runs_ticked,
        });
    }

    for child in &mission.child_runs {
        let Some(state_text) = child.run_state.as_deref() else {
            continue;
        };
        let run_state = RunState::from_str(state_text).map_err(|e| e.to_string())?;
        match run_state {
            RunState::Ready | RunState::Running | RunState::Retrying => {
                let _ =
                    RunnerEngine::run_tick(connection, &child.run_id).map_err(|e| e.to_string())?;
                child_runs_ticked += 1;
            }
            RunState::NeedsApproval | RunState::NeedsClarification | RunState::Blocked => {}
            RunState::Succeeded | RunState::Failed | RunState::Canceled => {}
        }
    }

    let refreshed = get_mission(connection, mission_id)?;
    let contract = refreshed.contract.clone();

    if contract.has_blocked_or_pending_child {
        let detail = refreshed
            .child_runs
            .iter()
            .find(|c| {
                matches!(
                    c.run_state.as_deref(),
                    Some("needs_approval" | "needs_clarification" | "blocked")
                )
            })
            .map(|c| {
                format!(
                    "Child {} requires attention before aggregation.",
                    c.child_key
                )
            })
            .unwrap_or_else(|| "A child run requires attention before aggregation.".to_string());
        update_mission_status(
            connection,
            mission_id,
            MissionStatus::Blocked,
            Some(&detail),
            None,
            &detail,
            json!({}),
        )?;
        let mission = get_mission(connection, mission_id)?;
        return Ok(MissionTickResult {
            mission,
            child_runs_ticked,
        });
    }

    if !contract.all_children_terminal {
        update_mission_status(
            connection,
            mission_id,
            MissionStatus::WaitingChildren,
            None,
            None,
            "Mission tick complete. Waiting for child runs.",
            json!({"childRunsTicked": child_runs_ticked}),
        )?;
        let mission = get_mission(connection, mission_id)?;
        return Ok(MissionTickResult {
            mission,
            child_runs_ticked,
        });
    }

    let any_failed = refreshed.child_runs.iter().any(|c| {
        matches!(
            c.run_state.as_deref(),
            Some("failed" | "canceled" | "blocked")
        )
    });
    if any_failed {
        update_mission_status(
            connection,
            mission_id,
            MissionStatus::Failed,
            Some(
                "One or more child runs failed. Review child receipts and retry the mission later.",
            ),
            None,
            "Mission failed because at least one child run failed.",
            json!({}),
        )?;
        let mission = get_mission(connection, mission_id)?;
        return Ok(MissionTickResult {
            mission,
            child_runs_ticked,
        });
    }

    update_mission_status(
        connection,
        mission_id,
        MissionStatus::Aggregating,
        None,
        None,
        "All child runs completed. Building mission summary.",
        json!({}),
    )?;

    let summary = build_daily_brief_multi_source_summary(connection, &refreshed)?;
    let summary_json = serde_json::to_string(&summary).map_err(|e| e.to_string())?;
    update_mission_status(
        connection,
        mission_id,
        MissionStatus::Succeeded,
        None,
        Some(&summary_json),
        "Mission aggregation complete.",
        json!({"childRuns": refreshed.child_runs.len()}),
    )?;

    let mission = get_mission(connection, mission_id)?;
    Ok(MissionTickResult {
        mission,
        child_runs_ticked,
    })
}

fn map_mission_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MissionRecord> {
    let template_kind: String = row.get(1)?;
    let status: String = row.get(2)?;
    Ok(MissionRecord {
        id: row.get(0)?,
        template_kind: MissionTemplateKind::parse(&template_kind).map_err(|_| {
            rusqlite::Error::InvalidColumnType(
                1,
                "template_kind".to_string(),
                rusqlite::types::Type::Text,
            )
        })?,
        status: MissionStatus::parse(&status).map_err(|_| {
            rusqlite::Error::InvalidColumnType(2, "status".to_string(), rusqlite::types::Type::Text)
        })?,
        provider: row.get(3)?,
        failure_reason: row.get(4)?,
        summary_json: row.get(5)?,
        created_at_ms: row.get(6)?,
        updated_at_ms: row.get(7)?,
        child_runs_count: row.get(8)?,
        terminal_children_count: row.get(9)?,
    })
}

fn build_contract_status(
    mission: &MissionRecord,
    child_runs: &[MissionRunLink],
) -> MissionContractStatus {
    let all_children_terminal = !child_runs.is_empty()
        && child_runs.iter().all(|c| {
            matches!(
                c.run_state.as_deref(),
                Some("succeeded" | "failed" | "blocked" | "canceled")
            )
        });
    let has_blocked_or_pending_child = child_runs.iter().any(|c| {
        matches!(
            c.run_state.as_deref(),
            Some("needs_approval" | "needs_clarification" | "blocked")
        )
    });
    let aggregation_summary_exists = mission
        .summary_json
        .as_ref()
        .is_some_and(|s| !s.trim().is_empty());
    MissionContractStatus {
        all_children_terminal,
        has_blocked_or_pending_child,
        aggregation_summary_exists,
        ready_to_complete: all_children_terminal
            && !has_blocked_or_pending_child
            && aggregation_summary_exists,
    }
}

fn validate_mission_draft(draft: &MissionDraft) -> Result<(), String> {
    if draft.template_kind != MissionTemplateKind::DailyBriefMultiSource {
        return Err("Only daily_brief_multi_source is available in this MVP slice.".to_string());
    }
    if draft.source_groups.is_empty() {
        return Err("Mission draft needs at least one child source group.".to_string());
    }
    for group in &draft.source_groups {
        if group.sources.is_empty() {
            return Err(format!("{} has no sources.", group.child_key));
        }
    }
    Ok(())
}

fn parse_provider(value: &str) -> Result<ProviderId, String> {
    match value {
        "openai" => Ok(ProviderId::OpenAi),
        "anthropic" => Ok(ProviderId::Anthropic),
        "gemini" => Ok(ProviderId::Gemini),
        _ => Err("Unsupported provider for mission draft.".to_string()),
    }
}

fn build_daily_brief_child_plan(
    intent: &str,
    provider: ProviderId,
    sources: &[String],
) -> AutopilotPlan {
    let mut plan = AutopilotPlan::from_intent(
        RecipeKind::DailyBrief,
        format!("{intent} (mission child)"),
        provider,
    );
    plan.daily_sources = sources.to_vec();
    // Keep mission child runs read+aggregate only to avoid approval-gated drafting at child level.
    plan.steps = plan
        .steps
        .into_iter()
        .filter(|step| {
            matches!(
                step.primitive,
                PrimitiveId::ReadSources | PrimitiveId::AggregateDailySummary
            )
        })
        .map(strip_step_approval)
        .collect::<Vec<PlanStep>>();
    if plan.steps.is_empty() {
        plan.steps = vec![];
    }
    plan
}

fn strip_step_approval(mut step: PlanStep) -> PlanStep {
    step.requires_approval = false;
    step
}

fn build_daily_brief_multi_source_summary(
    connection: &Connection,
    mission: &MissionDetail,
) -> Result<Value, String> {
    let mut child_summaries = Vec::new();
    for child in &mission.child_runs {
        let payload: Option<String> = connection
            .query_row(
                "SELECT content FROM outcomes WHERE run_id = ?1 AND kind = 'daily_summary' LIMIT 1",
                params![child.run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("Failed to load child daily summary artifact: {e}"))?;
        let parsed: Value = payload
            .as_deref()
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .unwrap_or_else(|| json!({}));
        let title = parsed
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Daily Brief child summary");
        let bullets = parsed
            .get("bullet_points")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .take(3)
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        child_summaries.push(json!({
            "childKey": child.child_key,
            "sourceLabel": child.source_label,
            "runId": child.run_id,
            "title": title,
            "bullets": bullets,
        }));
    }

    let aggregated_title = format!("Mission brief: {} source updates", child_summaries.len());
    let rollup = child_summaries
        .iter()
        .take(6)
        .map(|item| {
            let source = item
                .get("sourceLabel")
                .and_then(|v| v.as_str())
                .unwrap_or("Source");
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Summary");
            format!("{source}: {title}")
        })
        .collect::<Vec<String>>();

    Ok(json!({
        "templateKind": mission.mission.template_kind,
        "title": aggregated_title,
        "summaryLines": rollup,
        "children": child_summaries,
        "generatedAtMs": now_ms()
    }))
}

fn update_mission_status(
    connection: &Connection,
    mission_id: &str,
    status: MissionStatus,
    failure_reason: Option<&str>,
    summary_json: Option<&str>,
    event_summary: &str,
    event_details: Value,
) -> Result<(), String> {
    let now = now_ms();
    connection
        .execute(
            "UPDATE missions
             SET status = ?1,
                 failure_reason = ?2,
                 summary_json = COALESCE(?3, summary_json),
                 updated_at_ms = ?4
             WHERE id = ?5",
            params![
                status.as_str(),
                failure_reason,
                summary_json,
                now,
                mission_id
            ],
        )
        .map_err(|e| format!("Failed to update mission state: {e}"))?;
    insert_mission_event(
        connection,
        mission_id,
        &format!("state_{}", status.as_str()),
        event_summary,
        event_details,
        now,
    )?;
    // Keep mission_runs.status in sync with child run state snapshots.
    refresh_mission_run_status_snapshots(connection, mission_id)?;
    Ok(())
}

fn refresh_mission_run_status_snapshots(
    connection: &Connection,
    mission_id: &str,
) -> Result<(), String> {
    let mut stmt = connection
        .prepare("SELECT run_id FROM mission_runs WHERE mission_id = ?1")
        .map_err(|e| format!("Failed to prepare mission run snapshot refresh: {e}"))?;
    let run_ids = stmt
        .query_map(params![mission_id], |r| r.get::<_, String>(0))
        .map_err(|e| format!("Failed to query mission run ids: {e}"))?;
    for run_id in run_ids {
        let run_id = run_id.map_err(|e| format!("Failed to parse mission run id: {e}"))?;
        if let Ok(run) = RunnerEngine::get_run(connection, &run_id) {
            connection
                .execute(
                    "UPDATE mission_runs SET status = ?1, updated_at_ms = ?2 WHERE mission_id = ?3 AND run_id = ?4",
                    params![run.state.as_str(), now_ms(), mission_id, run_id],
                )
                .map_err(|e| format!("Failed to refresh mission run status: {e}"))?;
        }
    }
    Ok(())
}

fn insert_mission_event(
    connection: &Connection,
    mission_id: &str,
    event_type: &str,
    summary: &str,
    details_json: Value,
    created_at_ms: i64,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO mission_events (id, mission_id, event_type, summary, details_json, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                make_id("mission_event"),
                mission_id,
                event_type,
                truncate(summary, 240),
                serde_json::to_string(&details_json).unwrap_or_else(|_| "{}".to_string()),
                created_at_ms
            ],
        )
        .map_err(|e| format!("Failed to insert mission event: {e}"))?;
    Ok(())
}

fn insert_mission_event_tx(
    tx: &rusqlite::Transaction<'_>,
    mission_id: &str,
    event_type: &str,
    summary: &str,
    details_json: Value,
    created_at_ms: i64,
) -> Result<(), String> {
    tx.execute(
        "INSERT INTO mission_events (id, mission_id, event_type, summary, details_json, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            make_id("mission_event"),
            mission_id,
            event_type,
            truncate(summary, 240),
            serde_json::to_string(&details_json).unwrap_or_else(|_| "{}".to_string()),
            created_at_ms
        ],
    )
    .map_err(|e| format!("Failed to insert mission event: {e}"))?;
    Ok(())
}

fn summarize_source_label(source: &str) -> String {
    let s = source.trim();
    if s.starts_with("http://") || s.starts_with("https://") {
        s.chars().take(60).collect()
    } else {
        let prefix = if s.len() > 60 { &s[..60] } else { s };
        format!("Inline: {prefix}")
    }
}

fn truncate(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_string();
    }
    input.chars().take(max).collect()
}

fn make_id(prefix: &str) -> String {
    let seq = MISSION_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}_{}_{}", prefix, now_ms(), seq)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::bootstrap_schema;
    use rusqlite::Connection;

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open");
        bootstrap_schema(&mut conn).expect("bootstrap");
        conn
    }

    fn sample_draft() -> MissionDraft {
        create_mission_draft(CreateMissionDraftInput {
            template_kind: "daily_brief_multi_source".to_string(),
            intent: "Brief me on these updates".to_string(),
            provider: Some("openai".to_string()),
            sources: vec![
                "Inline note: source one status".to_string(),
                "Inline note: source two status".to_string(),
            ],
        })
        .expect("draft")
    }

    #[test]
    fn mission_start_fans_out_child_runs_with_unique_idempotency_keys() {
        std::env::set_var("TERMINUS_TRANSPORT", "mock");
        let mut conn = test_conn();
        let detail = start_mission(
            &mut conn,
            StartMissionInput {
                draft: sample_draft(),
                idempotency_key: Some("mission-idem-1".to_string()),
            },
        )
        .expect("start");
        assert_eq!(detail.child_runs.len(), 2);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mission_runs WHERE mission_id = ?1",
                params![detail.mission.id],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(count, 2);
        let unique_keys: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT idempotency_key) FROM runs WHERE autopilot_id LIKE ?1",
                params![format!("{}_%", detail.mission.id)],
                |r| r.get(0),
            )
            .expect("distinct keys");
        assert_eq!(unique_keys, 2);
    }

    #[test]
    fn mission_waits_until_children_terminal_then_aggregates() {
        std::env::set_var("TERMINUS_TRANSPORT", "mock");
        let mut conn = test_conn();
        let started = start_mission(
            &mut conn,
            StartMissionInput {
                draft: sample_draft(),
                idempotency_key: None,
            },
        )
        .expect("start");

        let first = run_mission_tick(&mut conn, &started.mission.id).expect("tick1");
        assert!(matches!(
            first.mission.mission.status,
            MissionStatus::WaitingChildren
                | MissionStatus::Running
                | MissionStatus::Aggregating
                | MissionStatus::Succeeded
        ));

        let second = run_mission_tick(&mut conn, &started.mission.id).expect("tick2");
        let final_tick = run_mission_tick(&mut conn, &started.mission.id).expect("tick3");
        let final_state = final_tick.mission.mission.status;
        assert!(matches!(
            final_state,
            MissionStatus::Succeeded | MissionStatus::WaitingChildren | MissionStatus::Aggregating
        ));
        let eventually = if matches!(final_state, MissionStatus::Succeeded) {
            final_tick
        } else if matches!(second.mission.mission.status, MissionStatus::Succeeded) {
            second
        } else {
            run_mission_tick(&mut conn, &started.mission.id).expect("tick4")
        };
        assert_eq!(eventually.mission.mission.status, MissionStatus::Succeeded);
        assert!(eventually.mission.contract.aggregation_summary_exists);
    }

    #[test]
    fn contract_blocks_when_child_is_blocked() {
        std::env::set_var("TERMINUS_TRANSPORT", "mock");
        let mut conn = test_conn();
        let started = start_mission(
            &mut conn,
            StartMissionInput {
                draft: sample_draft(),
                idempotency_key: None,
            },
        )
        .expect("start");
        let child = started.child_runs.first().expect("child");
        conn.execute(
            "UPDATE runs SET state = 'blocked', failure_reason = 'Manual test block' WHERE id = ?1",
            params![child.run_id],
        )
        .expect("force blocked");
        let tick = run_mission_tick(&mut conn, &started.mission.id).expect("tick");
        assert_eq!(tick.mission.mission.status, MissionStatus::Blocked);
        assert!(tick.mission.contract.has_blocked_or_pending_child);
    }
}
