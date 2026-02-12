import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { HomeSnapshot } from "./types";

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

export function App() {
  const [snapshot, setSnapshot] = useState<HomeSnapshot>(fallbackSnapshot);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [retryCount, setRetryCount] = useState(0);

  const loadSnapshot = () => {
    setLoading(true);
    invoke<HomeSnapshot>("get_home_snapshot")
      .then((data) => {
        setSnapshot(data);
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
            <button
              type="button"
              className="retry-button"
              onClick={loadSnapshot}
              aria-label="Retry loading data"
            >
              Retry
            </button>
          </aside>
        )}
        
        <header className="hero">
          <p className="kicker">Terminus</p>
          <h1>Personal AI OS</h1>
          <p className="subhead">Autopilots, outcomes, approvals, and activity in one calm view.</p>
        </header>

      <section className="surface-grid" aria-label="Home surfaces" role="region">
        {snapshot.surfaces.map((surface) => (
          <article 
            key={surface.title} 
            className={`surface-card ${surface.count === 0 ? 'empty' : ''}`}
            aria-labelledby={`${surface.title.toLowerCase()}-title`}
          >
            <div>
              <h2 id={`${surface.title.toLowerCase()}-title`}>{surface.title}</h2>
              <p className="surface-subtitle">{surface.subtitle}</p>
            </div>
            <div className="surface-footer">
              <span className="count-badge" aria-label={`${surface.count} ${surface.count === 1 ? 'item' : 'items'}`}>
                {surface.count === 0 ? "Empty" : `${surface.count} ${surface.count === 1 ? 'item' : 'items'}`}
              </span>
              <button 
                type="button" 
                className="cta-button"
                aria-label={`${surface.cta} for ${surface.title}`}
              >
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
    </>
  );
}
