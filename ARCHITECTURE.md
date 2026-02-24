# Architecture Overview

Last updated: 2026-02-22
Version: MVP (v0.1.x, local-first desktop)

## What Terminus Is
Terminus is a local-first desktop system for repeatable follow-through.
The product surface is object-first: `Autopilots / Outcomes / Approvals / Activity`.
Chat-style input exists only as an intake method (Intent Bar), not as the primary runtime surface.

## Current Stack
- Desktop shell: Tauri (macOS target first)
- Frontend: React + TypeScript
- Runtime: Rust (tick-based runner)
- Storage: SQLite + local vault files (vault usage remains constrained)
- Secrets: macOS Keychain only
- Model providers: OpenAI + Anthropic (Supported), Gemini (Experimental)

## Runtime Model (Canonical)
The runtime is a persisted, bounded state machine.

Core commands:
- `start_recipe_run(...)` creates/persists a run and returns immediately
- `run_tick(run_id)` advances bounded work (single step / bounded transition)
- `resume_due_runs(limit)` resumes retrying runs that are due
- `tick_runner_cycle()` performs a bounded background/app-open cycle (runner + watcher cadence)

### Run states (current)
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
Terminus is transitioning from draft-first internals to action-canonical behavior.

Canonical runtime primitives produce:
- `Actions` (typed executable work)
- `Approvals` (authorization gates for risky actions)
- `Outcomes` (completed work summary + receipts)

Important distinction:
- Generated text may still exist as payload/internal compatibility artifacts.
- The user-facing product surface should represent completed work or a one-tap approval to execute.

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
- `autopilot_profile`, `decision_events`, `run_evaluations`, `adaptation_log`, `memory_cards` — Learning Layer data
- `email_ingest_events`, `inbox_watcher_state` — inbox watcher ingestion + watcher backoff state
- `runner_control` — background runner + watcher cadence config/status

SQLite runtime hardening already in place:
- WAL mode
- busy timeout
- migration-safe `ensure_column(...)` style upgrades
- schema metadata row (`schema_meta`)

## Shared MVP Preset Runtime
All three MVP presets run on one shared schema + runner + approvals + receipts stack.

1. Website Monitor
- `read_web` (allowlisted domains only)
- snapshot persistence + change detection
- summarization / outcome generation
- approval-gated follow-through (e.g. email send remains gated)

2. Inbox Triage (MVP input: paste/forward/share-in + watcher path)
- `read_forwarded_email`
- triage/classify/extract
- outcome/task-style completion and optional gated outbound/send
- OAuth watcher path exists but mailbox mutations/sends remain policy-gated

3. Daily Brief
- `read_sources`
- `aggregate_daily_summary`
- dedupe/history persistence
- outcome generation / delivery path stays constrained

## Provider + Transport Architecture
Provider execution is abstracted behind provider and transport seams.

Provider layer:
- `ProviderKind`: OpenAI / Anthropic / Gemini
- `ProviderTier`: Supported / Experimental
- `ProviderRequest`, `ProviderResponse`, `ProviderError` (retryable classification)

Transport layer:
- `MockTransport` (deterministic tests)
- `LocalHttpTransport` (real BYOK local execution)
- Future seam: hosted Relay transport (not implemented)

Secrets:
- Provider keys and OAuth tokens are stored in macOS Keychain
- Never stored in SQLite
- Never logged or exported in receipts

## Security and Control Boundaries
Non-negotiable runtime boundaries enforced in code:
- deny-by-default primitive allowlists
- approvals required for write/send actions by default
- compose-first email policy with explicit send enable + allowlists + quiet hours + max/day
- spend rails enforced at runtime in integer cents
- receipts are redacted and human-readable

Web fetch hardening (current):
- HTTP/HTTPS only
- domain allowlist required
- redirect re-validation per hop
- private/local/loopback rejection
- resolved IP pinning into `curl` (`--resolve`) to reduce DNS rebinding risk
- response size/content-type limits

Tauri hardening (current baseline):
- production CSP enabled (separate dev override config)
- main window has explicit capability file boundary (`src-tauri/capabilities/main-window.json`)

## Background Runtime and Watchers
Terminus remains local-first:
- background runner works while app process is alive and Mac is awake
- app-open mode is explicit when background mode is off
- missed cycles are detected and surfaced as user-visible truth

Inbox watcher behavior:
- provider-specific polling (Gmail / Microsoft 365)
- per-provider dedupe (`email_ingest_events`)
- provider-level backoff state (`inbox_watcher_state`) for rate-limit/retryable failures
- Gmail path uses batch message-details fetch (with sequential fallback)

## Learning Layer (Evaluate → Adapt → Memory)
Learning is local-first, bounded, and non-capability-expanding.

Pipeline (terminal runs only, and only when enabled):
1. Evaluate run
2. Apply bounded adaptation rules (allowlisted knobs only)
3. Update compact memory cards
4. Enrich terminal receipt with evaluation/adaptation/memory titles

Safety constraints:
- no raw email/web/provider payload storage in learning tables
- bounded JSON schemas, rate limits, retention/compaction
- learning cannot expand primitive allowlists, recipients, send toggles, or capabilities

## Current Known Debt (intentional / deferred)
- `runner.rs` and `App.tsx` are still large and need structural decomposition
- fine-grained per-command Tauri app IPC permissions are not yet defined (window capability boundary exists)
- some DB schema inconsistencies (legacy columns/timestamps) remain for compatibility and need a cleanup migration plan
- Gmail watcher batching is implemented, but broader watcher observability/backoff UX can be improved further

## How to Reason About the System
If you are modifying Terminus, preserve these invariants:
- object-first UX (not chat-first)
- tick-based bounded execution (no loop-to-terminal blocking)
- deny-by-default primitives
- approvals for risky writes/sends
- idempotent side effects + persisted receipts
- local secrets only (Keychain)
- no capability growth from learning/guidance paths
