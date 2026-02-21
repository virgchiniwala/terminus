# Handoff
Last updated: 2026-02-22

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

## Current Verification Baseline
- `cd src-tauri && cargo test` passes
- `npm run build` passes

## Current Priority Track
- Canonical priority docs:
  - `docs/TERMINUS_PRODUCT_STRATEGY_v3.md`
  - `tasks/TERMINUS_TASKLIST_v3.md`
- Active track (top-down):
  1. P0.11/P0.12 Voice object + rule extraction approval flow
  2. P1 provider routing/caching and security hardening
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

## Next Suggested Work
1. Wire learning outcomes into object surfaces with calm, non-technical language.
2. Complete schedule suggestion UX gate (only after first successful run).
3. Add policy tests around outbound quiet hours and recipient/domain allowlists.
