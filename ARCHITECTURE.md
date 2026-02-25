# Architecture Overview

Last updated: 2026-02-25
Version: MVP (v0.1.x, local-first desktop)

## What Terminus Is
Terminus is a local-first desktop system for repeatable professional follow-through — a **Personal AI OS and personal agent harness**.

The product surface is object-first: `Autopilots / Outcomes / Approvals / Activity`. Chat-style input exists only as an intake method (Intent Bar), not as the primary runtime surface.

**Harness framing:** The runtime provides the same structural guarantees that the best engineering teams build for their coding agents — architecture as guardrails (PrimitiveGuard), bounded tool catalog (11 primitives), documented preferences (Voice/Rules), and planning before execution (classify → preview → approve → run) — for non-technical professional users.

## Current Stack
- Desktop shell: Tauri 2 (macOS target first)
- Frontend: React 19 + TypeScript (strict)
- Runtime: Rust (tick-based runner)
- Storage: SQLite (WAL mode) + local vault files
- Secrets: macOS Keychain only
- Model providers: OpenAI + Anthropic (Supported), Gemini (Experimental)

## Runtime Model (Canonical)
The runtime is a persisted, bounded state machine.

Core commands:
- `start_recipe_run(...)` creates/persists a run and returns immediately
- `run_tick(run_id)` advances bounded work (single step / bounded transition)
- `resume_due_runs(limit)` resumes retrying runs that are due
- `tick_runner_cycle()` performs a bounded background/app-open cycle (runner + watcher cadence)

### Run states
- `ready`
- `running`
- `needs_approval`
- `needs_clarification` (non-terminal pause)
- `retrying`
- `succeeded`
- `failed`
- `blocked` (hard blocked, e.g. hard cap/policy)
- `canceled`

Key behavior:
- Clarifications are not terminal. Runs pause in `needs_clarification` and resume on answer.
- Hard caps block before side effects.
- Retries are bounded and scheduled via `next_retry_at_ms` with persisted backoff metadata.
- State transitions and activity rows are written atomically in one transaction.

## Action-First Completion Model
Canonical runtime primitives produce:
- `Actions` (typed executable work)
- `Approvals` (authorization gates for risky actions)
- `Outcomes` (completed work summary + receipts)

The user-facing product surface represents completed work or a one-tap approval to execute — never draft text for the user to copy elsewhere.

## Objects and Persistence
SQLite is the source of truth for runtime state.

Primary tables (selected):
- `runs` — persisted state machine, spend, retry metadata, provider kind/tier, plan JSON
- `activities` — human-readable audit trail of transitions/events
- `approvals` — pending/approved/rejected approvals, typed payload metadata, optional `action_id`
- `clarifications` — single-slot missing info cards (pending/answered/canceled)
- `actions` — canonical executable actions with idempotency keys
- `action_executions` — action execution attempts/results (idempotent execution receipts)
- `outcomes` — completed outcome payloads + terminal receipts
- `spend_ledger` — per-step spend entries (integer cents, idempotent keys)
- `provider_calls` — provider observability metadata (latency/usage/cost estimates)
- `autopilot_profile`, `decision_events`, `run_evaluations`, `adaptation_log`, `memory_cards` — Learning Layer
- `email_ingest_events`, `inbox_watcher_state` — inbox watcher ingestion + watcher backoff state
- `runner_control` — background runner + watcher cadence config/status

SQLite runtime hardening:
- WAL mode
- busy timeout
- migration-safe `ensure_column(...)` style upgrades
- schema metadata row (`schema_meta`)

## Recipes (Shared Runtime)
All four recipes run on one shared schema + runner + approvals + receipts stack.

**1. Website Monitor**
- `ReadWeb` (allowlisted domains only)
- snapshot persistence + change detection
- summarization / outcome generation
- approval-gated follow-through

**2. Inbox Triage** (paste/forward/share-in + watcher path)
- `ReadForwardedEmail`
- triage/classify/extract
- outcome/task-style completion and optional gated outbound/send
- OAuth watcher path; mailbox mutations/sends remain policy-gated

**3. Daily Brief**
- `ReadSources`
- `AggregateDailySummary`
- dedupe/history persistence
- outcome generation / delivery path stays constrained

**4. Custom (Dynamic Plan Generation) — P0 in active development**
- User describes any professional workflow in natural language via Intent Bar
- `generate_custom_plan()` sends primitives catalog + intent to LLM, receives plan JSON
- `validate_and_build_plan()` enforces safety invariants server-side (SendEmail always approval, max 10 steps, no invented primitives)
- Plan shown in Draft Plan Card for user review before committing
- `start_recipe_run` accepts `plan_json` parameter for pre-validated Custom plans
- Runner `execute_step()` dispatches by `PrimitiveId` (recipe-agnostic); only two coupling points needed updating (ReadWeb gate + prompt fallback)

## Provider + Transport Architecture

Provider layer:
- `ProviderKind`: OpenAI / Anthropic / Gemini
- `ProviderTier`: Supported / Experimental
- `ProviderRequest`, `ProviderResponse`, `ProviderError` (retryable classification)

Transport layer (`ExecutionTransport` trait, `src-tauri/src/transport/mod.rs`):
- `MockTransport` — deterministic tests, no network
- `LocalHttpTransport` — BYOK via Keychain-stored API keys (advanced/fallback)
- `RelayTransport` — **P1 in active development** — hosted plan via subscriber_token in Keychain; relay enforces tier limits, selects provider, returns response; enables remote approval via push channel (WebSocket/SSE)

Transport selection: if `subscriber_token` present → `RelayTransport`; else → `LocalHttpTransport`.

Secrets:
- Provider keys, OAuth tokens, subscriber token stored in macOS Keychain
- Never stored in SQLite
- Never logged or exported in receipts

## Security and Control Boundaries
Non-negotiable runtime boundaries enforced in code:
- Deny-by-default primitive allowlists (PrimitiveGuard)
- Approvals required for write/send actions by default
- `SendEmail` ALWAYS requires approval — enforced in both preset steps and `validate_and_build_plan()`
- Compose-first email policy with explicit send enable + allowlists + quiet hours + max/day
- Spend rails enforced at runtime in integer cents before side effects
- Receipts are redacted and human-readable

Web fetch hardening:
- HTTP/HTTPS only
- Domain allowlist required
- Redirect re-validation per hop
- Private/local/loopback rejection
- Resolved IP pinning into `curl` (`--resolve`) to reduce DNS rebinding risk
- Response size/content-type limits

Tauri hardening:
- Production CSP enabled (separate dev override config)
- Main window has explicit capability file boundary (`src-tauri/capabilities/main-window.json`)

## Background Runtime and Watchers
Terminus remains local-first:
- Background runner works while app process is alive and Mac is awake
- App-open mode is explicit when background mode is off
- Missed cycles are detected and surfaced as user-visible truth

Inbox watcher behavior:
- Provider-specific polling (Gmail / Microsoft 365)
- Per-provider dedupe (`email_ingest_events`)
- Provider-level backoff state (`inbox_watcher_state`)
- Gmail path uses batch message-details fetch (with sequential fallback)

## Learning Layer (Evaluate → Adapt → Memory)
Learning is local-first, bounded, and non-capability-expanding.

Pipeline (terminal runs only, and only when enabled):
1. Evaluate run
2. Apply bounded adaptation rules (allowlisted knobs only)
3. Update compact memory cards
4. Enrich terminal receipt with evaluation/adaptation/memory titles

Safety constraints:
- No raw email/web/provider payload storage in learning tables
- Bounded JSON schemas, rate limits, retention/compaction
- Learning cannot expand primitive allowlists, recipients, send toggles, or capabilities

## Known Debt (intentional / deferred)
- `runner.rs` (~236KB) and `App.tsx` (1,253 lines) are large and need structural decomposition (P7)
- Frontend test coverage is ~10% — App.tsx has zero tests; critical surfaces untested (P7)
- Fine-grained per-command Tauri app IPC permissions not yet defined
- Some DB schema inconsistencies (legacy columns/timestamps) need cleanup migration
- Gmail watcher batching is implemented but broader watcher observability/backoff UX can improve
- snake_case ↔ camelCase normalization layer in `uiLogic.ts` should be replaced with `#[serde(rename_all = "camelCase")]` on Rust structs

## How to Reason About the System
If you are modifying Terminus, preserve these invariants:
- Object-first UX (not chat-first)
- Tick-based bounded execution (no loop-to-terminal blocking)
- Deny-by-default primitives (PrimitiveGuard)
- Approvals for risky writes/sends (SendEmail always approval)
- Idempotent side effects + persisted receipts
- Local secrets only (Keychain)
- No capability growth from learning/guidance paths
- Relay is transport, not compute — runner stays local
- PrimitiveId must remain extensible (MCP direction, long-term)
