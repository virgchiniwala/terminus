import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  ApplyInterventionResult,
  EmailConnectionRecord,
  HomeSnapshot,
  InterventionSuggestion,
  IntentDraftResponse,
  OAuthStartResponse,
  RecipeKind,
  RunDiagnosticRecord,
  RunnerControlRecord,
  AutopilotSendPolicyRecord,
  ClarificationRecord,
  IntentDraftKind,
} from "./types";

const fallbackSnapshot: HomeSnapshot = {
  surfaces: [
    { title: "Autopilots", subtitle: "Create repeatable follow-through", count: 0, cta: "Create Autopilot" },
    { title: "Outcomes", subtitle: "Results from completed runs", count: 0, cta: "View Outcomes" },
    { title: "Approvals", subtitle: "Actions waiting for your go-ahead", count: 0, cta: "Open Queue" },
    { title: "Activity", subtitle: "What happened and why", count: 0, cta: "Open Activity" },
  ],
  runner: {
    mode: "app_open",
    statusLine: "Autopilots run only while the app is open.",
  },
};

function nowId(prefix: string): string {
  return `${prefix}_${Date.now()}`;
}

function normalizeSnapshot(raw: unknown): HomeSnapshot {
  const value = raw as {
    surfaces?: HomeSnapshot["surfaces"];
    runner?: {
      mode?: "app_open" | "background";
      statusLine?: string;
      status_line?: string;
      backlogCount?: number;
      backlog_count?: number;
      watcherEnabled?: boolean;
      watcher_enabled?: boolean;
      watcherLastTickMs?: number | null;
      watcher_last_tick_ms?: number | null;
      missedRunsCount?: number;
      missed_runs_count?: number;
      suppressedAutopilotsCount?: number;
      suppressed_autopilots_count?: number;
      suppressedAutopilots?: Array<{
        autopilotId?: string;
        autopilot_id?: string;
        name?: string;
        suppressUntilMs?: number;
        suppress_until_ms?: number;
      }>;
      suppressed_autopilots?: Array<{
        autopilotId?: string;
        autopilot_id?: string;
        name?: string;
        suppressUntilMs?: number;
        suppress_until_ms?: number;
      }>;
    };
  };
  return {
    surfaces: value.surfaces ?? fallbackSnapshot.surfaces,
    runner: {
      mode: value.runner?.mode ?? "app_open",
      statusLine: value.runner?.statusLine ?? value.runner?.status_line ?? fallbackSnapshot.runner.statusLine,
      backlogCount: value.runner?.backlogCount ?? value.runner?.backlog_count ?? 0,
      watcherEnabled: value.runner?.watcherEnabled ?? value.runner?.watcher_enabled ?? true,
      watcherLastTickMs:
        value.runner?.watcherLastTickMs ?? value.runner?.watcher_last_tick_ms ?? null,
      missedRunsCount:
        value.runner?.missedRunsCount ?? value.runner?.missed_runs_count ?? 0,
      suppressedAutopilotsCount:
        value.runner?.suppressedAutopilotsCount ?? value.runner?.suppressed_autopilots_count ?? 0,
      suppressedAutopilots: (value.runner?.suppressedAutopilots ??
        value.runner?.suppressed_autopilots ??
        [])?.map((item) => ({
        autopilotId: item.autopilotId ?? item.autopilot_id ?? "",
        name: item.name ?? item.autopilotId ?? item.autopilot_id ?? "Autopilot",
        suppressUntilMs: item.suppressUntilMs ?? item.suppress_until_ms ?? Date.now(),
      })),
    },
  };
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

function formatShortLocalTime(ms: number): string {
  try {
    return new Date(ms).toLocaleString([], {
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
    });
  } catch {
    return "soon";
  }
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
        const isFirstFailure = retryCountRef.current === 0;
        setError(
          isFirstFailure
            ? "Could not load data. Using default view."
            : "Still unable to connect. Check that Tauri backend is running."
        );
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
        const normalized = rows.map((row: any) => ({
          provider: row.provider,
          status: row.status,
          accountEmail: row.accountEmail ?? row.account_email ?? null,
          scopes: row.scopes ?? [],
          connectedAtMs: row.connectedAtMs ?? row.connected_at_ms ?? null,
          updatedAtMs: row.updatedAtMs ?? row.updated_at_ms ?? Date.now(),
          lastError: row.lastError ?? row.last_error ?? null,
        })) as EmailConnectionRecord[];
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
    if (runnerControlSaveTimerRef.current != null) {
      window.clearTimeout(runnerControlSaveTimerRef.current);
    }
    runnerControlSaveTimerRef.current = window.setTimeout(() => {
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
    if (sendPolicySaveTimerRef.current != null) {
      window.clearTimeout(sendPolicySaveTimerRef.current);
    }
    sendPolicySaveTimerRef.current = window.setTimeout(() => {
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
  }, [loadSnapshot, loadConnections, loadRunnerControl, loadClarifications, loadRunDiagnostics]);

  useEffect(() => {
    const interval = window.setInterval(() => {
      invoke("tick_runner_cycle")
        .then(() => {
          loadSnapshot();
          loadClarifications();
          loadRunDiagnostics();
        })
        .catch(() => {
          // keep silent; runner status remains visible on Home
        });
    }, 10_000);
    return () => window.clearInterval(interval);
  }, [loadSnapshot, loadClarifications, loadRunDiagnostics]);

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
    if (!draft || runDraftLoading) {
      return;
    }
    const autopilotId = nowId(draft.kind === "draft_autopilot" ? "autopilot" : "run");
    const idempotencyKey = nowId("idem");
    const dailySources = recipeNeedsSources(draft.plan.recipe) ? draft.plan.dailySources : undefined;
    const pastedText = recipeNeedsPastedText(draft.plan.recipe) ? draft.plan.inboxSourceText : undefined;

    setRunDraftLoading(true);
    invoke("start_recipe_run", {
      autopilotId,
      recipe: draft.plan.recipe,
      intent: draft.plan.intent,
      pastedText,
      dailySources,
      provider: draft.plan.provider.id,
      idempotencyKey,
      maxRetries: 2,
    })
      .then(() => {
        setRunNotice(`${draft.preview.primaryCta} started. Open Activity for live progress.`);
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

        <section className="connection-panel" aria-label="Email connections">
          <div className="connection-panel-header">
            <h2>Email Connections</h2>
            <p>Connect Gmail or Microsoft 365 once so inbox automations can run while your Mac is awake.</p>
          </div>
          <div className="connection-setup-grid">
            <label>
              Provider
              <select
                value={oauthProvider}
                onChange={(event) => setOauthProvider(event.target.value as "gmail" | "microsoft365")}
              >
                <option value="gmail">Gmail</option>
                <option value="microsoft365">Microsoft 365</option>
              </select>
            </label>
            <label>
              OAuth Client ID
              <input
                value={oauthClientId}
                onChange={(event) => setOauthClientId(event.target.value)}
                placeholder="Paste OAuth client id"
              />
            </label>
            <label>
              Redirect URI
              <input
                value={oauthRedirectUri}
                onChange={(event) => setOauthRedirectUri(event.target.value)}
                placeholder="https://your-app/callback"
              />
            </label>
            <button type="button" className="intent-primary" onClick={saveOauthSetup}>
              Save Setup
            </button>
          </div>
          <div className="watcher-controls">
            <label>
              Inbox Autopilot ID
              <input
                value={watcherAutopilotId}
                onChange={(event) => setWatcherAutopilotId(event.target.value)}
                placeholder="auto_inbox_watch"
              />
            </label>
            <label>
              Max emails per tick
              <input
                type="number"
                min={1}
                max={25}
                value={watcherMaxItems}
                onChange={(event) => setWatcherMaxItems(Number(event.target.value) || 10)}
              />
            </label>
          </div>
          {runnerControl && (
            <div className="watcher-controls">
              <label>
                <span>Background runner</span>
                <select
                  value={runnerControl.backgroundEnabled ? "on" : "off"}
                  onChange={(event) =>
                    saveRunnerControl({
                      ...runnerControl,
                      backgroundEnabled: event.target.value === "on",
                    })
                  }
                >
                  <option value="off">Off</option>
                  <option value="on">On</option>
                </select>
              </label>
              <label>
                <span>Inbox watcher</span>
                <select
                  value={runnerControl.watcherEnabled ? "on" : "off"}
                  onChange={(event) =>
                    saveRunnerControl({
                      ...runnerControl,
                      watcherEnabled: event.target.value === "on",
                    })
                  }
                >
                  <option value="on">Active</option>
                  <option value="off">Paused</option>
                </select>
              </label>
              <label>
                <span>Watcher interval (seconds)</span>
                <input
                  type="number"
                  min={15}
                  max={900}
                  value={runnerControl.watcherPollSeconds}
                  onChange={(event) =>
                    saveRunnerControl({
                      ...runnerControl,
                      watcherPollSeconds: Number(event.target.value) || 60,
                    })
                  }
                />
              </label>
              <label>
                <span>Watcher max emails</span>
                <input
                  type="number"
                  min={1}
                  max={25}
                  value={runnerControl.watcherMaxItems}
                  onChange={(event) =>
                    saveRunnerControl({
                      ...runnerControl,
                      watcherMaxItems: Number(event.target.value) || 10,
                    })
                  }
                />
              </label>
            </div>
          )}
          <div className="watcher-controls">
            <label>
              <span>Send policy Autopilot ID</span>
              <input
                value={sendPolicyAutopilotId}
                onChange={(event) => setSendPolicyAutopilotId(event.target.value)}
                placeholder="auto_inbox_watch_gmail"
              />
            </label>
            <label>
              <span>&nbsp;</span>
              <button type="button" onClick={loadSendPolicy}>
                Load Send Policy
              </button>
            </label>
          </div>
          {sendPolicy && (
            <div className="watcher-controls">
              <label>
                <span>Sending</span>
                <select
                  value={sendPolicy.allowSending ? "on" : "off"}
                  onChange={(event) =>
                    saveSendPolicy({ ...sendPolicy, allowSending: event.target.value === "on" })
                  }
                >
                  <option value="off">Compose only</option>
                  <option value="on">Allow sending</option>
                </select>
              </label>
              <label>
                <span>Recipient allowlist (comma separated)</span>
                <input
                  value={sendPolicyAllowlistInput}
                  onChange={(event) => setSendPolicyAllowlistInput(event.target.value)}
                  onBlur={() =>
                    saveSendPolicy({
                      ...sendPolicy,
                      recipientAllowlist: sendPolicyAllowlistInput
                        .split(",")
                        .map((x) => x.trim())
                        .filter((x) => x.length > 0),
                    })
                  }
                  placeholder="person@example.com, @company.com"
                />
              </label>
              <label>
                <span>Max sends per day</span>
                <input
                  type="number"
                  min={1}
                  max={200}
                  value={sendPolicy.maxSendsPerDay}
                  onChange={(event) =>
                    saveSendPolicy({
                      ...sendPolicy,
                      maxSendsPerDay: Number(event.target.value) || sendPolicy.maxSendsPerDay,
                    })
                  }
                />
              </label>
              <label>
                <span>Allow outside quiet hours</span>
                <select
                  value={sendPolicy.allowOutsideQuietHours ? "yes" : "no"}
                  onChange={(event) =>
                    saveSendPolicy({
                      ...sendPolicy,
                      allowOutsideQuietHours: event.target.value === "yes",
                    })
                  }
                >
                  <option value="no">No</option>
                  <option value="yes">Yes</option>
                </select>
              </label>
            </div>
          )}
          {connectionsMessage && <p className="connection-message">{connectionsMessage}</p>}
          <div className="watcher-controls">
            <label>
              <span>Guide scope</span>
              <select
                value={guideScopeType}
                onChange={(event) =>
                  setGuideScopeType(event.target.value as "autopilot" | "run" | "approval" | "outcome")
                }
              >
                <option value="autopilot">Autopilot</option>
                <option value="run">Run</option>
                <option value="approval">Approval</option>
                <option value="outcome">Outcome</option>
              </select>
            </label>
            <label>
              <span>Scope ID</span>
              <input
                value={guideScopeId}
                onChange={(event) => setGuideScopeId(event.target.value)}
                placeholder="autopilot_123 / run_123 / ..."
              />
            </label>
            <label>
              <span>Guide instruction</span>
              <input
                value={guideInstruction}
                onChange={(event) => setGuideInstruction(event.target.value)}
                placeholder="One thing to change for this item"
              />
            </label>
            <label>
              <span>&nbsp;</span>
              <button type="button" onClick={submitGuide}>
                Apply Guide
              </button>
            </label>
          </div>
          {guideMessage && <p className="connection-message">{guideMessage}</p>}

          <div className="connection-cards">
            {connections.map((record) => (
              <article key={record.provider} className="connection-card">
                <h3>{record.provider === "gmail" ? "Gmail" : "Microsoft 365"}</h3>
                <p>Status: {record.status === "connected" ? "Connected" : "Disconnected"}</p>
                {record.accountEmail && <p>Account: {record.accountEmail}</p>}
                <div className="connection-actions">
                  <button type="button" onClick={() => startOauth(record.provider)}>
                    {record.status === "connected" ? "Reconnect" : "Connect"}
                  </button>
                  <button
                    type="button"
                    onClick={() => runWatcherTick(record.provider)}
                    disabled={record.status !== "connected"}
                  >
                    Poll Inbox Now
                  </button>
                  {record.status === "connected" && (
                    <button type="button" onClick={() => disconnectProvider(record.provider)}>
                      Disconnect
                    </button>
                  )}
                </div>
              </article>
            ))}
          </div>

          {oauthSession && (
            <div className="oauth-flow">
              <p>
                Open this link to authorize {oauthSession.provider === "gmail" ? "Gmail" : "Microsoft 365"}:
              </p>
              <a href={oauthSession.authUrl} target="_blank" rel="noreferrer">
                {oauthSession.authUrl}
              </a>
              <label>
                Authorization code
                <input
                  value={oauthCode}
                  onChange={(event) => setOauthCode(event.target.value)}
                  placeholder="Paste code from callback"
                />
              </label>
              <div className="connection-actions">
                <button type="button" className="intent-primary" onClick={completeOauth}>
                  Complete Connection
                </button>
                <button type="button" onClick={() => setOauthSession(null)}>
                  Cancel
                </button>
              </div>
            </div>
          )}
        </section>
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
