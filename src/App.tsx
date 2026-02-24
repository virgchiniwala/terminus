import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ConnectionPanel } from "./components/ConnectionPanel";
import type {
  ApplyInterventionResult,
  EmailConnectionRecord,
  HomeSnapshot,
  InterventionSuggestion,
  IntentDraftResponse,
  MissionDetail,
  MissionDraft,
  MissionRecord,
  MissionTickResult,
  OAuthStartResponse,
  RecipeKind,
  RunDiagnosticRecord,
  RunnerControlRecord,
  AutopilotSendPolicyRecord,
  ClarificationRecord,
  ContextReceipt,
  IntentDraftKind,
  MemoryCardRecord,
} from "./types";
import {
  canStartDraftRun,
  fallbackSnapshot,
  formatShortLocalTime,
  homeLoadErrorMessage,
  normalizeEmailConnectionRecord,
  normalizeSnapshot,
  replaceDebouncedTimer,
} from "./uiLogic";

function nowId(prefix: string): string {
  return `${prefix}_${Date.now()}`;
}

function normalizeDraft(raw: unknown): IntentDraftResponse {
  const value = raw as any;
  const plan = value.plan ?? {};
  const provider = plan.provider ?? {};
  return {
    kind: value.kind,
    classificationReason: value.classificationReason ?? value.classification_reason ?? "",
    plan: {
      schemaVersion: plan.schemaVersion ?? plan.schema_version ?? "1.0",
      recipe: plan.recipe,
      intent: plan.intent ?? "",
      provider: {
        id: provider.id ?? "openai",
        tier: provider.tier ?? "supported",
        defaultModel: provider.defaultModel ?? provider.default_model ?? "gpt-4o-mini",
      },
      allowedPrimitives: plan.allowedPrimitives ?? plan.allowed_primitives ?? [],
      steps: (plan.steps ?? []).map((step: any) => ({
        id: step.id,
        label: step.label,
        primitive: step.primitive,
        requiresApproval: step.requiresApproval ?? step.requires_approval ?? false,
        riskTier: step.riskTier ?? step.risk_tier ?? "low",
      })),
      dailySources: plan.dailySources ?? plan.daily_sources ?? [],
      webSourceUrl: plan.webSourceUrl ?? plan.web_source_url ?? null,
      webAllowedDomains: plan.webAllowedDomains ?? plan.web_allowed_domains ?? [],
      inboxSourceText: plan.inboxSourceText ?? plan.inbox_source_text ?? null,
      recipientHints: plan.recipientHints ?? plan.recipient_hints ?? [],
    },
    preview: {
      reads: value.preview?.reads ?? [],
      writes: value.preview?.writes ?? [],
      approvalsRequired: value.preview?.approvalsRequired ?? value.preview?.approvals_required ?? [],
      estimatedSpend: value.preview?.estimatedSpend ?? value.preview?.estimated_spend ?? "",
      primaryCta: value.preview?.primaryCta ?? value.preview?.primary_cta ?? "Run now",
    },
  };
}

function recipeNeedsSources(recipe: RecipeKind): boolean {
  return recipe === "daily_brief";
}

function recipeNeedsPastedText(recipe: RecipeKind): boolean {
  return recipe === "inbox_triage";
}

export function App() {
  const [snapshot, setSnapshot] = useState<HomeSnapshot>(fallbackSnapshot);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [retryCount, setRetryCount] = useState(0);
  const retryCountRef = useRef(0);

  const [intentOpen, setIntentOpen] = useState(false);
  const [intentInput, setIntentInput] = useState("");
  const [intentError, setIntentError] = useState<string | null>(null);
  const [intentLoading, setIntentLoading] = useState(false);
  const [draft, setDraft] = useState<IntentDraftResponse | null>(null);
  const [runNotice, setRunNotice] = useState<string | null>(null);
  const [runDraftLoading, setRunDraftLoading] = useState(false);
  const [connections, setConnections] = useState<EmailConnectionRecord[]>([]);
  const [connectionsMessage, setConnectionsMessage] = useState<string | null>(null);
  const [oauthProvider, setOauthProvider] = useState<"gmail" | "microsoft365">("gmail");
  const [oauthClientId, setOauthClientId] = useState("");
  const [oauthRedirectUri, setOauthRedirectUri] = useState("");
  const [oauthSession, setOauthSession] = useState<OAuthStartResponse | null>(null);
  const [oauthCode, setOauthCode] = useState("");
  const [watcherAutopilotId, setWatcherAutopilotId] = useState("auto_inbox_watch");
  const [watcherMaxItems, setWatcherMaxItems] = useState(10);
  const [runnerControl, setRunnerControl] = useState<RunnerControlRecord | null>(null);
  const [sendPolicyAutopilotId, setSendPolicyAutopilotId] = useState("auto_inbox_watch_gmail");
  const [sendPolicyAllowlistInput, setSendPolicyAllowlistInput] = useState("");
  const [sendPolicy, setSendPolicy] = useState<AutopilotSendPolicyRecord | null>(null);
  const [guideScopeType, setGuideScopeType] = useState<"autopilot" | "run" | "approval" | "outcome">("autopilot");
  const [guideScopeId, setGuideScopeId] = useState("");
  const [guideInstruction, setGuideInstruction] = useState("");
  const [guideMessage, setGuideMessage] = useState<string | null>(null);
  const [clarifications, setClarifications] = useState<ClarificationRecord[]>([]);
  const [clarificationAnswers, setClarificationAnswers] = useState<Record<string, string>>({});
  const [clarificationsMessage, setClarificationsMessage] = useState<string | null>(null);
  const [runDiagnostics, setRunDiagnostics] = useState<RunDiagnosticRecord[]>([]);
  const [diagnosticsMessage, setDiagnosticsMessage] = useState<string | null>(null);
  const [missions, setMissions] = useState<MissionRecord[]>([]);
  const [selectedMissionId, setSelectedMissionId] = useState<string | null>(null);
  const [selectedMission, setSelectedMission] = useState<MissionDetail | null>(null);
  const [missionsMessage, setMissionsMessage] = useState<string | null>(null);
  const [missionActionLoading, setMissionActionLoading] = useState(false);
  const [selectedMissionChildRunId, setSelectedMissionChildRunId] = useState<string | null>(null);
  const [selectedMissionContextReceipt, setSelectedMissionContextReceipt] = useState<ContextReceipt | null>(null);
  const [selectedMissionMemoryCards, setSelectedMissionMemoryCards] = useState<MemoryCardRecord[]>([]);
  const [contextReceiptLoading, setContextReceiptLoading] = useState(false);
  const [contextReceiptMessage, setContextReceiptMessage] = useState<string | null>(null);
  const runnerControlSaveTimerRef = useRef<number | null>(null);
  const sendPolicySaveTimerRef = useRef<number | null>(null);
  const intentOverlayRef = useRef<HTMLDivElement | null>(null);
  const intentTextareaRef = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    retryCountRef.current = retryCount;
  }, [retryCount]);

  const loadSnapshot = useCallback(() => {
    setLoading(true);
    invoke<HomeSnapshot>("get_home_snapshot")
      .then((data) => {
        setSnapshot(normalizeSnapshot(data));
        setError(null);
        setRetryCount(0);
      })
      .catch((err) => {
        console.error("Failed to load home snapshot:", err);
        setError(homeLoadErrorMessage(retryCountRef.current));
        setSnapshot(fallbackSnapshot);
        setRetryCount((c) => c + 1);
      })
      .finally(() => {
        setLoading(false);
      });
  }, []);

  const normalizeClarification = useCallback((row: any): ClarificationRecord => ({
    id: row.id,
    runId: row.runId ?? row.run_id,
    stepId: row.stepId ?? row.step_id,
    fieldKey: row.fieldKey ?? row.field_key,
    question: row.question,
    optionsJson: row.optionsJson ?? row.options_json ?? null,
    answerJson: row.answerJson ?? row.answer_json ?? null,
    status: row.status,
  }), []);

  const normalizeRunDiagnostic = useCallback((row: any): RunDiagnosticRecord => ({
    id: row.id,
    runId: row.runId ?? row.run_id,
    autopilotId: row.autopilotId ?? row.autopilot_id,
    runState: row.runState ?? row.run_state,
    healthStatus: row.healthStatus ?? row.health_status,
    reasonCode: row.reasonCode ?? row.reason_code,
    summary: row.summary ?? "",
    suggestions: (row.suggestions ?? []).map((s: any): InterventionSuggestion => ({
      kind: s.kind,
      label: s.label,
      reason: s.reason ?? "",
      disabled: Boolean(s.disabled),
    })),
    createdAtMs: row.createdAtMs ?? row.created_at_ms ?? Date.now(),
  }), []);

  const loadClarifications = useCallback(() => {
    invoke<ClarificationRecord[]>("list_pending_clarifications")
      .then((rows: any[]) => {
        const normalized = (rows ?? []).map(normalizeClarification);
        setClarifications(normalized);
        setClarificationAnswers((prev) => {
          const next = { ...prev };
          for (const item of normalized) {
            if (next[item.id] == null) {
              next[item.id] = "";
            }
          }
          return next;
        });
      })
      .catch((err) => {
        console.error("Failed to load clarifications:", err);
      });
  }, [normalizeClarification]);

  const loadRunDiagnostics = useCallback(() => {
    invoke<RunDiagnosticRecord[]>("list_run_diagnostics", { limit: 12 })
      .then((rows: any[]) => {
        setRunDiagnostics((rows ?? []).map(normalizeRunDiagnostic));
      })
      .catch((err) => {
        console.error("Failed to load run diagnostics:", err);
        setDiagnosticsMessage("Could not load run diagnostics.");
      });
  }, [normalizeRunDiagnostic]);

  const normalizeMissionRecord = useCallback((row: any): MissionRecord => ({
    id: row.id,
    templateKind: row.templateKind ?? row.template_kind,
    status: row.status,
    provider: row.provider,
    failureReason: row.failureReason ?? row.failure_reason ?? null,
    childRunsCount: row.childRunsCount ?? row.child_runs_count ?? 0,
    terminalChildrenCount: row.terminalChildrenCount ?? row.terminal_children_count ?? 0,
    summaryJson: row.summaryJson ?? row.summary_json ?? null,
    createdAtMs: row.createdAtMs ?? row.created_at_ms ?? Date.now(),
    updatedAtMs: row.updatedAtMs ?? row.updated_at_ms ?? Date.now(),
  }), []);

  const normalizeMissionDetail = useCallback((row: any): MissionDetail => ({
    mission: normalizeMissionRecord(row.mission ?? {}),
    childRuns: (row.childRuns ?? row.child_runs ?? []).map((c: any) => ({
      childKey: c.childKey ?? c.child_key,
      sourceLabel: c.sourceLabel ?? c.source_label ?? null,
      runId: c.runId ?? c.run_id,
      runRole: c.runRole ?? c.run_role ?? "child",
      status: c.status,
      runState: c.runState ?? c.run_state ?? null,
      runFailureReason: c.runFailureReason ?? c.run_failure_reason ?? null,
      updatedAtMs: c.updatedAtMs ?? c.updated_at_ms ?? Date.now(),
    })),
    events: (row.events ?? []).map((e: any) => ({
      id: e.id,
      eventType: e.eventType ?? e.event_type,
      summary: e.summary ?? "",
      detailsJson: e.detailsJson ?? e.details_json ?? "{}",
      createdAtMs: e.createdAtMs ?? e.created_at_ms ?? Date.now(),
    })),
    contract: {
      allChildrenTerminal:
        row.contract?.allChildrenTerminal ?? row.contract?.all_children_terminal ?? false,
      hasBlockedOrPendingChild:
        row.contract?.hasBlockedOrPendingChild ??
        row.contract?.has_blocked_or_pending_child ??
        false,
      aggregationSummaryExists:
        row.contract?.aggregationSummaryExists ?? row.contract?.aggregation_summary_exists ?? false,
      readyToComplete: row.contract?.readyToComplete ?? row.contract?.ready_to_complete ?? false,
    },
  }), [normalizeMissionRecord]);

  const normalizeMemoryCard = useCallback((row: any): MemoryCardRecord => ({
    cardId: row.cardId ?? row.card_id,
    autopilotId: row.autopilotId ?? row.autopilot_id,
    cardType: row.cardType ?? row.card_type ?? "unknown",
    title: row.title ?? "",
    confidence: row.confidence ?? 0,
    createdFromRunId: row.createdFromRunId ?? row.created_from_run_id ?? null,
    updatedAtMs: row.updatedAtMs ?? row.updated_at_ms ?? Date.now(),
    version: row.version ?? 1,
    suppressed: Boolean(row.suppressed ?? false),
  }), []);

  const normalizeContextReceipt = useCallback((raw: any): ContextReceipt => ({
    runId: raw.runId ?? raw.run_id,
    autopilotId: raw.autopilotId ?? raw.autopilot_id,
    recipe: raw.recipe ?? "unknown",
    providerKind: raw.providerKind ?? raw.provider_kind ?? "unknown",
    providerTier: raw.providerTier ?? raw.provider_tier ?? "unknown",
    runState: raw.runState ?? raw.run_state ?? "unknown",
    terminalReceiptFound: raw.terminalReceiptFound ?? raw.terminal_receipt_found ?? false,
    sources: (raw.sources ?? []).map((s: any) => ({
      sourceKind: s.sourceKind ?? s.source_kind ?? "unknown",
      sourceId: s.sourceId ?? s.source_id ?? null,
      url: s.url ?? null,
      status: s.status ?? "unknown",
      fetchedAtMs: s.fetchedAtMs ?? s.fetched_at_ms ?? null,
      contentHash: s.contentHash ?? s.content_hash ?? null,
      excerptChars: s.excerptChars ?? s.excerpt_chars ?? null,
      changed: s.changed ?? null,
      diffScore: s.diffScore ?? s.diff_score ?? null,
      error: s.error ?? null,
    })),
    memoryTitlesUsed: raw.memoryTitlesUsed ?? raw.memory_titles_used ?? [],
    memoryCardsUsed: (raw.memoryCardsUsed ?? raw.memory_cards_used ?? []).map(normalizeMemoryCard),
    policyConstraints: {
      denyByDefaultPrimitives:
        raw.policyConstraints?.denyByDefaultPrimitives ??
        raw.policy_constraints?.deny_by_default_primitives ??
        true,
      allowedPrimitives:
        raw.policyConstraints?.allowedPrimitives ??
        raw.policy_constraints?.allowed_primitives ??
        [],
      webAllowedDomains:
        raw.policyConstraints?.webAllowedDomains ??
        raw.policy_constraints?.web_allowed_domains ??
        [],
      approvalRequiredSteps:
        raw.policyConstraints?.approvalRequiredSteps ??
        raw.policy_constraints?.approval_required_steps ??
        [],
      sendPolicy: {
        allowSending:
          raw.policyConstraints?.sendPolicy?.allowSending ??
          raw.policy_constraints?.send_policy?.allow_sending ??
          false,
        recipientAllowlistCount:
          raw.policyConstraints?.sendPolicy?.recipientAllowlistCount ??
          raw.policy_constraints?.send_policy?.recipient_allowlist_count ??
          0,
        maxSendsPerDay:
          raw.policyConstraints?.sendPolicy?.maxSendsPerDay ??
          raw.policy_constraints?.send_policy?.max_sends_per_day ??
          0,
        quietHoursStartLocal:
          raw.policyConstraints?.sendPolicy?.quietHoursStartLocal ??
          raw.policy_constraints?.send_policy?.quiet_hours_start_local ??
          18,
        quietHoursEndLocal:
          raw.policyConstraints?.sendPolicy?.quietHoursEndLocal ??
          raw.policy_constraints?.send_policy?.quiet_hours_end_local ??
          9,
        allowOutsideQuietHours:
          raw.policyConstraints?.sendPolicy?.allowOutsideQuietHours ??
          raw.policy_constraints?.send_policy?.allow_outside_quiet_hours ??
          false,
      },
    },
    runtimeProfileOverlay: {
      learningEnabled:
        raw.runtimeProfileOverlay?.learningEnabled ??
        raw.runtime_profile_overlay?.learning_enabled ??
        true,
      mode: raw.runtimeProfileOverlay?.mode ?? raw.runtime_profile_overlay?.mode ?? "balanced",
      suppressUntilMs:
        raw.runtimeProfileOverlay?.suppressUntilMs ??
        raw.runtime_profile_overlay?.suppress_until_ms ??
        null,
      minDiffScoreToNotify:
        raw.runtimeProfileOverlay?.minDiffScoreToNotify ??
        raw.runtime_profile_overlay?.min_diff_score_to_notify ??
        0.2,
      maxSources:
        raw.runtimeProfileOverlay?.maxSources ?? raw.runtime_profile_overlay?.max_sources ?? 5,
      maxBullets:
        raw.runtimeProfileOverlay?.maxBullets ?? raw.runtime_profile_overlay?.max_bullets ?? 6,
      replyLengthHint:
        raw.runtimeProfileOverlay?.replyLengthHint ??
        raw.runtime_profile_overlay?.reply_length_hint ??
        "medium",
    },
    redactionFlags: raw.redactionFlags ?? raw.redaction_flags ?? [],
    rationaleCodes: raw.rationaleCodes ?? raw.rationale_codes ?? [],
    keySignals: raw.keySignals ?? raw.key_signals ?? [],
    providerCalls: (raw.providerCalls ?? raw.provider_calls ?? []).map((p: any) => ({
      provider: p.provider ?? "unknown",
      model: p.model ?? "unknown",
      requestKind: p.requestKind ?? p.request_kind ?? "unknown",
      inputChars: p.inputChars ?? p.input_chars ?? null,
      outputChars: p.outputChars ?? p.output_chars ?? null,
      latencyMs: p.latencyMs ?? p.latency_ms ?? null,
      costCentsEst: p.costCentsEst ?? p.cost_cents_est ?? null,
      createdAtMs: p.createdAtMs ?? p.created_at_ms ?? Date.now(),
    })),
  }), [normalizeMemoryCard]);

  const loadMissions = useCallback(() => {
    invoke<MissionRecord[]>("list_missions", { limit: 10 })
      .then((rows: any[]) => {
        const normalized = (rows ?? []).map(normalizeMissionRecord);
        setMissions(normalized);
        setSelectedMissionId((prev) => prev ?? normalized[0]?.id ?? null);
      })
      .catch((err) => {
        console.error("Failed to load missions:", err);
        setMissionsMessage("Could not load missions.");
      });
  }, [normalizeMissionRecord]);

  const loadMissionDetail = useCallback((missionId: string) => {
    invoke<MissionDetail>("get_mission", { missionId })
      .then((payload: any) => setSelectedMission(normalizeMissionDetail(payload)))
      .catch((err) => {
        console.error("Failed to load mission detail:", err);
        setMissionsMessage(typeof err === "string" ? err : "Could not load mission details.");
      });
  }, [normalizeMissionDetail]);

  const loadContextReceiptForRun = useCallback((runId: string) => {
    setContextReceiptLoading(true);
    setContextReceiptMessage(null);
    setSelectedMissionChildRunId(runId);
    invoke<ContextReceipt>("get_context_receipt", { runId })
      .then((receipt: any) => {
        const normalizedReceipt = normalizeContextReceipt(receipt);
        setSelectedMissionContextReceipt(normalizedReceipt);
        return invoke<MemoryCardRecord[]>("list_memory_cards_for_autopilot", {
          autopilotId: normalizedReceipt.autopilotId,
        });
      })
      .then((cards: any[]) => {
        setSelectedMissionMemoryCards((cards ?? []).map(normalizeMemoryCard));
      })
      .catch((err) => {
        console.error("Failed to load context receipt:", err);
        setContextReceiptMessage(typeof err === "string" ? err : "Could not load context receipt.");
      })
      .finally(() => setContextReceiptLoading(false));
  }, [normalizeContextReceipt, normalizeMemoryCard]);

  const toggleMemoryCardSuppression = useCallback((autopilotId: string, cardId: string, nextSuppressed: boolean) => {
    const command = nextSuppressed ? "suppress_memory_card" : "unsuppress_memory_card";
    invoke<MemoryCardRecord>(command, { autopilotId, cardId })
      .then((updated: any) => {
        const normalized = normalizeMemoryCard(updated);
        setSelectedMissionMemoryCards((prev) =>
          prev.map((item) => (item.cardId === normalized.cardId ? normalized : item)),
        );
        setSelectedMissionContextReceipt((prev) =>
          prev
            ? {
                ...prev,
                memoryCardsUsed: prev.memoryCardsUsed.map((item) =>
                  item.cardId === normalized.cardId ? normalized : item,
                ),
              }
            : prev,
        );
      })
      .catch((err) => {
        console.error("Failed to update memory card suppression:", err);
        setContextReceiptMessage(typeof err === "string" ? err : "Could not update memory card.");
      });
  }, [normalizeMemoryCard]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && intentOpen) {
        event.preventDefault();
        setIntentOpen(false);
        return;
      }
      const cmdK = (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k";
      if (!cmdK) {
        return;
      }
      event.preventDefault();
      setIntentOpen(true);
      setIntentError(null);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [intentOpen]);

  useEffect(() => {
    if (!intentOpen) {
      return;
    }
    const timer = window.setTimeout(() => intentTextareaRef.current?.focus(), 0);
    return () => window.clearTimeout(timer);
  }, [intentOpen]);

  const classifiedLabel = useMemo(() => {
    if (!draft) {
      return "";
    }
    return draft.kind === "draft_autopilot" ? "Recurring Autopilot" : "One-time Run";
  }, [draft]);

  const loadConnections = useCallback(() => {
    invoke<EmailConnectionRecord[]>("list_email_connections")
      .then((rows) => {
        const normalized = rows.map(normalizeEmailConnectionRecord);
        setConnections(normalized);
      })
      .catch((err) => {
        console.error("Failed to load connections:", err);
        setConnectionsMessage("Could not load provider connections.");
      });
  }, []);

  const loadRunnerControl = useCallback(() => {
    invoke<RunnerControlRecord>("get_runner_control")
      .then((payload: any) => {
        setRunnerControl({
          backgroundEnabled: payload.backgroundEnabled ?? payload.background_enabled ?? false,
          watcherEnabled: payload.watcherEnabled ?? payload.watcher_enabled ?? true,
          watcherPollSeconds: payload.watcherPollSeconds ?? payload.watcher_poll_seconds ?? 60,
          watcherMaxItems: payload.watcherMaxItems ?? payload.watcher_max_items ?? 10,
          gmailAutopilotId:
            payload.gmailAutopilotId ?? payload.gmail_autopilot_id ?? "auto_inbox_watch_gmail",
          microsoftAutopilotId:
            payload.microsoftAutopilotId ??
            payload.microsoft_autopilot_id ??
            "auto_inbox_watch_microsoft365",
          watcherLastTickMs: payload.watcherLastTickMs ?? payload.watcher_last_tick_ms ?? null,
          missedRunsCount: payload.missedRunsCount ?? payload.missed_runs_count ?? 0,
        });
      })
      .catch((err) => {
        console.error("Failed to load runner control:", err);
      });
  }, []);

  const persistRunnerControl = useCallback((next: RunnerControlRecord) => {
    invoke<RunnerControlRecord>("update_runner_control", {
      input: {
        backgroundEnabled: next.backgroundEnabled,
        watcherEnabled: next.watcherEnabled,
        watcherPollSeconds: next.watcherPollSeconds,
        watcherMaxItems: next.watcherMaxItems,
        gmailAutopilotId: next.gmailAutopilotId,
        microsoftAutopilotId: next.microsoftAutopilotId,
      },
    })
      .then(() => {
        setConnectionsMessage("Runner controls updated.");
        loadSnapshot();
      })
      .catch((err) => {
        console.error("Failed to update runner control:", err);
        setConnectionsMessage(typeof err === "string" ? err : "Could not update runner controls.");
      });
  }, [loadSnapshot]);

  const saveRunnerControl = useCallback((next: RunnerControlRecord) => {
    setRunnerControl(next);
    runnerControlSaveTimerRef.current = replaceDebouncedTimer(window, runnerControlSaveTimerRef.current, () => {
      runnerControlSaveTimerRef.current = null;
      persistRunnerControl(next);
    }, 300);
  }, [persistRunnerControl]);

  const loadSendPolicy = () => {
    invoke<AutopilotSendPolicyRecord>("get_autopilot_send_policy", {
      autopilotId: sendPolicyAutopilotId,
    })
      .then((payload: any) => {
        const normalized: AutopilotSendPolicyRecord = {
          autopilotId: payload.autopilotId ?? payload.autopilot_id ?? sendPolicyAutopilotId,
          allowSending: payload.allowSending ?? payload.allow_sending ?? false,
          recipientAllowlist: payload.recipientAllowlist ?? payload.recipient_allowlist ?? [],
          maxSendsPerDay: payload.maxSendsPerDay ?? payload.max_sends_per_day ?? 10,
          quietHoursStartLocal:
            payload.quietHoursStartLocal ?? payload.quiet_hours_start_local ?? 18,
          quietHoursEndLocal: payload.quietHoursEndLocal ?? payload.quiet_hours_end_local ?? 9,
          allowOutsideQuietHours:
            payload.allowOutsideQuietHours ?? payload.allow_outside_quiet_hours ?? false,
          updatedAtMs: payload.updatedAtMs ?? payload.updated_at_ms ?? 0,
        };
        setSendPolicy(normalized);
        setSendPolicyAllowlistInput(normalized.recipientAllowlist.join(", "));
      })
      .catch((err) => {
        console.error("Failed to load send policy:", err);
        setConnectionsMessage("Could not load send policy for this Autopilot.");
      });
  };

  const persistSendPolicy = useCallback((next: AutopilotSendPolicyRecord) => {
    invoke<AutopilotSendPolicyRecord>("update_autopilot_send_policy", {
      input: {
        autopilotId: next.autopilotId,
        allowSending: next.allowSending,
        recipientAllowlist: next.recipientAllowlist,
        maxSendsPerDay: next.maxSendsPerDay,
        quietHoursStartLocal: next.quietHoursStartLocal,
        quietHoursEndLocal: next.quietHoursEndLocal,
        allowOutsideQuietHours: next.allowOutsideQuietHours,
      },
    })
      .then((payload: any) => {
        setSendPolicy({
          autopilotId: payload.autopilotId ?? payload.autopilot_id ?? next.autopilotId,
          allowSending: payload.allowSending ?? payload.allow_sending ?? next.allowSending,
          recipientAllowlist:
            payload.recipientAllowlist ??
            payload.recipient_allowlist ??
            next.recipientAllowlist,
          maxSendsPerDay:
            payload.maxSendsPerDay ?? payload.max_sends_per_day ?? next.maxSendsPerDay,
          quietHoursStartLocal:
            payload.quietHoursStartLocal ??
            payload.quiet_hours_start_local ??
            next.quietHoursStartLocal,
          quietHoursEndLocal:
            payload.quietHoursEndLocal ??
            payload.quiet_hours_end_local ??
            next.quietHoursEndLocal,
          allowOutsideQuietHours:
            payload.allowOutsideQuietHours ??
            payload.allow_outside_quiet_hours ??
            next.allowOutsideQuietHours,
          updatedAtMs: payload.updatedAtMs ?? payload.updated_at_ms ?? Date.now(),
        });
        setConnectionsMessage("Send policy updated.");
      })
      .catch((err) => {
        console.error("Failed to save send policy:", err);
        setConnectionsMessage(typeof err === "string" ? err : "Could not update send policy.");
      });
  }, []);

  const saveSendPolicy = useCallback((next: AutopilotSendPolicyRecord) => {
    setSendPolicy(next);
    sendPolicySaveTimerRef.current = replaceDebouncedTimer(window, sendPolicySaveTimerRef.current, () => {
      sendPolicySaveTimerRef.current = null;
      persistSendPolicy(next);
    }, 300);
  }, [persistSendPolicy]);

  useEffect(() => {
    loadSnapshot();
    loadConnections();
    loadRunnerControl();
    loadClarifications();
    loadRunDiagnostics();
    loadMissions();
  }, [loadSnapshot, loadConnections, loadRunnerControl, loadClarifications, loadRunDiagnostics, loadMissions]);

  useEffect(() => {
    if (!selectedMissionId) {
      setSelectedMission(null);
      setSelectedMissionChildRunId(null);
      setSelectedMissionContextReceipt(null);
      setSelectedMissionMemoryCards([]);
      return;
    }
    loadMissionDetail(selectedMissionId);
  }, [selectedMissionId, loadMissionDetail]);

  useEffect(() => {
    const interval = window.setInterval(() => {
      invoke("tick_runner_cycle")
        .then(() => {
          loadSnapshot();
          loadClarifications();
          loadRunDiagnostics();
          loadMissions();
          if (selectedMissionId) {
            loadMissionDetail(selectedMissionId);
          }
        })
        .catch(() => {
          // keep silent; runner status remains visible on Home
        });
    }, 10_000);
    return () => window.clearInterval(interval);
  }, [loadSnapshot, loadClarifications, loadRunDiagnostics, loadMissions, loadMissionDetail, selectedMissionId]);

  useEffect(() => {
    return () => {
      if (runnerControlSaveTimerRef.current != null) {
        window.clearTimeout(runnerControlSaveTimerRef.current);
      }
      if (sendPolicySaveTimerRef.current != null) {
        window.clearTimeout(sendPolicySaveTimerRef.current);
      }
    };
  }, []);

  const generateDraft = (forcedKind?: IntentDraftKind) => {
    const intent = intentInput.trim();
    if (!intent) {
      setIntentError("Add a one-line intent to continue.");
      return;
    }
    setIntentLoading(true);
    setIntentError(null);
    setRunNotice(null);
    invoke<IntentDraftResponse>("draft_intent", { intent, forcedKind })
      .then((payload) => {
        setDraft(normalizeDraft(payload));
      })
      .catch((err) => {
        console.error("Failed to draft intent:", err);
        setIntentError(typeof err === "string" ? err : "Could not prepare this setup yet.");
      })
      .finally(() => {
        setIntentLoading(false);
      });
  };

  const runDraft = () => {
    const currentDraft = draft;
    if (!currentDraft || !canStartDraftRun(currentDraft, runDraftLoading)) {
      return;
    }
    const autopilotId = nowId(currentDraft.kind === "draft_autopilot" ? "autopilot" : "run");
    const idempotencyKey = nowId("idem");
    const dailySources = recipeNeedsSources(currentDraft.plan.recipe) ? currentDraft.plan.dailySources : undefined;
    const pastedText = recipeNeedsPastedText(currentDraft.plan.recipe)
      ? currentDraft.plan.inboxSourceText
      : undefined;

    setRunDraftLoading(true);
    invoke("start_recipe_run", {
      autopilotId,
      recipe: currentDraft.plan.recipe,
      intent: currentDraft.plan.intent,
      pastedText,
      dailySources,
      provider: currentDraft.plan.provider.id,
      idempotencyKey,
      maxRetries: 2,
    })
      .then(() => {
        setRunNotice(`${currentDraft.preview.primaryCta} started. Open Activity for live progress.`);
        setIntentOpen(false);
        setIntentInput("");
        setDraft(null);
        loadSnapshot();
      })
      .catch((err) => {
        console.error("Failed to start run:", err);
        setIntentError(typeof err === "string" ? err : "Could not start this run.");
      })
      .finally(() => {
        setRunDraftLoading(false);
      });
  };

  const handleIntentOverlayKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Tab") {
      return;
    }
    const root = intentOverlayRef.current;
    if (!root) {
      return;
    }
    const focusables = Array.from(
      root.querySelectorAll<HTMLElement>(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
      )
    ).filter((node) => !node.hasAttribute("disabled"));
    if (focusables.length === 0) {
      return;
    }
    const first = focusables[0];
    const last = focusables[focusables.length - 1];
    const active = document.activeElement as HTMLElement | null;
    if (event.shiftKey && active === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  };

  const saveOauthSetup = () => {
    setConnectionsMessage(null);
    invoke("save_email_oauth_config", {
      input: {
        provider: oauthProvider,
        clientId: oauthClientId,
        redirectUri: oauthRedirectUri,
      },
    })
      .then(() => {
        setConnectionsMessage("Connection setup saved.");
      })
      .catch((err) => {
        console.error("Failed to save oauth config:", err);
        setConnectionsMessage(typeof err === "string" ? err : "Could not save setup.");
      });
  };

  const startOauth = (provider: "gmail" | "microsoft365") => {
    setConnectionsMessage(null);
    invoke<OAuthStartResponse>("start_email_oauth", { provider })
      .then((payload) => {
        setOauthSession({
          provider: payload.provider,
          authUrl: payload.authUrl ?? (payload as any).auth_url,
          state: payload.state,
          expiresAtMs: payload.expiresAtMs ?? (payload as any).expires_at_ms,
        });
        setOauthCode("");
      })
      .catch((err) => {
        console.error("Failed to start oauth:", err);
        setConnectionsMessage(typeof err === "string" ? err : "Could not start connection.");
      });
  };

  const completeOauth = () => {
    if (!oauthSession) {
      return;
    }
    setConnectionsMessage(null);
    invoke<EmailConnectionRecord>("complete_email_oauth", {
      input: {
        provider: oauthSession.provider,
        state: oauthSession.state,
        code: oauthCode,
      },
    })
      .then(() => {
        setConnectionsMessage("Email provider connected.");
        setOauthSession(null);
        setOauthCode("");
        loadConnections();
      })
      .catch((err) => {
        console.error("Failed to complete oauth:", err);
        setConnectionsMessage(typeof err === "string" ? err : "Could not complete connection.");
      });
  };

  const disconnectProvider = (provider: "gmail" | "microsoft365") => {
    setConnectionsMessage(null);
    invoke("disconnect_email_provider", { provider })
      .then(() => {
        setConnectionsMessage("Provider disconnected.");
        loadConnections();
      })
      .catch((err) => {
        console.error("Failed to disconnect provider:", err);
        setConnectionsMessage(typeof err === "string" ? err : "Could not disconnect provider.");
      });
  };

  const runWatcherTick = (provider: "gmail" | "microsoft365") => {
    setConnectionsMessage(null);
    invoke<{ fetched: number; deduped: number; startedRuns: number; started_runs?: number }>(
      "run_inbox_watcher_tick",
      {
        provider,
        autopilotId: watcherAutopilotId,
        maxItems: watcherMaxItems,
      }
    )
      .then((summary: any) => {
        const started = summary.startedRuns ?? summary.started_runs ?? 0;
        setConnectionsMessage(
          `Watcher tick complete: fetched ${summary.fetched}, deduped ${summary.deduped}, queued ${started}.`
        );
        loadSnapshot();
        loadRunnerControl();
      })
      .catch((err) => {
        console.error("Watcher tick failed:", err);
        setConnectionsMessage(typeof err === "string" ? err : "Watcher tick failed.");
      });
  };

  const submitGuide = () => {
    setGuideMessage(null);
    invoke<{ mode: string; message: string; proposedRule?: string | null }>("submit_guidance", {
      input: {
        scopeType: guideScopeType,
        scopeId: guideScopeId,
        instruction: guideInstruction,
      },
    })
      .then((payload: any) => {
        const msg = payload?.proposedRule
          ? `${payload.message} Proposed rule: ${payload.proposedRule}`
          : payload?.message ?? "Guidance saved.";
        setGuideMessage(msg);
      })
      .catch((err) => {
        console.error("Failed to submit guidance:", err);
        setGuideMessage(typeof err === "string" ? err : "Could not save guidance.");
      });
  };

  const clarificationOptions = (item: ClarificationRecord): string[] => {
    if (!item.optionsJson) {
      return [];
    }
    try {
      const parsed = JSON.parse(item.optionsJson);
      if (Array.isArray(parsed)) {
        return parsed.filter((v): v is string => typeof v === "string").slice(0, 6);
      }
      if (parsed && Array.isArray(parsed.options)) {
        return parsed.options.filter((v: unknown): v is string => typeof v === "string").slice(0, 6);
      }
    } catch {
      // ignore malformed options payload
    }
    return [];
  };

  const submitClarification = (item: ClarificationRecord) => {
    const raw = (clarificationAnswers[item.id] ?? "").trim();
    if (!raw) {
      setClarificationsMessage("Add one answer so Terminus can continue.");
      return;
    }
    const answerPayload = JSON.stringify({ value: raw, fieldKey: item.fieldKey });
    setClarificationsMessage(null);
    invoke("submit_clarification_answer", {
      clarificationId: item.id,
      answerJson: answerPayload,
    })
      .then(() => {
        setClarificationsMessage("Answer saved. Terminus resumed the run.");
        loadClarifications();
        loadSnapshot();
        loadRunDiagnostics();
      })
      .catch((err) => {
        console.error("Failed to submit clarification answer:", err);
        setClarificationsMessage(typeof err === "string" ? err : "Could not submit answer.");
      });
  };

  const applyIntervention = (runId: string, kind: string) => {
    setDiagnosticsMessage(null);
    invoke<ApplyInterventionResult>("apply_intervention", { input: { runId, kind } })
      .then((result) => {
        setDiagnosticsMessage(result.message);
        loadRunDiagnostics();
        loadClarifications();
        loadSnapshot();
      })
      .catch((err) => {
        console.error("Failed to apply intervention:", err);
        setDiagnosticsMessage(typeof err === "string" ? err : "Could not apply intervention.");
      });
  };

  const createDemoMission = () => {
    if (missionActionLoading) return;
    setMissionActionLoading(true);
    setMissionsMessage(null);
    invoke<MissionDraft>("create_mission_draft", {
      input: {
        templateKind: "daily_brief_multi_source",
        intent: "Create a mission brief from these updates",
        provider: "openai",
        sources: [
          "Inline note: Product - onboarding milestone moved to next week.",
          "Inline note: Ops - billing ticket volume increased today.",
          "Inline note: GTM - customer requested security follow-up.",
        ],
      },
    })
      .then((draft) => invoke<MissionDetail>("start_mission", { input: { draft } }))
      .then((payload: any) => {
        const detail = normalizeMissionDetail(payload);
        setSelectedMissionId(detail.mission.id);
        setSelectedMission(detail);
        setMissionsMessage("Mission created. Run a mission tick to advance child runs.");
        loadMissions();
      })
      .catch((err) => {
        console.error("Failed to create mission:", err);
        setMissionsMessage(typeof err === "string" ? err : "Could not create mission.");
      })
      .finally(() => setMissionActionLoading(false));
  };

  const tickMission = (missionId: string) => {
    if (missionActionLoading) return;
    setMissionActionLoading(true);
    setMissionsMessage(null);
    invoke<MissionTickResult>("run_mission_tick", { missionId })
      .then((payload: any) => {
        const detail = normalizeMissionDetail(payload.mission ?? payload);
        const advanced = payload.childRunsTicked ?? payload.child_runs_ticked ?? 0;
        setSelectedMission(detail);
        setSelectedMissionId(detail.mission.id);
        setMissionsMessage(`Mission tick complete (${advanced} child runs advanced).`);
        loadMissions();
      })
      .catch((err) => {
        console.error("Failed to tick mission:", err);
        setMissionsMessage(typeof err === "string" ? err : "Could not run mission tick.");
      })
      .finally(() => setMissionActionLoading(false));
  };

  if (loading) {
    return (
      <main className="app-shell loading-state" aria-label="Loading Terminus" aria-busy="true">
        <div className="loading-spinner" role="status">
          <span className="sr-only">Loading...</span>
        </div>
      </main>
    );
  }

  return (
    <>
      <a href="#main-content" className="skip-to-main">
        Skip to main content
      </a>
      <main id="main-content" className="app-shell" aria-label="Terminus Home">
        {error && (
          <aside className="error-banner" role="alert" aria-live="polite">
            <div className="error-content">
              <span className="error-icon">⚠️</span>
              <p>{error}</p>
            </div>
            <button type="button" className="retry-button" onClick={loadSnapshot} aria-label="Retry loading data">
              Retry
            </button>
          </aside>
        )}

        {runNotice && (
          <aside className="run-notice" role="status">
            <p>{runNotice}</p>
          </aside>
        )}

        <header className="hero">
          <p className="kicker">Terminus</p>
          <h1>Personal AI OS</h1>
          <p className="subhead">Autopilots, outcomes, approvals, and activity in one calm view.</p>
          <button type="button" className="intent-open-button" onClick={() => setIntentOpen(true)}>
            Open Intent Bar (⌘K)
          </button>
        </header>

        <section className="surface-grid" aria-label="Home surfaces" role="region">
          {snapshot.surfaces.map((surface) => (
            <article
              key={surface.title}
              className={`surface-card ${surface.count === 0 ? "empty" : ""}`}
              aria-labelledby={`${surface.title.toLowerCase()}-title`}
            >
              <div>
                <h2 id={`${surface.title.toLowerCase()}-title`}>{surface.title}</h2>
                <p className="surface-subtitle">{surface.subtitle}</p>
              </div>
              <div className="surface-footer">
                <span className="count-badge" aria-label={`${surface.count} ${surface.count === 1 ? "item" : "items"}`}>
                  {surface.count === 0 ? "Empty" : `${surface.count} ${surface.count === 1 ? "item" : "items"}`}
                </span>
                <button type="button" className="cta-button" aria-label={`${surface.cta} for ${surface.title}`}>
                  {surface.cta}
                </button>
              </div>
            </article>
          ))}
        </section>

        <section className="runner-banner" aria-label="Runner status">
          <strong>Runner mode:</strong> {snapshot.runner.mode === "background" ? "Background" : "App Open"}
          <p>{snapshot.runner.statusLine}</p>
          <p>Pending runs: {snapshot.runner.backlogCount ?? 0}</p>
          <p>Missed while asleep/offline: {snapshot.runner.missedRunsCount ?? 0}</p>
          {(snapshot.runner.suppressedAutopilots?.length ?? 0) > 0 && (
            <div className="runner-suppressed-list">
              <p><strong>Suppressed Autopilots</strong></p>
              <ul>
                {snapshot.runner.suppressedAutopilots?.map((item) => (
                  <li key={`${item.autopilotId}_${item.suppressUntilMs}`}>
                    {item.name} (<code>{item.autopilotId}</code>) until {formatShortLocalTime(item.suppressUntilMs)}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </section>

        <section className="diagnostics-panel" aria-label="Missions">
          <div className="connection-panel-header">
            <h2>Missions (MVP)</h2>
            <p>Mission orchestration fans out child runs, then completes an aggregate summary when the contract passes.</p>
          </div>
          <div className="connection-actions">
            <button type="button" onClick={createDemoMission} disabled={missionActionLoading}>
              {missionActionLoading ? "Working..." : "Create Demo Mission"}
            </button>
            {selectedMissionId && (
              <button
                type="button"
                className="intent-primary"
                onClick={() => tickMission(selectedMissionId)}
                disabled={missionActionLoading}
              >
                Run Mission Tick
              </button>
            )}
          </div>
          {missionsMessage && <p className="connection-message">{missionsMessage}</p>}
          <div className="connection-cards">
            <article className="connection-card">
              <h3>Mission List</h3>
              {missions.length === 0 ? (
                <p>No missions yet.</p>
              ) : (
                <div className="clarification-list">
                  {missions.map((mission) => (
                    <button
                      key={mission.id}
                      type="button"
                      className="clarification-chip"
                      onClick={() => setSelectedMissionId(mission.id)}
                      aria-pressed={selectedMissionId === mission.id}
                      title={`${mission.status} · ${mission.terminalChildrenCount}/${mission.childRunsCount} child runs terminal`}
                    >
                      {mission.templateKind} · {mission.status}
                    </button>
                  ))}
                </div>
              )}
            </article>

            <article className="connection-card">
              <h3>Mission Detail</h3>
              {!selectedMission ? (
                <p>Select a mission to view status.</p>
              ) : (
                <>
                  <p>
                    Status: <code>{selectedMission.mission.status}</code>
                  </p>
                  <p>
                    Child runs: {selectedMission.mission.terminalChildrenCount}/
                    {selectedMission.mission.childRunsCount} terminal
                  </p>
                  {selectedMission.mission.failureReason && (
                    <p>Reason: {selectedMission.mission.failureReason}</p>
                  )}
                  <p>
                    Contract: terminal={selectedMission.contract.allChildrenTerminal ? "yes" : "no"} ·
                    blocked/pending={selectedMission.contract.hasBlockedOrPendingChild ? "yes" : "no"} ·
                    summary={selectedMission.contract.aggregationSummaryExists ? "yes" : "no"}
                  </p>
                  <div className="clarification-list">
                    {selectedMission.childRuns.map((child) => (
                      <div key={child.runId} className="clarification-card">
                        <p className="clarification-meta">
                          {child.childKey} · <code>{child.runState ?? child.status}</code>
                        </p>
                        {child.sourceLabel && <p>{child.sourceLabel}</p>}
                        {child.runFailureReason && <p>Reason: {child.runFailureReason}</p>}
                        <div className="diagnostic-actions">
                          <button
                            type="button"
                            className="clarification-chip"
                            onClick={() => loadContextReceiptForRun(child.runId)}
                            disabled={contextReceiptLoading}
                          >
                            {contextReceiptLoading && selectedMissionChildRunId === child.runId
                              ? "Loading context..."
                              : "View Context"}
                          </button>
                        </div>
                      </div>
                    ))}
                  </div>
                  {selectedMission.mission.summaryJson && (() => {
                    try {
                      const parsed = JSON.parse(selectedMission.mission.summaryJson);
                      const title = parsed?.title as string | undefined;
                      const lines = Array.isArray(parsed?.summaryLines) ? parsed.summaryLines.slice(0, 6) : [];
                      return (
                        <div className="runner-suppressed-list">
                          <p><strong>{title ?? "Mission Summary"}</strong></p>
                          {lines.length > 0 && (
                            <ul>
                              {lines.map((line: string, idx: number) => (
                                <li key={`${idx}_${line}`}>{line}</li>
                              ))}
                            </ul>
                          )}
                        </div>
                      );
                    } catch {
                      return null;
                    }
                  })()}
                </>
              )}
            </article>

            <article className="connection-card">
              <h3>Context Receipt (Read-only MVP)</h3>
              {!selectedMissionContextReceipt ? (
                <p>Select a child run and click “View Context”.</p>
              ) : (
                <>
                  <p className="clarification-meta">
                    Run <code>{selectedMissionContextReceipt.runId}</code> · {selectedMissionContextReceipt.recipe} ·{" "}
                    <code>{selectedMissionContextReceipt.runState}</code>
                  </p>
                  {contextReceiptMessage && <p className="connection-message">{contextReceiptMessage}</p>}
                  <p>
                    Provider: {selectedMissionContextReceipt.providerKind} ({selectedMissionContextReceipt.providerTier})
                    {" · "}
                    Receipt: {selectedMissionContextReceipt.terminalReceiptFound ? "present" : "not yet terminal"}
                  </p>
                  <p>
                    Redaction flags:{" "}
                    {selectedMissionContextReceipt.redactionFlags.length > 0
                      ? selectedMissionContextReceipt.redactionFlags.join(", ")
                      : "none"}
                  </p>

                  <div className="runner-suppressed-list">
                    <p><strong>Sources</strong></p>
                    {selectedMissionContextReceipt.sources.length === 0 ? (
                      <p>No source artifacts yet.</p>
                    ) : (
                      <ul>
                        {selectedMissionContextReceipt.sources.slice(0, 8).map((source, idx) => (
                          <li key={`${source.sourceKind}_${source.sourceId ?? source.url ?? idx}`}>
                            {source.sourceKind}
                            {source.url ? ` · ${source.url}` : ""}
                            {source.status ? ` · ${source.status}` : ""}
                            {typeof source.excerptChars === "number" ? ` · ${source.excerptChars} chars` : ""}
                            {typeof source.diffScore === "number" ? ` · diff ${source.diffScore.toFixed(2)}` : ""}
                            {source.error ? ` · ${source.error}` : ""}
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>

                  <div className="runner-suppressed-list">
                    <p><strong>Memory + Profile</strong></p>
                    <p>
                      Titles used:{" "}
                      {selectedMissionContextReceipt.memoryTitlesUsed.length > 0
                        ? selectedMissionContextReceipt.memoryTitlesUsed.join(", ")
                        : "none"}
                    </p>
                    <p>
                      Learning mode: {selectedMissionContextReceipt.runtimeProfileOverlay.mode} · enabled=
                      {selectedMissionContextReceipt.runtimeProfileOverlay.learningEnabled ? "yes" : "no"} ·
                      maxSources={selectedMissionContextReceipt.runtimeProfileOverlay.maxSources} ·
                      maxBullets={selectedMissionContextReceipt.runtimeProfileOverlay.maxBullets}
                    </p>
                    <div className="clarification-list">
                      {selectedMissionMemoryCards.slice(0, 8).map((card) => (
                        <div key={card.cardId} className="clarification-card">
                          <p className="clarification-meta">
                            {card.title} · <code>{card.cardType}</code> · {card.suppressed ? "suppressed" : "active"}
                          </p>
                          <p>Confidence: {card.confidence}</p>
                          <button
                            type="button"
                            className="clarification-chip"
                            onClick={() =>
                              toggleMemoryCardSuppression(card.autopilotId, card.cardId, !card.suppressed)
                            }
                          >
                            {card.suppressed ? "Unsuppress" : "Suppress"}
                          </button>
                        </div>
                      ))}
                    </div>
                  </div>

                  <div className="runner-suppressed-list">
                    <p><strong>Policy + Rationale</strong></p>
                    <p>
                      Deny-by-default primitives:{" "}
                      {selectedMissionContextReceipt.policyConstraints.denyByDefaultPrimitives ? "yes" : "no"}
                    </p>
                    <p>
                      Allowed domains:{" "}
                      {selectedMissionContextReceipt.policyConstraints.webAllowedDomains.length > 0
                        ? selectedMissionContextReceipt.policyConstraints.webAllowedDomains.join(", ")
                        : "none"}
                    </p>
                    <p>
                      Approval-required steps:{" "}
                      {selectedMissionContextReceipt.policyConstraints.approvalRequiredSteps.length > 0
                        ? selectedMissionContextReceipt.policyConstraints.approvalRequiredSteps.join(", ")
                        : "none"}
                    </p>
                    <p>
                      Send policy: allow={selectedMissionContextReceipt.policyConstraints.sendPolicy.allowSending ? "yes" : "no"}
                      {" · "}allowlist={selectedMissionContextReceipt.policyConstraints.sendPolicy.recipientAllowlistCount}
                      {" · "}max/day={selectedMissionContextReceipt.policyConstraints.sendPolicy.maxSendsPerDay}
                    </p>
                    <p>
                      Rationale codes:{" "}
                      {selectedMissionContextReceipt.rationaleCodes.length > 0
                        ? selectedMissionContextReceipt.rationaleCodes.join(", ")
                        : "none"}
                    </p>
                    <p>
                      Key signals:{" "}
                      {selectedMissionContextReceipt.keySignals.length > 0
                        ? selectedMissionContextReceipt.keySignals.join(", ")
                        : "none"}
                    </p>
                  </div>
                </>
              )}
            </article>
          </div>
        </section>

        <section className="diagnostics-panel" aria-label="Needs attention">
          <div className="connection-panel-header">
            <h2>Needs Attention</h2>
            <p>Supervisor diagnostics classify blocked runs and suggest safe next actions.</p>
          </div>
          {diagnosticsMessage && <p className="connection-message">{diagnosticsMessage}</p>}
          {runDiagnostics.filter((item) => !["healthy_running", "completed"].includes(item.healthStatus)).length === 0 ? (
            <div className="clarification-empty">
              <p>No runs need intervention right now.</p>
            </div>
          ) : (
            <div className="diagnostic-list">
              {runDiagnostics
                .filter((item) => !["healthy_running", "completed"].includes(item.healthStatus))
                .map((item) => (
                  <article key={item.id} className="diagnostic-card">
                    <div className="diagnostic-header-row">
                      <p className="clarification-kicker">Run health</p>
                      <span className={`diagnostic-status status-${item.healthStatus}`}>
                        {item.healthStatus.split("_").join(" ")}
                      </span>
                    </div>
                    <p className="diagnostic-summary">{item.summary}</p>
                    <p className="clarification-meta">
                      Run: <code>{item.runId}</code> · Autopilot: <code>{item.autopilotId}</code> · State:{" "}
                      <code>{item.runState}</code>
                    </p>
                    <div className="diagnostic-actions">
                      {item.suggestions.slice(0, 4).map((suggestion) => (
                        <button
                          key={`${item.id}_${suggestion.kind}`}
                          type="button"
                          className="clarification-chip"
                          disabled={suggestion.disabled}
                          title={suggestion.reason}
                          onClick={() => applyIntervention(item.runId, suggestion.kind)}
                        >
                          {suggestion.label}
                        </button>
                      ))}
                    </div>
                  </article>
                ))}
            </div>
          )}
        </section>

        <section className="clarifications-panel" aria-label="Clarifications">
          <div className="connection-panel-header">
            <h2>Clarifications</h2>
            <p>When Terminus is missing one detail, it asks one question and resumes immediately.</p>
          </div>
          {clarificationsMessage && <p className="connection-message">{clarificationsMessage}</p>}
          {clarifications.length === 0 ? (
            <div className="clarification-empty">
              <p>No clarifications waiting.</p>
            </div>
          ) : (
            <div className="clarification-list">
              {clarifications.map((item) => {
                const options = clarificationOptions(item);
                return (
                  <article key={item.id} className="clarification-card">
                    <p className="clarification-kicker">One thing I need to proceed</p>
                    <p className="clarification-question">{item.question}</p>
                    <p className="clarification-meta">
                      Run: <code>{item.runId}</code> · Field: <code>{item.fieldKey}</code>
                    </p>
                    {options.length > 0 && (
                      <div className="clarification-options" aria-label="Quick picks">
                        {options.map((option) => (
                          <button
                            key={option}
                            type="button"
                            className="clarification-chip"
                            onClick={() =>
                              setClarificationAnswers((prev) => ({ ...prev, [item.id]: option }))
                            }
                          >
                            {option}
                          </button>
                        ))}
                      </div>
                    )}
                    <div className="clarification-answer-row">
                      <input
                        aria-label={`Clarification answer for ${item.fieldKey}`}
                        value={clarificationAnswers[item.id] ?? ""}
                        onChange={(event) =>
                          setClarificationAnswers((prev) => ({
                            ...prev,
                            [item.id]: event.target.value,
                          }))
                        }
                        placeholder="Type one answer"
                      />
                      <button type="button" className="intent-primary" onClick={() => submitClarification(item)}>
                        Answer & Resume
                      </button>
                    </div>
                  </article>
                );
              })}
            </div>
          )}
        </section>

        <ConnectionPanel
          oauthProvider={oauthProvider}
          setOauthProvider={setOauthProvider}
          oauthClientId={oauthClientId}
          setOauthClientId={setOauthClientId}
          oauthRedirectUri={oauthRedirectUri}
          setOauthRedirectUri={setOauthRedirectUri}
          saveOauthSetup={saveOauthSetup}
          watcherAutopilotId={watcherAutopilotId}
          setWatcherAutopilotId={setWatcherAutopilotId}
          watcherMaxItems={watcherMaxItems}
          setWatcherMaxItems={setWatcherMaxItems}
          runnerControl={runnerControl}
          saveRunnerControl={saveRunnerControl}
          sendPolicyAutopilotId={sendPolicyAutopilotId}
          setSendPolicyAutopilotId={setSendPolicyAutopilotId}
          loadSendPolicy={loadSendPolicy}
          sendPolicy={sendPolicy}
          sendPolicyAllowlistInput={sendPolicyAllowlistInput}
          setSendPolicyAllowlistInput={setSendPolicyAllowlistInput}
          saveSendPolicy={saveSendPolicy}
          connectionsMessage={connectionsMessage}
          guideScopeType={guideScopeType}
          setGuideScopeType={setGuideScopeType}
          guideScopeId={guideScopeId}
          setGuideScopeId={setGuideScopeId}
          guideInstruction={guideInstruction}
          setGuideInstruction={setGuideInstruction}
          submitGuide={submitGuide}
          guideMessage={guideMessage}
          connections={connections}
          startOauth={startOauth}
          runWatcherTick={runWatcherTick}
          disconnectProvider={disconnectProvider}
          oauthSession={oauthSession}
          oauthCode={oauthCode}
          setOauthCode={setOauthCode}
          completeOauth={completeOauth}
          setOauthSession={setOauthSession}
        />
      </main>

      {intentOpen && (
        <div
          ref={intentOverlayRef}
          className="intent-overlay"
          role="dialog"
          aria-modal="true"
          aria-label="Intent Bar"
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) {
              setIntentOpen(false);
            }
          }}
          onKeyDown={handleIntentOverlayKeyDown}
        >
          <div className="intent-card">
            <div className="intent-header">
              <h2>Intent Bar</h2>
              <button type="button" className="intent-close" onClick={() => setIntentOpen(false)}>
                Close
              </button>
            </div>
            <p className="intent-help">Describe what you want done in one sentence.</p>
            <textarea
              ref={intentTextareaRef}
              className="intent-input"
              value={intentInput}
              onChange={(e) => setIntentInput(e.target.value)}
              placeholder="Example: Monitor https://example.com and send me an update when it changes"
            />
            <div className="intent-actions">
              <button
                type="button"
                className="intent-primary"
                onClick={() => generateDraft()}
                disabled={intentLoading}
              >
                {intentLoading ? "Preparing..." : "Prepare Setup"}
              </button>
            </div>
            {intentError && <p className="intent-error">{intentError}</p>}

            {draft && (
              <section className="draft-preview" aria-label="Run plan preview">
                <p className="draft-kind">{classifiedLabel}</p>
                <p className="draft-reason">{draft.classificationReason}</p>
                <div className="intent-actions">
                  {draft.kind === "one_off_run" ? (
                    <button
                      type="button"
                      onClick={() => generateDraft("draft_autopilot")}
                      disabled={intentLoading}
                    >
                      Make recurring
                    </button>
                  ) : (
                    <button
                      type="button"
                      onClick={() => generateDraft("one_off_run")}
                      disabled={intentLoading}
                    >
                      Run once
                    </button>
                  )}
                </div>
                <p className="draft-spend">{draft.preview.estimatedSpend}</p>
                <div className="draft-columns">
                  <div>
                    <h3>Will read</h3>
                    <ul>{draft.preview.reads.map((item) => <li key={item}>{item}</li>)}</ul>
                  </div>
                  <div>
                    <h3>Will execute</h3>
                    <ul>{draft.preview.writes.map((item) => <li key={item}>{item}</li>)}</ul>
                  </div>
                  <div>
                    <h3>Needs approval</h3>
                    <ul>
                      {draft.preview.approvalsRequired.length === 0
                        ? <li>None</li>
                        : draft.preview.approvalsRequired.map((item) => <li key={item}>{item}</li>)}
                    </ul>
                  </div>
                </div>
                <button
                  type="button"
                  className="intent-primary"
                  onClick={runDraft}
                  disabled={runDraftLoading}
                >
                  {runDraftLoading ? "Starting..." : draft.preview.primaryCta}
                </button>
              </section>
            )}
          </div>
        </div>
      )}
    </>
  );
}
