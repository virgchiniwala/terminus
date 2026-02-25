import type {
  EmailConnectionRecord,
  HomeSnapshot,
  IntentDraftResponse,
  OnboardingStateRecord,
} from "./types";

export const fallbackSnapshot: HomeSnapshot = {
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

export function normalizeSnapshot(raw: unknown): HomeSnapshot {
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
      watcherLastTickMs: value.runner?.watcherLastTickMs ?? value.runner?.watcher_last_tick_ms ?? null,
      missedRunsCount: value.runner?.missedRunsCount ?? value.runner?.missed_runs_count ?? 0,
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

export function normalizeEmailConnectionRecord(row: unknown): EmailConnectionRecord {
  const value = row as Record<string, unknown>;
  return {
    provider: (value.provider as "gmail" | "microsoft365") ?? "gmail",
    status: (value.status as "connected" | "disconnected") ?? "disconnected",
    accountEmail: (value.accountEmail as string | null) ?? (value.account_email as string | null) ?? null,
    scopes: (value.scopes as string[]) ?? [],
    connectedAtMs:
      (value.connectedAtMs as number | null) ?? (value.connected_at_ms as number | null) ?? null,
    updatedAtMs: (value.updatedAtMs as number) ?? (value.updated_at_ms as number) ?? Date.now(),
    lastError: (value.lastError as string | null) ?? (value.last_error as string | null) ?? null,
    watcherBackoffUntilMs:
      (value.watcherBackoffUntilMs as number | null) ??
      (value.watcher_backoff_until_ms as number | null) ??
      null,
    watcherConsecutiveFailures:
      (value.watcherConsecutiveFailures as number) ??
      (value.watcher_consecutive_failures as number) ??
      0,
    watcherLastError:
      (value.watcherLastError as string | null) ?? (value.watcher_last_error as string | null) ?? null,
    watcherUpdatedAtMs:
      (value.watcherUpdatedAtMs as number | null) ??
      (value.watcher_updated_at_ms as number | null) ??
      null,
  };
}

export function normalizeOnboardingStateRecord(row: unknown): OnboardingStateRecord {
  const value = row as Record<string, unknown>;
  return {
    onboardingComplete:
      (value.onboardingComplete as boolean) ?? (value.onboarding_complete as boolean) ?? false,
    dismissed: (value.dismissed as boolean) ?? false,
    roleText: (value.roleText as string) ?? (value.role_text as string) ?? "",
    workFocusText: (value.workFocusText as string) ?? (value.work_focus_text as string) ?? "",
    biggestPainText:
      (value.biggestPainText as string) ?? (value.biggest_pain_text as string) ?? "",
    recommendedIntent:
      (value.recommendedIntent as string | null) ??
      (value.recommended_intent as string | null) ??
      null,
    startedAtMs: (value.startedAtMs as number) ?? (value.started_at_ms as number) ?? Date.now(),
    updatedAtMs: (value.updatedAtMs as number) ?? (value.updated_at_ms as number) ?? Date.now(),
    completedAtMs:
      (value.completedAtMs as number | null) ?? (value.completed_at_ms as number | null) ?? null,
    dismissedAtMs:
      (value.dismissedAtMs as number | null) ?? (value.dismissed_at_ms as number | null) ?? null,
    firstSuccessfulRunAtMs:
      (value.firstSuccessfulRunAtMs as number | null) ??
      (value.first_successful_run_at_ms as number | null) ??
      null,
  };
}

export function formatShortLocalTime(ms: number): string {
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

export function watcherStatusLine(record: EmailConnectionRecord, nowMs = Date.now()): string {
  if (record.status !== "connected") {
    return "Watcher inactive until connected.";
  }
  const backoffUntil = record.watcherBackoffUntilMs ?? null;
  if (backoffUntil && backoffUntil > nowMs) {
    return `Rate-limited or temporarily unavailable. Retrying at ${formatShortLocalTime(backoffUntil)}.`;
  }
  const failures = record.watcherConsecutiveFailures ?? 0;
  if (failures > 0) {
    return `Watcher recovering (${failures} recent failure${failures === 1 ? "" : "s"}).`;
  }
  return "Watcher ready.";
}

export function homeLoadErrorMessage(retryCount: number): string {
  return retryCount === 0
    ? "Could not load data. Using default view."
    : "Still unable to connect. Check that Tauri backend is running.";
}

export function canStartDraftRun(
  draft: IntentDraftResponse | null,
  runDraftLoading: boolean
): boolean {
  return Boolean(draft) && !runDraftLoading;
}

export type TimeoutScheduler = Pick<typeof window, "setTimeout" | "clearTimeout">;

export function replaceDebouncedTimer(
  scheduler: TimeoutScheduler,
  currentTimer: number | null,
  callback: () => void,
  delayMs: number
): number {
  if (currentTimer != null) {
    scheduler.clearTimeout(currentTimer);
  }
  return scheduler.setTimeout(callback, delayMs);
}
