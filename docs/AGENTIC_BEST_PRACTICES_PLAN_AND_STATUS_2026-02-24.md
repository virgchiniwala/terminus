# Agentic Best-Practices Plan + Status (Fresh Session Handoff)
Last updated: 2026-02-25

## Why This File Exists
This file captures:
- the agentic-orchestration plan discussed from external article research
- what was actually implemented in Terminus from that plan
- the current repo/project state from the latest `main` docs

Use this as the fast-start context for a fresh Codex session before proposing new work.

## Executive Summary
Terminus already implements many strong agentic/runtime best practices (bounded state machine, approvals, receipts, retries, learning/memory, local-first safety).

The original gap areas in this plan have now been implemented in MVP form:
- supervisor-grade diagnostics/interventions (Workstream A)
- mission-level orchestration (Workstream B MVP)
- context/memory provenance UX (Workstream C MVP)

Current gap has shifted to the next orchestration layer:
- webhook/relay-triggered orchestration inputs (inbound external events)
- richer rule extraction and operator teaching loops
- hosted relay packaging and remote execution ergonomics

## Sources That Informed the Plan (Conceptually)
- OpenClaw/Krause-style orchestration patterns (context partitioning, deterministic monitoring, completion contracts, resource-aware concurrency)
- Simon Willison's agentic engineering patterns (pattern discipline, explicit loops/evaluation)
- Chowder/Takopi-style operator UX patterns (status visibility, event lifecycle, actionable notifications)

Terminus adapts these patterns to a trust-first product:
- deny-by-default primitives
- approval gates for writes/sends
- no arbitrary shell/code execution
- local-first secrets and runtime

## Adopt / Adapt / Reject (Terminus framing)
### Adopt
- centralized orchestration logic
- context partitioning (business/policy/memory vs execution context)
- deterministic monitoring loops
- explicit completion contracts
- bounded retries with actionable operator notifications
- resource-aware coordination
- visible status/receipts

### Adapt for Terminus
- implement orchestration as typed runs/missions over the existing runner
- expose supervision in object-first UI (Run/Mission/Approval/Outcome), not chat
- keep interventions strictly allowlisted and auditable

### Reject
- unrestricted agent permissions
- arbitrary shell/tmux agent spawning
- bypass-approval flags
- opaque autonomous side effects
- direct prod DB access by execution agents

## Original Plan (Decision-Complete Direction)
### Workstream A — Supervisor Loop (Run Health + Interventions)
- Derived `RunHealth` classification from runs/approvals/clarifications/retry metadata/failure reasons
- Deterministic intervention suggestions (no LLM dependency)
- Persisted diagnostic snapshots (`run_diagnostics`)
- Safe `apply_intervention` command with strict allowlist
- Home/Mission Control "Needs Attention" surface

### Workstream B — Mission Orchestration (Safe Fan-Out / Fan-In)
- First-class `missions`, `mission_runs`, `mission_events`
- Parent/child coordination with completion contracts
- Initial templates only:
  - `daily_brief_multi_source`
  - `website_monitor_batch`
  - `inbox_triage_batch`
- Resource governance/concurrency budgeting surfaced in runner controls

### Workstream C — Context + Memory Provenance UX
- `Context Receipt` artifact per run/mission
- visible memory/source/policy inputs and rationale codes
- memory suppression/provenance controls
- readiness gate + notification conditions for actionable completion only

## What Was Implemented (This Session / Merged PR)
User reported the PR from branch `codex/supervisor-diagnostics-panel` was created and merged.

### Implemented Scope (Workstream A slice)
- New backend diagnostics module:
  - `src-tauri/src/diagnostics.rs`
- Deterministic run-health classification statuses:
  - `healthy_running`
  - `waiting_for_approval`
  - `waiting_for_clarification`
  - `retrying_transient`
  - `retrying_stuck`
  - `policy_blocked`
  - `provider_misconfigured`
  - `source_unreachable`
  - `resource_throttled`
  - `completed`
  - `failed_unclassified`
- Allowlisted intervention suggestions and apply API
- Persisted diagnostic snapshots in SQLite (`run_diagnostics`)
- Tauri commands:
  - `list_run_diagnostics`
  - `apply_intervention`
- Learning-layer helper to support intervention:
  - autopilot suppression setter (`pause_autopilot_15m`)
- Frontend wiring:
  - types for diagnostics/interventions
  - Home "Needs Attention" panel to surface blocked/problem runs
  - buttons for safe interventions

### Intervention Kinds Implemented
- `approve_pending_action`
- `answer_clarification` (requires actual answer via Clarifications panel)
- `retry_now_if_due`
- `pause_autopilot_15m`
- `reduce_source_scope` (Daily Brief only)
- `switch_provider_supported_default`
- `open_receipt` (informational)
- `open_activity_log` (informational)

### Validation Performed During Implementation
- `npm run build` passed
- `cargo test diagnostics::tests::classifies_reason_patterns` passed

Note:
- Full `cargo test` in the sandbox environment still hit existing local bind permission failures in some runner web/server tests (`Operation not permitted`). This was environmental, not introduced by the diagnostics slice.

## What Was Implemented After This Plan (Workstreams B + C)
### Mission Orchestration (Workstream B MVP) — Implemented
- `missions`, `mission_runs`, `mission_events` tables shipped
- Mission commands (`create_mission_draft`, `start_mission`, `get_mission`, `list_missions`, `run_mission_tick`) shipped
- Mission UI surface shipped (Missions panel with list/detail/tick)
- Completion contract shipped (child runs terminal + no blocked/pending child + summary present)

### Context + Memory Provenance (Workstream C MVP) — Implemented
- Context Receipt read API/UI shipped (run/mission legibility)
- memory provenance read APIs shipped
- memory suppress/unsuppress controls shipped (autopilot-scoped, bounded)

## Current Workstream Status (as of 2026-02-25)
- Workstream A (Supervisor Loop): **Implemented**
- Workstream B (Mission Orchestration MVP): **Implemented**
- Workstream C (Context + Memory Provenance MVP): **Implemented**
- New active follow-on: **Rule Cards ("Teach Once") + BYOK auth improvements** (bounded operator teaching + reduced setup friction)

## Current Project State (from latest `main` docs)
Source inspected:
- `/Users/vir.c/terminus/mission_control.md`
- `/Users/vir.c/terminus/handoff.md`

### `mission_control.md` (current snapshot)
- Tracks current/next work on live `main`, not a PR worktree snapshot
- Current baseline includes dynamic plans, relay transport/approvals, onboarding, voice, webhook triggers, and `CallApi` MVP
- Next priorities now center on Rule Cards ("Teach Once") + API/action-layer expansion after `CallApi`
  - extract `main.rs` helper logic into `guidance_utils.rs`
  - reduce file size / improve seams
- Next priorities in that snapshot:
  1. Voice object + rule extraction approval flow (P0.11/P0.12)
  2. Structural hardening follow-up
  3. Watcher health UI follow-up (dedicated status surface)

### `handoff.md` (PR28 snapshot)
- Last updated: 2026-02-24
- Includes shipped work through:
  - PR26 watcher health UX
  - PR27 frontend test foundation (Vitest + RTL + `npm test`)
  - PR28 structural refactor/prep
- Verification baseline in that snapshot says:
  - `cargo fmt --check` passes
  - `cargo test` passes
  - `npm test` passes
  - `npm run lint` passes
  - `npm run build` passes

## Important Sequencing Note (Updated)
This file started as a proposal; the repo has since advanced beyond the original scope.

Use it now as:
- a record of the orchestration concepts Terminus adopted, and
- a status bridge for what is already implemented vs. what comes next

Current documented priorities have moved beyond Workstreams A/B/C into:
- relay-backed triggers/integration ingress
- rule extraction and operator teaching loops
- hosted relay packaging + onboarding polish

## Recommended Next-Step Focus (Current)
1. **Rule extraction / "Teach Once"**
   - explicit, approval-gated reusable behavior changes
2. **Outbound API expansion after `CallApi` MVP**
   - broader HTTP/API policy model, templates, and safer reusable API actions
3. **BYOK auth UX follow-up**
   - optional in-app Codex OAuth browser flow + clearer expiry/reconnect handling

## If Continuing the Agentic Plan, Start Here (Implementation Order)
1. **Mission Orchestration MVP (smallest vertical slice)**
   - `daily_brief_multi_source` only
   - parent/child rows + one completion contract
   - no generic DAG builder
2. **Mission UI surface**
   - simple mission list + mission detail card
   - no large UI redesign
3. **Context Receipt MVP**
   - read-only receipt first (sources/memory/policy/rationale)
4. **Memory provenance controls**
   - suppress/re-enable only

## Guardrails for Any Future Session (Do Not Regress)
- No chat-first or harness-first UI drift
- No new primitive that enables arbitrary shell/code execution
- No capability escalation via interventions/memory/adaptation
- Keep interventions deterministic, allowlisted, and auditable
- Preserve approval defaults for write/send actions
- Keep all user-facing failures human-readable

## Quick References
- Core runtime state machine: `src-tauri/src/runner.rs`
- SQLite schema/bootstrap: `src-tauri/src/db.rs`
- Learning layer: `src-tauri/src/learning/mod.rs`
- Home UI shell: `src/App.tsx`
- Latest mission control snapshot: `/Users/vir.c/terminus/mission_control.md`
- Latest handoff snapshot: `/Users/vir.c/terminus/handoff.md`
