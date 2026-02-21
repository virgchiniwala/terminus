import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { HomeSnapshot, IntentDraftResponse, RecipeKind } from "./types";

const fallbackSnapshot: HomeSnapshot = {
  surfaces: [
    { title: "Autopilots", subtitle: "Create repeatable follow-through", count: 0, cta: "Create Autopilot" },
    { title: "Outcomes", subtitle: "Results from completed runs", count: 0, cta: "View Outcomes" },
    { title: "Approvals", subtitle: "Drafts waiting for your go-ahead", count: 0, cta: "Open Queue" },
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
    runner?: { mode?: "app_open" | "background"; statusLine?: string; status_line?: string };
  };
  return {
    surfaces: value.surfaces ?? fallbackSnapshot.surfaces,
    runner: {
      mode: value.runner?.mode ?? "app_open",
      statusLine: value.runner?.statusLine ?? value.runner?.status_line ?? fallbackSnapshot.runner.statusLine,
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

  const [intentOpen, setIntentOpen] = useState(false);
  const [intentInput, setIntentInput] = useState("");
  const [intentError, setIntentError] = useState<string | null>(null);
  const [intentLoading, setIntentLoading] = useState(false);
  const [draft, setDraft] = useState<IntentDraftResponse | null>(null);
  const [runNotice, setRunNotice] = useState<string | null>(null);

  const loadSnapshot = () => {
    setLoading(true);
    invoke<HomeSnapshot>("get_home_snapshot")
      .then((data) => {
        setSnapshot(normalizeSnapshot(data));
        setError(null);
        setRetryCount(0);
      })
      .catch((err) => {
        console.error("Failed to load home snapshot:", err);
        const isFirstFailure = retryCount === 0;
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
  };

  useEffect(() => {
    loadSnapshot();
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
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
  }, []);

  const classifiedLabel = useMemo(() => {
    if (!draft) {
      return "";
    }
    return draft.kind === "draft_autopilot" ? "Draft Autopilot" : "One-off Run";
  }, [draft]);

  const generateDraft = () => {
    const intent = intentInput.trim();
    if (!intent) {
      setIntentError("Add a one-line intent to continue.");
      return;
    }
    setIntentLoading(true);
    setIntentError(null);
    setRunNotice(null);
    invoke<IntentDraftResponse>("draft_intent", { intent })
      .then((payload) => {
        setDraft(normalizeDraft(payload));
      })
      .catch((err) => {
        console.error("Failed to draft intent:", err);
        setIntentError(typeof err === "string" ? err : "Could not prepare this draft yet.");
      })
      .finally(() => {
        setIntentLoading(false);
      });
  };

  const runDraft = () => {
    if (!draft) {
      return;
    }
    const autopilotId = nowId(draft.kind === "draft_autopilot" ? "autopilot" : "run");
    const idempotencyKey = nowId("idem");
    const dailySources = recipeNeedsSources(draft.plan.recipe) ? draft.plan.dailySources : undefined;
    const pastedText = recipeNeedsPastedText(draft.plan.recipe) ? draft.plan.inboxSourceText : undefined;

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
        </section>
      </main>

      {intentOpen && (
        <div className="intent-overlay" role="dialog" aria-modal="true" aria-label="Intent Bar">
          <div className="intent-card">
            <div className="intent-header">
              <h2>Intent Bar</h2>
              <button type="button" className="intent-close" onClick={() => setIntentOpen(false)}>
                Close
              </button>
            </div>
            <p className="intent-help">Describe what you want done in one sentence.</p>
            <textarea
              className="intent-input"
              value={intentInput}
              onChange={(e) => setIntentInput(e.target.value)}
              placeholder="Example: Monitor https://example.com and send me an update when it changes"
            />
            <div className="intent-actions">
              <button type="button" className="intent-primary" onClick={generateDraft} disabled={intentLoading}>
                {intentLoading ? "Preparing..." : "Create Draft"}
              </button>
            </div>
            {intentError && <p className="intent-error">{intentError}</p>}

            {draft && (
              <section className="draft-preview" aria-label="Draft plan preview">
                <p className="draft-kind">{classifiedLabel}</p>
                <p className="draft-reason">{draft.classificationReason}</p>
                <p className="draft-spend">{draft.preview.estimatedSpend}</p>
                <div className="draft-columns">
                  <div>
                    <h3>Will read</h3>
                    <ul>{draft.preview.reads.map((item) => <li key={item}>{item}</li>)}</ul>
                  </div>
                  <div>
                    <h3>Will create</h3>
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
                <button type="button" className="intent-primary" onClick={runDraft}>
                  {draft.preview.primaryCta}
                </button>
              </section>
            )}
          </div>
        </div>
      )}
    </>
  );
}
