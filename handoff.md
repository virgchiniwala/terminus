# Handoff
Last updated: 2026-02-24

## Fresh Session Note
- For the agentic-orchestration plan, implemented supervisor diagnostics slice, and latest cross-session context summary, read `docs/AGENTIC_BEST_PRACTICES_PLAN_AND_STATUS_2026-02-24.md` first.

## What Is Shipped
- Object-first desktop shell and home surfaces
- Shared plan schema for Website Monitor, Inbox Triage, Daily Brief
- Deny-by-default primitive guard
- Persisted tick runner with approvals, retries, receipts, and idempotency
- Spend rails enforced in cents with hard-stop behavior before side effects
- Provider/transport abstraction with supported/experimental tiers
- Local secret handling via OS keychain (no secret persistence in repo/db)
- Learning Layer (Evaluate -> Adapt -> Memory) integrated at terminal run boundary
- Learning hardening:
  - safe `record_decision_event` schema/allowlist/rate-limit/idempotency
  - adaptation dedupe by hash + no-op suppression
  - bounded memory-card growth via deterministic upserts
  - manual `compact_learning_data` command with retention enforcement
- Universal Intake foundations:
  - `draft_intent` backend command (intent -> classified object draft + preview)
  - `⌘K` Intent Bar overlay in UI
  - one-off vs draft-autopilot classification with reason string
  - Draft Plan Card with read/write/approval/spend preview + `Run now` CTA
  - one-tap classification override in Intent Bar (`Make recurring` / `Run once`)
  - optional `forcedKind` support in `draft_intent` command for deterministic overrides
  - intake copy shifted toward setup/action semantics instead of draft-first labels
- Email connection foundations:
  - OAuth config + session + connection tables in SQLite
  - Keychain-backed OAuth token storage for Gmail and Microsoft 365
  - Tauri commands: list/save/start/complete/disconnect provider connection
  - Manual inbox watcher tick command with provider fetch + dedupe + run queueing
  - Runner control model + throttled watcher cadence command (`tick_runner_cycle`)
  - App-open polling loop in UI for watcher cadence visibility
- Safe Effector foundations for send:
  - `autopilot_send_policy` table and Tauri commands to get/update policy
  - typed approval payload fields on approvals (`payload_type`, `payload_json`)
  - `send.email` runner path enforces allowlist, max/day, quiet hours, and explicit enablement
  - send receipts persisted as `email_sent` outcomes with idempotent upsert by run/step/kind
  - send execution now routes through provider-backed email effectors (mock/local-http mode)
  - sender/thread ingest context is persisted (`provider_thread_id`, `sender_email`) for outbound execution
  - `send.email` uses connected inbox provider context and persists provider message/thread ids in receipts
- Inbox triage execution:
  - new `triage.email` primitive in plan schema and runtime
  - inbox triage step is approval-gated and executes provider-backed archive action
  - triage execution receipt persisted as `email_triage_executed`
- Runner cadence + reconciliation:
  - missed runner cycles are detected from `watcher_last_tick_ms` and poll interval
  - bounded catch-up loop runs up to 3 cycles before current tick execution
  - runner state persists `missed_runs_count` and Home shows explicit missed/offline truth
- Background runner execution:
  - tray icon enabled (`tray-icon` feature) with actions: open, run cycle now, quit
  - window close is intercepted: if background mode is enabled, window hides instead of exiting
  - backend thread ticks runner every ~10s when app process is alive and background mode is enabled
  - tick execution continues to use existing bounded runner model and `resume_due_runs`
- Scoped Guide foundations:
  - new `submit_guidance` Tauri command with explicit scope (`autopilot|run|approval|outcome`)
  - guidance classification modes: `applied`, `proposed_rule`, `needs_approval`
  - risky instructions (capability/policy expansion attempts) are blocked from auto-apply and returned as `needs_approval`
  - persisted `guidance_events` table stores scope, mode, bounded instruction, and result JSON
  - current UI includes a minimal Guide panel for scoped input and response messaging
- Transition foundations (draft-to-action canonicalization):
  - new `actions` and `action_executions` tables with idempotency keys
  - `approvals.action_id` support (approval rows can reference executable actions)
  - runner action interfaces: `ActionType`, `ActionRecord`, `ActionExecutionRecord`, `execute_action(...)`
  - clarification queue foundation: `clarifications` table, `list_pending_clarifications`, `submit_clarification_answer`
  - provider observability foundation: `provider_calls` table + provider dispatch logging hooks in runner
- Completed outcome semantics (compatibility-safe):
  - write-step provider outputs now also persist canonical `CreateOutcomeAction` + `action_executions`
  - `completed_outcome` rows are persisted for generated completion payloads
  - new primary outcomes query/count (`list_primary_outcomes`, run-based semantics) hides internal draft artifacts from Home outcomes count
  - duplicate approval on already-approved write step does not create duplicate `action_executions`
- Clarification queue UI (single-slot):
  - Home now renders a `Clarifications` object panel
  - pending clarifications load via `list_pending_clarifications`
  - one answer submits via `submit_clarification_answer` and immediately resumes the run
  - quick-pick options are supported when `options_json` is present
- Security + correctness hardening (audit-driven bundle):
  - `read.sources` now uses plan/domain allowlists (no self-authorized source fetch)
  - web fetches reject private/local/loopback targets in production paths (initial + redirects)
  - Daily Brief allowlist hosts are seeded from explicit sources at plan/run creation (compatibility-safe)
  - keychain writes no longer pass secrets via process argv (`security ... -w -` with stdin)
  - clarification `recipient` answers are written back into `runs.plan_json` before resume
  - learning pipeline runs only when `learning_enabled` and skips clarification-paused terminalization
  - runner status surfaces suppressed-autopilot count for visible suppression truth
  - SQLite connections configured with WAL + `busy_timeout`; background/tray tick errors are surfaced (sanitized)
  - inbox watcher uses autopilot preferred provider, parses Microsoft `receivedDateTime`, and returns clearer 429 failures
  - production CSP tightened; dev-only CSP moved to `src-tauri/tauri.dev.conf.json`
  - frontend fixes: polling stale closure, debounced runner/send policy writes, run-start double submit guard, clarification input label, modal Escape/click-outside/focus trap
  - redaction precision improved (avoid corrupting email addresses / `skipping`)
- State correctness + suppression visibility follow-up:
  - clarification pauses now persist as `runs.state = 'needs_clarification'` (non-terminal)
  - ticks no-op while clarification is pending; legacy `blocked + pending clarification` remains readable
  - primary outcomes/backlog queries treat `needs_clarification` as first-class and still support legacy blocked clarification rows
  - Home snapshot now includes suppressed Autopilot details (`autopilot_id`, `name`, `suppress_until_ms`) and UI shows them in the runner banner
- Security/infra follow-up:
  - `web::fetch_allowlisted_text` now resolves and pins a concrete IP per request hop using `curl --resolve` (reduces DNS rebinding window)
  - capability file scaffold added for main Tauri window (`src-tauri/capabilities/main-window.json`) and explicit main window label in `tauri.conf.json`
  - expired provider session with missing refresh token now clears stored connection/session state during token access path
- Reliability/cleanup follow-up:
  - Gmail watcher details fetch now uses Gmail batch endpoint (with sequential fallback)
  - provider-level watcher backoff state persisted in `inbox_watcher_state`
  - watcher cycle continues polling other providers when one provider fails
  - `list_primary_outcomes` now limits recent runs first via CTE + supporting indexes for approvals/outcomes/clarifications lookups
  - dead `RunState::Draft` enum variant removed
  - `ARCHITECTURE.md` refreshed to reflect current shipped runtime
- CI + quality gates baseline:
  - GitHub Actions CI workflow added for PRs and `main` pushes (macOS runner)
  - CI checks: `cargo fmt --check`, `cargo test`, `npm run lint`, `npm run build`
  - lightweight ESLint flat config added with `npm run lint`
- Watcher health UX (PR26):
  - `list_email_connections` now includes watcher state fields from `inbox_watcher_state`
    (`watcher_backoff_until_ms`, `watcher_consecutive_failures`, `watcher_last_error`, `watcher_updated_at_ms`)
  - Connections cards surface provider watcher health:
    - watcher ready
    - recovering (recent failures)
    - backoff/retry time for rate limits / temporary provider failures
  - Connection cards also surface provider connection issues (for reconnect-required cases)
- Frontend test foundation (PR27):
  - Vitest + React Testing Library + jsdom setup added
  - `npm test` script added and wired into CI
  - UI helper extraction (`src/uiLogic.ts`) for deterministic tests:
    - `normalizeSnapshot`
    - `normalizeEmailConnectionRecord`
    - watcher status messaging
    - polling retry error copy
    - debounce timer replacement helper
    - run-start guard helper
  - `ConnectionHealthSummary` extracted and covered with RTL rendering tests
- Structural refactor / prep (PR28):
  - `App.tsx` Connections surface extracted to `src/components/ConnectionPanel.tsx` (behavior-preserving)
  - `main.rs` pure guidance/retry/log sanitization helpers extracted to `src-tauri/src/guidance_utils.rs`
  - moved guidance/retry helper unit tests into `guidance_utils` module tests
  - reduced primary file sizes to improve next-step refactors:
    - `src/App.tsx`: 1166 -> 911 lines
    - `src-tauri/src/main.rs`: 1175 -> 1033 lines
- Mission Orchestration MVP (Iteration 2):
  - new SQLite tables: `missions`, `mission_runs`, `mission_events`
  - new backend module: `src-tauri/src/missions.rs`
  - mission template MVP: `daily_brief_multi_source` (fan-out child daily-brief runs + deterministic aggregate summary)
  - mission commands:
    - `create_mission_draft`
    - `start_mission`
    - `get_mission`
    - `list_missions`
    - `run_mission_tick`
  - completion contract enforced in mission detail/tick:
    - all child runs terminal
    - no child in `needs_approval` / `needs_clarification` / `blocked`
    - aggregation summary present before `succeeded`
  - minimal Home Missions panel added (list + detail + tick + demo mission create button)

## Current Verification Baseline
- `cd src-tauri && cargo fmt --check` passes
- `cd src-tauri && cargo test` passes
- `npm test` passes
- `npm run lint` passes
- `npm run build` passes

## Mission MVP DevTools / Manual Validation
1. Create a mission draft:
   - `create_mission_draft({ templateKind: "daily_brief_multi_source", intent: "Create a mission brief", provider: "openai", sources: ["Inline note: A", "Inline note: B"] })`
2. Start mission:
   - `start_mission({ draft })`
3. Tick mission until terminal:
   - `run_mission_tick({ missionId })`
4. Inspect mission:
   - `get_mission({ missionId })`
   - verify contract flags and `summaryJson` after child success

## Current Priority Track
- Canonical priority docs:
  - `docs/TERMINUS_PRODUCT_STRATEGY_v3.md`
  - `tasks/TERMINUS_TASKLIST_v3.md`
- Active track (top-down):
  1. P0.11/P0.12 Voice object + rule extraction approval flow
  2. P1 provider routing/caching and structural hardening cleanup
  3. UX and copy cleanup for calm language and trust clarity

## Learning Storage and Privacy Guardrails
- Learning stores bounded metadata only (hashes, counts, latencies, reason codes).
- Learning does not store raw email text, raw website text, provider payload dumps, auth headers, or keys.
- `record_decision_event` is insert-only and cannot mutate runtime permissions or outbound controls.
- Retention policy:
  - decision events: last 500 / 90 days per autopilot
  - adaptation log: last 200 per autopilot
  - run evaluations: last 500 / 180 days per autopilot
  - memory cards: bounded by upsert key and size limits

## DevTools Validation Snippets
1. Accept bounded decision event:
   - `record_decision_event({ autopilotId, runId, eventType: "outcome_opened", metadataJson: "{\"reason_code\":\"opened\"}", clientEventId: "evt_1" })`
2. Reject unsafe decision event:
   - `record_decision_event({ ..., metadataJson: "{\"reason_code\":\"Authorization: Bearer secret\"}" })`
   - expected: human-readable validation error
3. Manual compaction dry run:
   - `compact_learning_data({ autopilotId, dryRun: true })`
4. Manual compaction apply:
   - `compact_learning_data({ autopilotId, dryRun: false })`
5. Watcher health payload check:
   - `list_email_connections()`
   - confirm each provider row includes watcher fields and non-zero `watcher_consecutive_failures` / future `watcher_backoff_until_ms` after simulated rate-limit failures

## Operational Truths
- Local-first runtime
- Runs execute while app is open; background mode and awake-state truth are explicit product surfaces
- Inbox automation path is provider-connected watcher + dedupe
- Compose-first outbound policy remains default; send remains policy-gated

## Immediate Risks to Watch
- Clone drift toward chat-first or harness-first UI
- Scope drift into marketplace/tool-authoring behavior
- Silent capability expansion from adaptation logic
- Contradictory currency policy across docs/UI/runtime naming
- Residual structural debt from large files (`runner.rs`, `App.tsx`) despite functional hardening

## Known Deferred Audit Items (not in this PR)
- Large refactors: split `App.tsx`, split `runner.rs`, move `main.rs` business logic into modules
- Full state-model cleanup beyond clarification (legacy `blocked` migration/normalization)
- Fine-grained per-command app IPC permissions (current capability file provides window boundary + core permissions only)
- Schema-wide FK cascade rebuild / migration framework cleanup
- Legacy money/timestamp schema cleanup (float spend columns + timestamp normalization)

## Next Suggested Work
1. Wire learning outcomes into object surfaces with calm, non-technical language.
2. Complete schedule suggestion UX gate (only after first successful run).
3. Add policy tests around outbound quiet hours and recipient/domain allowlists.
