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

## Current Verification Baseline
- `cd src-tauri && cargo test` passes
- `npm run build` passes

## Current Priority Track
- Canonical priority docs:
  - `docs/TERMINUS_PRODUCT_STRATEGY_v3.md`
  - `tasks/TERMINUS_TASKLIST_v3.md`
- Active track (top-down):
  1. P0 background runner + due-run scheduler + missed-run reconciliation
  2. P0 scoped Guide/Voice/Rules without chat-first drift
  3. P1 provider routing/caching and security hardening

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
