# Terminus MVP Plan (Phase B)

**Mission:** Calm, minimal Personal AI OS for non-technical users.  
**Core object model:** `Autopilots -> Outcomes -> Approvals -> Activity` (chat is input only).  
**Implementation target:** macOS desktop (Tauri + React), Rust local runner, SQLite + local vault, Keychain secrets.

## MVP Scope

### In
- One cohesive Autopilot system with shared creation flow: one sentence -> structured plan object.
- Three "First Autopilots" as recipes on the same primitives/runtime:
  - Website monitor -> summary -> approval -> email draft
  - Inbox triage via Forward-to-Terminus/paste ingestion -> approval queue
  - Daily Brief OS -> aggregate configured sources -> single outcome card
- Shared runtime components for all recipes:
  - Primitive catalog (deny-by-default)
  - Persisted runner state machine + scheduling
  - Approval queue
  - Unified Outcome + Receipt format
  - Email outbox delivery path
- Local vault portability: SQLite + `/vault/` + encrypted export/import + snapshot/restore points.

### Out
- Arbitrary shell/code execution for end users.
- Hosted/always-on runner.
- OpenClaw compatibility/import, plugin marketplace, third-party skill execution.
- Full IMAP/OAuth inbox integration (MVP uses forwarding/paste ingestion).

## Object Model

- **Autopilot:** user goal, schedule policy, allowed primitives, spend/sending constraints, provider preference.
- **Outcome:** canonical result artifact (summary card, draft, brief).
- **Approval:** pending write/send decisions with preview and diff.
- **Activity:** timeline of runs, state changes, cost, and receipts.
- **Receipt:** run facts (what happened, what data was used, cost/time, why failed, next action).

## Primitive Catalog (Constrained "OS syscalls")

- `read.web` (allowlisted domains)
- `read.forwarded_email` (from local ingestion)
- `read.vault_file`
- `write.outcome_draft`
- `write.email_draft`
- `send.email` (gated; off by default)
- `schedule.run`
- `notify.user`

Rules:
- Deny-by-default tool access.
- Read-only actions can auto-run.
- Any write/send action requires explicit approval.
- No primitive for arbitrary command execution.

## Memory/Context Architecture

- **Skill Library:**
  - Versioned local skill specs in vault (recipe behavior and formatting guides).
  - Only allowlisted skills per Autopilot plan.
- **Skill Router:**
  - Selects skills based on plan type (`monitor`, `triage`, `brief`) and risk tier.
- **Compaction:**
  - Summarize prior run context into durable "run memory cards".
  - Keep only decision-critical context per step.
- **Caching:**
  - Cache plan decomposition, source fingerprints, and normalized summaries.
  - Idempotency key per run input fingerprint.
- **Model Routing:**
  - Route by cost/risk profile.
  - Cheaper mode reduces passes and uses cheaper default model path while preserving receipts.

## Provider Strategy

- **Supported** (fully tested + documented + CI): OpenAI, Anthropic.
- **Experimental** (UI-supported, labeled, not guaranteed): Gemini.
- Provider UX remains uniform: connect/validate/select without terminal usage.

## Security Posture

- Deny-by-default permissions with explicit allowlists for domains/recipients.
- Secrets stored in macOS Keychain; never in plaintext config.
- Send policy defaults:
  - Compose-only default.
  - Send allowed only with: per-Autopilot allow-send toggle, per-run approval, recipient/domain allowlist, max sends/day, quiet-hours enforcement.
- Cost guardrails (runtime enforced, UI language in currency only):
  - Currency default USD, user-configurable.
  - Per-run soft/hard: `$0.40 / $0.80`.
  - Daily soft/hard: `$3 / $5`.
- Receipts/audit per run with human-readable failure reasons and recovery steps.

## Reliability Contract

- Persisted state machine: `draft -> ready -> scheduled -> running -> waiting_approval -> retrying -> succeeded|failed|canceled`.
- Bounded retries + idempotent writes.
- "Why it didn't run" surfaced in plain language (sleep/offline, cap exceeded, approval pending, source unavailable).
- Scheduling rule: no schedule until first successful test run.
- Runner truth in UI:
  - Default: runs while app open.
  - Optional background mode: runs while Mac awake with background agent.

## Milestones

### M1: Core Autopilot OS
- Deliverables:
  - Object-based Home (Autopilots, Outcomes, Approvals, Activity).
  - Intent -> structured plan object -> approval -> run.
  - Primitive enforcement + runner state machine + receipts.
  - Cost gates and provider tier labels.
- Acceptance criteria:
  - One Autopilot runs end-to-end twice with persisted history and approvals.
  - Any blocked run shows human-readable reason and next action.

### M2: Three First Autopilots on Shared Runtime
- Deliverables:
  - Recipe presets A/B/C on same planner/runtime primitives.
  - Forward-to-Terminus inbox ingestion.
  - Scheduling suggestions only after successful test run.
- Acceptance criteria:
  - Each preset is created in under 10 minutes by pilot users.
  - Each preset completes two successful runs (scheduled or manual).

### M3: Trust, Recovery, and Pilot Hardening
- Deliverables:
  - Approval bundles + strong diff previews.
  - Snapshot/restore, export/import debug bundle.
  - Background runner mode, run-health surfaces, reliability tuning.
- Acceptance criteria:
  - Pilot users approve >=3 drafts through approval queue.
  - On-time run rate >=90% when app/background mode is active and Mac awake.
  - Failure recovery actions are user-completable without terminal intervention.

## Definition of Done (Pilot)

- User creates all three Autopilots in <30 minutes total.
- Each Autopilot runs successfully twice with clear receipts.
- User approves at least 3 drafts from approval queue (compose-only acceptable).
- Reliability meets >=90% on-time run rate under stated local-run constraints.
- Qualitative retention signal: user reports they would be annoyed if Terminus disappeared.
