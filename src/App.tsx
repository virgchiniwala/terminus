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

  useEffect(() => {
    invoke<HomeSnapshot>("get_home_snapshot")
      .then(setSnapshot)
      .catch(() => {
        setSnapshot(fallbackSnapshot);
      });
  }, []);

  return (
    <main className="app-shell" aria-label="Terminus Home">
      <header className="hero">
        <p className="kicker">Terminus</p>
        <h1>Personal AI OS</h1>
        <p className="subhead">Autopilots, outcomes, approvals, and activity in one calm view.</p>
      </header>

      <section className="surface-grid" aria-label="Home surfaces">
        {snapshot.surfaces.map((surface) => (
          <article key={surface.title} className="surface-card" aria-label={surface.title}>
            <div>
              <h2>{surface.title}</h2>
              <p>{surface.subtitle}</p>
            </div>
            <div className="surface-footer">
              <span>{surface.count === 0 ? "Empty" : `${surface.count} items`}</span>
              <button type="button">{surface.cta}</button>
            </div>
          </article>
        ))}
      </section>

      <section className="runner-banner" aria-label="Runner status">
        <strong>Runner mode:</strong> {snapshot.runner.mode === "background" ? "Background" : "App Open"}
        <p>{snapshot.runner.statusLine}</p>
      </section>
    </main>
  );
}
