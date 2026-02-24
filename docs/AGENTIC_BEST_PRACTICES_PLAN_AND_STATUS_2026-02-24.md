# Agentic Best-Practices Plan + Status (Fresh Session Handoff)
Last updated: 2026-02-24

## Why This File Exists
This file captures:
- the agentic-orchestration plan discussed from external article research
- what was actually implemented in Terminus from that plan
- the current repo/project state from the latest PR28 worktree docs

Use this as the fast-start context for a fresh Codex session before proposing new work.

## Executive Summary
Terminus already implements many strong agentic/runtime best practices (bounded state machine, approvals, receipts, retries, learning/memory, local-first safety). The main gap vs. orchestrator-style systems is not core execution safety, but:
- supervisor-grade diagnostics/interventions
- mission-level orchestration (parent/child runs, completion contracts)
- context/memory provenance UX

We implemented the first major slice:
- **Supervisor Loop MVP** (run diagnostics + allowlisted interventions + Home "Needs Attention" panel)

Mission orchestration MVP (Iteration 2) is now implemented. Context/memory provenance UX remains the next planned slice.

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

## What Is NOT Implemented Yet (from the plan)
### Mission Orchestration (Workstream B)
- Implemented MVP slice:
  - `missions` / `mission_runs` / `mission_events` tables
  - mission commands (`create_mission_draft`, `start_mission`, `get_mission`, `list_missions`, `run_mission_tick`)
  - mission UI surface (list/detail panel in Home)
  - `daily_brief_multi_source` template with completion contract
- Not implemented yet:
  - additional mission templates (`website_monitor_batch`, `inbox_triage_batch`)
  - broader resource governance/concurrency controls UI

### Context + Memory Provenance (Workstream C)
- No `Context Receipt` read API/UI yet
- No memory provenance/suppression UI yet
- No notification readiness gate config surfaced yet

## Workstream B Status Update (Post-PR30)
Mission Orchestration MVP has now shipped as a follow-on slice:
- backend module `src-tauri/src/missions.rs`
- mission tables in `src-tauri/src/db.rs`
- mission Tauri commands in `src-tauri/src/main.rs`
- mission UI/types in `src/App.tsx` and `src/types.ts`

This makes **Context + Memory Provenance UX (Workstream C / Iteration 3)** the next recommended step.

## Current Project State (from latest PR28 worktree docs)
Source inspected:
- `/Users/vir.c/terminus-pr28/mission_control.md`
- `/Users/vir.c/terminus-pr28/handoff.md`

### `mission_control.md` (PR28 snapshot)
- Last updated: 2026-02-24
- Branch: `codex/pr28-refactor-prep`
- Active work in that snapshot: **PR28 structural refactor/prep (no behavior change)**
- PR28 scope:
  - extract `ConnectionPanel` from `App.tsx`
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

## Important Sequencing Note
The agentic-orchestration plan is strategically valid, but it is **not yet the active top priority** in the latest PR28 mission-control snapshot.

Current documented priorities emphasize:
- Voice object + rule extraction flow
- structural hardening/refactor follow-up
- watcher health UI deepening

This means future sessions should explicitly decide whether to:
- continue current roadmap, or
- pivot to Workstream B (Mission Orchestration), or
- hybridize (e.g., watcher health UX refinement + mission groundwork)

## Recommended Next-Step Options (for a fresh session)
### Option A — Stay on current roadmap (recommended if following PR28 docs)
- Continue Voice object + rule extraction approval flow
- Keep the new diagnostics panel in mind as a future destination for voice/rule failures

### Option B — Hybrid bridge (recommended if introducing agentic plan incrementally)
- Extend diagnostics/supervisor surfaces with watcher-provider backoff/reconnect detail
- Then start Mission Orchestration MVP (`daily_brief_multi_source` only)

### Option C — Full pivot to agentic plan
- Start Workstream B immediately:
  - add `missions`, `mission_runs`, `mission_events`
  - implement one mission template + contract
  - add mission list/detail UI

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
- Latest mission control snapshot (PR28 worktree): `/Users/vir.c/terminus-pr28/mission_control.md`
- Latest handoff snapshot (PR28 worktree): `/Users/vir.c/terminus-pr28/handoff.md`
