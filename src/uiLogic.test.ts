import { describe, expect, it, vi } from "vitest";
import type { IntentDraftResponse } from "./types";
import {
  canStartDraftRun,
  homeLoadErrorMessage,
  normalizeEmailConnectionRecord,
  normalizeSnapshot,
  replaceDebouncedTimer,
  watcherStatusLine,
} from "./uiLogic";

describe("uiLogic", () => {
  it("normalizes snake_case home snapshot payloads", () => {
    const snapshot = normalizeSnapshot({
      runner: {
        status_line: "Ready",
        backlog_count: 2,
        missed_runs_count: 3,
        suppressed_autopilots_count: 1,
        suppressed_autopilots: [
          {
            autopilot_id: "auto_1",
            suppress_until_ms: 1234,
          },
        ],
      },
    });

    expect(snapshot.runner.statusLine).toBe("Ready");
    expect(snapshot.runner.backlogCount).toBe(2);
    expect(snapshot.runner.missedRunsCount).toBe(3);
    expect(snapshot.runner.suppressedAutopilotsCount).toBe(1);
    expect(snapshot.runner.suppressedAutopilots?.[0]).toEqual({
      autopilotId: "auto_1",
      name: "auto_1",
      suppressUntilMs: 1234,
    });
  });

  it("normalizes email connection watcher fields", () => {
    const row = normalizeEmailConnectionRecord({
      provider: "gmail",
      status: "connected",
      account_email: "user@example.com",
      scopes: [],
      updated_at_ms: 10,
      last_error: null,
      watcher_backoff_until_ms: 5000,
      watcher_consecutive_failures: 2,
      watcher_last_error: "Rate limited",
      watcher_updated_at_ms: 4500,
    });

    expect(row.accountEmail).toBe("user@example.com");
    expect(row.watcherBackoffUntilMs).toBe(5000);
    expect(row.watcherConsecutiveFailures).toBe(2);
    expect(row.watcherLastError).toBe("Rate limited");
    expect(row.watcherUpdatedAtMs).toBe(4500);
  });

  it("returns first and subsequent polling error messages", () => {
    expect(homeLoadErrorMessage(0)).toContain("Using default view");
    expect(homeLoadErrorMessage(1)).toContain("Still unable to connect");
  });

  it("gates run start on draft presence and loading state", () => {
    const fakeDraft = { kind: "one_off_run" } as IntentDraftResponse;
    expect(canStartDraftRun(null, false)).toBe(false);
    expect(canStartDraftRun(fakeDraft, true)).toBe(false);
    expect(canStartDraftRun(fakeDraft, false)).toBe(true);
  });

  it("debounces writes by replacing the previous timer", () => {
    vi.useFakeTimers();
    const calls: string[] = [];

    const firstTimer = replaceDebouncedTimer(window, null, () => calls.push("first"), 300);
    replaceDebouncedTimer(window, firstTimer, () => calls.push("second"), 300);

    vi.advanceTimersByTime(299);
    expect(calls).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(calls).toEqual(["second"]);
    vi.useRealTimers();
  });

  it("reports watcher backoff and recovery states", () => {
    const now = 1_000;
    expect(
      watcherStatusLine(
        {
          provider: "gmail",
          status: "connected",
          accountEmail: null,
          scopes: [],
          connectedAtMs: null,
          updatedAtMs: now,
          lastError: null,
          watcherBackoffUntilMs: now + 10_000,
          watcherConsecutiveFailures: 2,
          watcherLastError: "Rate limited",
          watcherUpdatedAtMs: now,
        },
        now
      )
    ).toContain("Retrying at");

    expect(
      watcherStatusLine(
        {
          provider: "gmail",
          status: "connected",
          accountEmail: null,
          scopes: [],
          connectedAtMs: null,
          updatedAtMs: now,
          lastError: null,
          watcherBackoffUntilMs: null,
          watcherConsecutiveFailures: 1,
          watcherLastError: null,
          watcherUpdatedAtMs: null,
        },
        now
      )
    ).toContain("recovering");
  });
});
