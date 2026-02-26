# Handoff
Last updated: 2026-02-26

## Fresh Session Note
- For the agentic-orchestration plan, implemented supervisor diagnostics slice, and latest cross-session context summary, read `docs/AGENTIC_BEST_PRACTICES_PLAN_AND_STATUS_2026-02-24.md` first.
- For the document-workflow pivot (Controlled Context Mode, PE-first wedge), read `/Users/vir.c/.claude/plans/new_terminus_plan_26Feb26.md` before planning new feature work.

## Current Work (Phase 0 Spike)
- Vault extraction viability spike for the revised document-workflow wedge
  - new `probe_vault_extraction` Tauri command (PDF/XLSX/TXT/MD implemented; DOCX dependency wired and marked for manual parser/fidelity validation)
  - added extraction crates + Tauri dialog plugin wiring
  - added `docs/VAULT_EXTRACTION_SPIKE.md` checklist and Go/No-Go gate
  - no runner/vault product behavior changes yet

## Current Work (PR45)
- Relay multi-device routing + device targeting foundations (in progress)
  - device registry and preferred-target routing policy
  - local desktop auto-registration / heartbeat
  - relay sync/push gating when this device is standby or manual-target-only
  - minimal Connections UI for relay devices and fallback policy

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
- Dynamic Plan Generation (Custom Recipe):
  - `RecipeKind::Custom` added to shared plan schema
  - `draft_intent` can generate LLM-backed custom plans using existing primitive catalog vocabulary
  - server-side validation enforces step bounds + primitive restrictions + approval/risk overrides
  - `start_recipe_run` accepts `plan_json` for pre-generated custom plans and re-validates before execution
  - runner `ReadWeb` path now supports `custom` recipe plans
- Relay transport client plumbing (desktop-side):
  - new `RelayTransport` implementing `ExecutionTransport`
  - subscriber token stored in macOS Keychain (no SQLite) via `set_subscriber_token` / `remove_subscriber_token`
  - `get_transport_status` exposes Hosted/BYOK/Mock mode + relay endpoint for UI visibility
  - provider runtime chooses transport dynamically (Hosted Relay when token exists; BYOK local or Mock fallback)
  - Connections panel shows execution mode and hosted token controls (minimal, no billing UI)
- Remote approval path hooks + provenance (desktop-side):
  - new Tauri commands: `approve_run_approval_remote`, `reject_run_approval_remote`
  - local and remote approval commands now use shared wrappers around canonical `RunnerEngine::approve/reject`
  - approval decision provenance persisted on `approvals` (`decided_channel`, `decided_by`)
  - terminal receipts now include `approval_resolutions[]` (approval id, step id, status, timestamp, channel, actor)
- Relay callback auth + readiness (desktop-side):
  - `resolve_relay_approval_callback` command validates callback secret and bounded request age, then resolves approvals via canonical codepath
  - relay callback replay/idempotency table: `relay_callback_events` (`request_id` unique)
  - Keychain stores relay callback secret + stable relay device id (for relay registration/readiness)
  - Connections panel shows remote approval readiness, device id, callback secret status, and pending approval count
  - callback secret can be issued/cleared from UI via Tauri commands (`issue_relay_callback_secret`, `clear_relay_callback_secret`)
- Relay approval sync loop (desktop-side polling MVP):
  - `tick_relay_approval_sync` polls relay for remote approval decisions using subscriber token + device id
  - `get_relay_sync_status` surfaces sync health/backoff (`relay_sync_state`) for the Connections panel
  - sync applies decisions through the same callback/approval codepath (reuses replay/idempotency protections)
  - `tick_runner_cycle` now runs bounded relay sync after due-run resume and records sync status in summary
- Relay push channel consumer (desktop-side long-poll/SSE-ready):
  - `tick_relay_approval_push` listens for remote approval decisions via relay stream endpoint (long-poll request)
  - `get_relay_push_status` surfaces push-channel health/backoff using a separate `relay_sync_state` row (`approval_push`)
  - push path falls back to poll endpoint if stream endpoint is unavailable/rejected (compatibility during relay rollout)
  - dedicated background relay push thread runs when background mode is enabled (separate from runner cycle thread)
  - Connections panel now shows both poll sync status and push channel status, each with manual trigger controls
- Interview-driven onboarding MVP (first-result flow):
  - `onboarding_state` SQLite singleton tracks onboarding completion, dismissal, answers, and recommended intent
  - Tauri commands: `get_onboarding_state`, `save_onboarding_state`, `dismiss_onboarding`
  - Home renders a guided onboarding panel (role / work focus / biggest repetitive pain) for first-run users
  - “Recommend First Autopilot” saves onboarding answers, generates a suggested intent, opens the Intent Bar, and drafts a runnable plan
  - onboarding auto-completes when the first successful run is detected (`runs.state = 'succeeded'`)
- Voice object MVP (global + per-Autopilot override):
  - SQLite tables: `voice_config` (singleton) and `autopilot_voice_config`
  - Tauri commands: `get_global_voice_config`, `update_global_voice_config`, `get_autopilot_voice_config`, `update_autopilot_voice_config`, `clear_autopilot_voice_config`
  - Home Voice panel supports global defaults and per-Autopilot override (tone/length/humor/notes)
  - Runner injects effective Voice config at the shared provider dispatch gateway (wording-only guidance)
  - Voice validation is allowlisted and bounded; notes are capped/sanitized
- Relay-backed Webhook Trigger MVP:
  - SQLite tables: `webhook_triggers`, `webhook_trigger_events`, `relay_webhook_callback_events`
  - Keychain-only per-trigger signing secrets (`terminus.webhook_trigger_secret.{trigger_id}`)
  - Tauri commands:
    - `list_webhook_triggers`
    - `create_webhook_trigger`
    - `rotate_webhook_trigger_secret`
    - `enable_webhook_trigger` / `disable_webhook_trigger`
    - `get_webhook_trigger_events`
    - `resolve_relay_webhook_callback`
    - `ingest_webhook_event_local_debug` (dev-only)
  - Inbound webhook validation:
    - relay callback auth + request freshness
    - webhook HMAC SHA256 signature + timestamp freshness
    - JSON-only content type
    - payload size cap (default 32 KB)
    - duplicate delivery dedupe via `(trigger_id, event_idempotency_key)`
  - Webhook deliveries enqueue runs via the existing runner + approvals/spend rails/receipts
  - Home Webhook panel (MVP) supports trigger creation, pause/resume, secret rotate, and delivery list
- `CallApi` primitive MVP (bounded outbound HTTP, approval-gated):
  - `PrimitiveId::CallApi` + shared schema `api_call_request` (`url`, `method`, `header_key_ref`, auth header/scheme, optional JSON body)
  - custom-plan validation forces `CallApi` to `requires_approval = true` and `risk_tier = high`
  - `CallApi` is not default-allowlisted in preset recipes; only custom plans can opt in via validated steps
  - API key refs stored in Keychain only (`terminus.api_key_ref.{ref_name}`); no secrets in SQLite/logs/receipts
  - bounded runtime execution enforces http/https + GET/POST, domain allowlist, timeout, response-size cap, and redacted excerpts
  - `api_call_result` outcome artifact persisted for inspection/receipts
  - Connections panel includes advanced API key-ref save/check/remove controls
- Codex OAuth BYOK support (OpenAI/Codex, advanced mode):
  - imports local Codex CLI OAuth session from `~/.codex/auth.json` (ChatGPT/Codex auth mode)
  - stores imported tokens in Keychain (`terminus.openai.codex_oauth_bundle`), never SQLite/logs/receipts
  - Local BYOK OpenAI requests fall back to Codex OAuth access token when no manual API key is set
  - Connections panel includes Codex OAuth import/remove/status controls (advanced)
  - parser tests validate local auth file shape handling and missing-token failure behavior
- Gmail PubSub trigger path (Gmail-only, polling fallback):
  - new `gmail_pubsub_state` + `gmail_pubsub_events` tables and relay callback replay table
  - relay callback command `resolve_relay_gmail_pubsub_callback` + debug helper for local simulation
  - PubSub callbacks parse/validate bounded PubSub envelope, dedupe events, then invoke existing Gmail watcher fetch/queue path
  - trigger mode support (`polling`, `gmail_pubsub`, `auto`) with fallback to polling when PubSub is inactive/expired/error
  - Connections panel shows Gmail PubSub mode, watch health, config fields, and recent PubSub events
  - backend tests cover envelope parsing, event dedupe, duplicate callback dedupe, and fetch failure recording
- Relay multi-device routing foundations (PR45):
  - new `relay_devices` + `relay_routing_policy` tables (migration-safe, seeded policy singleton)
  - relay status/readiness paths auto-register this desktop device and maintain last-seen timestamps
  - relay sync/push returns `device_not_target` with human-readable message when routing policy blocks local pulls
  - Connections panel shows relay device list (preferred/standby/offline/disabled) and offline fallback policy control

## Current Verification Baseline
- `cd src-tauri && cargo fmt --check` passes
- `cd src-tauri && cargo test` passes
- `cd src-tauri && cargo test` passes (93 tests)
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
  1. P1 Slack/mobile approval routing on relay server (consume desktop callback contract + push delivery)
  2. P0.12 Rule extraction approval flow ("Make this a rule")
  3. Onboarding + Voice UI test coverage (Intent Bar, onboarding panel, voice panel)

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
6. Webhook trigger create + simulated delivery (MVP):
   - `create_webhook_trigger({ input: { autopilotId: "auto_inbox_watch_gmail", description: "Webhook test" } })`
   - copy `signingSecretPreview` (shown once)
   - `ingest_webhook_event_local_debug({ input: { triggerId, deliveryId: "demo_1", bodyJson: "{\"event\":\"ping\"}" } })`
   - `get_webhook_trigger_events({ triggerId, limit: 10 })` and verify `queued` + `runId`
   - repeat same `deliveryId/body` and verify `duplicate`

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
1. Implement Rule object + approval-gated "Make this a rule" flow (bounded overlays only).
2. Add relay multi-device routing + device targeting foundations (preferred target + queue/fallback policy).
3. Add runtime ownership leases + doctor surfaces (relay push / trigger consumers / operator diagnostics).
