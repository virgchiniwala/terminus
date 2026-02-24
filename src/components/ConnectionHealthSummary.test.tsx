import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ConnectionHealthSummary } from "./ConnectionHealthSummary";

describe("ConnectionHealthSummary", () => {
  it("renders backoff and failure details for a connected provider", () => {
    render(
      <ConnectionHealthSummary
        record={{
          provider: "gmail",
          status: "connected",
          accountEmail: "user@example.com",
          scopes: [],
          connectedAtMs: 1,
          updatedAtMs: 1,
          lastError: "Reconnect required",
          watcherBackoffUntilMs: Date.now() + 60_000,
          watcherConsecutiveFailures: 3,
          watcherLastError: "Rate limited",
          watcherUpdatedAtMs: Date.now(),
        }}
      />
    );

    expect(screen.getByText(/Connection issue:/i)).toBeInTheDocument();
    expect(screen.getByText(/Retrying at/i)).toBeInTheDocument();
    expect(screen.getByText(/Recent failures: 3/i)).toBeInTheDocument();
    expect(screen.getByText(/Last watcher issue:/i)).toBeInTheDocument();
  });

  it("renders inactive watcher state when disconnected", () => {
    render(
      <ConnectionHealthSummary
        record={{
          provider: "microsoft365",
          status: "disconnected",
          accountEmail: null,
          scopes: [],
          connectedAtMs: null,
          updatedAtMs: 1,
          lastError: null,
          watcherBackoffUntilMs: null,
          watcherConsecutiveFailures: 0,
          watcherLastError: null,
          watcherUpdatedAtMs: null,
        }}
      />
    );

    expect(screen.getByText("Watcher inactive until connected.")).toBeInTheDocument();
    expect(screen.queryByText(/Recent failures:/i)).not.toBeInTheDocument();
  });
});

