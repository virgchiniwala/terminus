# Mission Control — Personal AI OS

**Last Updated:** 2026-02-17 15:40 SGT  
**Current Mode:** Day (Vir active)  
**Branch:** codex/daily-brief-real-preset

---

## Workflow Enforcement (Aligned to AGENTS)

- One owner per task by default (`Friday`, `Fury`, `Jarvis`, `Loki`).
- Day Mode: execute max 1-3 tasks at once.
- Every runtime task must include:
  - explicit action boundary
  - idempotency key strategy
  - bounded retry policy (retryable vs non-retryable)
  - atomic state transition + activity receipt
  - user-facing failure reason (no stack traces in UX fields)
- End-user surfaces stay object-first (`Autopilots/Outcomes/Approvals/Activity`), never chat-first.
- Provider guarantee policy:
  - Supported (CI-blocking): OpenAI, Anthropic
  - Experimental (non-CI-blocking): Gemini
  - Future lane: Hosted Terminus Relay transport (not implemented)

---

## Task Card Contract (Required Fields)

For every new task card in `Now`, `Next`, or `Blocked`, include all fields below:

- `Task ID + Title`
- `Owner` (single owner unless explicitly co-owned)
- `Status`
- `Scope` (what is changing)
- `Non-goals` (what must not change)
- `Files`
- `Risks`
- `Acceptance Criteria`
- `Verification`
- `Rollback/Containment` (how to disable or revert safely)

---

## Definition-of-Done Gate

A task is only marked `Shipped` when all are true:

- Tests for happy path and failure path are passing.
- State/persistence changes are verified against SQLite.
- Approval and permission behavior is verified (if applicable).
- Human-readable failure reasons are present for user-visible errors.
- Handoff includes exact reproduction/verification commands.

---

## Day Update (2026-02-12)

### T1007: Learning Layer (Evaluate -> Adapt -> Memory, Local-First)
**Owner:** Jarvis + Fury + Friday  
**Status:** Ready for Review  
**Scope:** Added deterministic learning layer with `decision_events`, idempotent `run_evaluations`, bounded `autopilot_profile` adaptation rules, compact `memory_cards`, runtime profile preflight (including suppression), and receipt enrichment (`evaluation`, `adaptation`, `memory_titles_used`)  
**Files:** `src-tauri/src/learning/mod.rs`, `src-tauri/src/runner.rs`, `src-tauri/src/db.rs`, `src-tauri/src/main.rs`  
**Verification:** `cargo test` (35 tests, pass) + `npm run build` (pass)

### T1007 Guardrails
- No primitive/domain/recipient capability growth from adaptation logic.
- No raw source/email/provider payloads in decision events, evaluations, or memory cards.
- Tick-runner model unchanged (`start_recipe_run` persists and returns; `run_tick` bounded; `resume_due_runs` unchanged).

### T1005: Provider + Transport Abstraction (Relay-ready seam)
**Owner:** Friday  
**Status:** Shipped  
**Scope:** Provider/transport seam, Local BYOK Keychain retrieval, real LocalHttpTransport for OpenAI/Anthropic (env-flagged), and deterministic tests  
**Files:** `src-tauri/src/providers/*`, `src-tauri/src/transport/*`, `src-tauri/src/runner.rs`, `src-tauri/src/db.rs`, `src-tauri/src/main.rs`  
**Verification:** `cargo test` and `npm run build` pass; receipt includes provider tier + spend cents metadata and redaction

### T1006: Billing-Safe Spend Accounting (Cents) + Local BYOK (Real Calls, No UI)
**Owner:** Friday  
**Status:** Shipped  
**Scope:** Replace float USD accounting with integer cents, tighten spend ledger idempotency, and implement real OpenAI/Anthropic execution behind `TERMINUS_TRANSPORT=local_http` (no provider UI)  
**Non-goals:** No Relay backend, no billing UI, no daemon/cron, do not change tick runner model  
**Files:** `src-tauri/src/db.rs`, `src-tauri/src/runner.rs`, `src-tauri/src/transport/local_http.rs`, `README.md`, `docs/plan.md`, `handoff.md`  
**Verification:** `cargo test` (16 tests) + `npm run build` pass; boundary test at exactly $0.80

### T1004: Persisted Runner State Machine + Approval Gate
**Owner:** Friday + Fury  
**Status:** Shipped  
**Scope:** Persisted `runs/activities/approvals/outcomes`, transactional run state transitions, bounded retry, idempotency keys, approve/reject resume flow  
**Files:** `src-tauri/src/db.rs`, `src-tauri/src/runner.rs`, `src-tauri/src/main.rs`  
**Verification:** `cargo test` passes (7 tests) including idempotency, bounded retry, and atomic transition

### T1001: App Shell + SQLite Bootstrap
**Owner:** Friday  
**Status:** Shipped  
**Scope:** Tauri + React + Rust shell with object-based Home and SQLite bootstrap for `Autopilots/Outcomes/Approvals/Activity`  
**Files:** `index.html`, `src/App.tsx`, `src/styles.css`, `src-tauri/src/main.rs`, `src-tauri/src/db.rs`  
**Verification:** `npm run build` passes; Tauri command `get_home_snapshot` reads counts from SQLite

### T1002: Shared Plan Schema (Intent -> Plan Object)
**Owner:** Friday  
**Status:** Shipped  
**Scope:** One schema for all three presets (website monitor, inbox triage, daily brief) with provider/risk metadata and primitive allowlist  
**Files:** `src-tauri/src/schema.rs`, `docs/plan_schema_examples.json`  
**Verification:** `cargo test` includes shared-schema test across A/B/C recipes

### T1003: Runtime Primitive Guard (Deny by Default)
**Owner:** Fury  
**Status:** Shipped  
**Scope:** Guard that blocks non-allowlisted primitives with user-friendly error messaging  
**Files:** `src-tauri/src/primitives.rs`  
**Verification:** `cargo test` includes denied-action test asserting message: \"This action isn't allowed in Terminus yet.\"

---

## Now (In Progress)

### T1007: Learning Layer (Evaluate -> Adapt -> Memory)
**Owner:** Jarvis + Fury + Friday  
**Status:** In Review  
**Scope:** Phase 1-3 learning loop implemented end-to-end and integrated into terminal run hooks  
**Non-goals:** No provider UI, no Relay, no plugin/harness/chat-first surfaces, no arbitrary code execution  
**Files:** `src-tauri/src/learning/mod.rs`, `src-tauri/src/runner.rs`, `src-tauri/src/db.rs`, `src-tauri/src/main.rs`  
**Risks:** Rule tuning may need follow-up based on pilot behavior; current adaptation rules are deterministic and intentionally conservative  
**Acceptance Criteria:** Decision events/evaluation/adaptation/memory are persisted, bounded, explainable, and idempotent  
**Verification:** `cd src-tauri && cargo test`; `npm run build`; DevTools commands in `handoff.md`  
**Rollback/Containment:** Disable profile effects by setting `learning_enabled=0` in `autopilot_profile` for the target autopilot

---

## Next (Queued)

---

## Blocked (Needs Vir)

*Nothing blocked yet.*

---

## Shipped (Done)

### T0012: LLM Provider Registry Design ✓
**Completed:** 2026-02-11 00:35 SGT  
**Owner:** Jarvis (main)  
**Output:** Multi-provider architecture (14.3KB) - common adapter, OpenAI/Anthropic providers, extensibility  
**Branch:** `docs/provider-registry` (merged to main)  
**Commit:** `2b6fd55` - "docs: add LLM provider registry design (multi-provider, extensible)"

### T0005: Threat Model ✓
**Completed:** 2026-02-10 22:22 SGT  
**Owner:** Fury  
**Output:** Security analysis (25KB) - 9 threat categories, 5 high-risk mitigations, permission model validated  
**Branch:** `docs/threat-model` (merged to main)  
**Commit:** `874b481` - "docs: add threat model and security analysis"

### T0004: UI Microcopy ✓
**Completed:** 2026-02-10 22:25 SGT  
**Owner:** Loki  
**Output:** Complete microcopy guide (27KB) - 14 sections, error messages, accessibility labels  
**Branch:** `docs/microcopy` (merged to main)  
**Commit:** `1b138ed` - "docs: add UI microcopy"

### T0003: Wireframes ✓
**Completed:** 2026-02-10 22:21 SGT  
**Owner:** Loki  
**Output:** 6 key screens wireframed (72.6KB) - onboarding, intent→plan, permissions, run view, activity, list  
**Branch:** `docs/wireframes` (merged to main)  
**Commit:** `7d5473b` - "docs: add wireframes for 6 key screens"

### T0007: Tauri App Scaffold ✓
**Completed:** 2026-02-10 22:13 SGT  
**Owner:** Friday  
**Output:** Tauri + React + TypeScript scaffold, working IPC, builds successfully  
**Branch:** `feat/tauri-scaffold` (ready to merge)  
**Run:** `npm run tauri:dev`  
**Commit:** "feat: add Tauri app scaffold with React frontend"

### T0011: Mission Control Dashboard UI ✓
**Completed:** 2026-02-10 22:07 SGT  
**Owner:** Jarvis (main)  
**Output:** Live dashboard at http://localhost:3334, auto-refresh, task tracking  
**Branch:** `feat/mission-control-ui` (ready to merge)  
**Commit:** `bfd200f` - "feat: add Mission Control dashboard UI (internal dev tool)"

### T0006: Token Strategy & QMD Pipeline ✓
**Completed:** 2026-02-10 22:05 SGT  
**Owner:** Jarvis (main)  
**Output:** QMD pipeline documented (6KB) - memory budgets, caching, delta prompting  
**Branch:** `docs/token-strategy` (merged to main)  
**Commit:** `e2afa0e` - "docs: add token strategy and QMD pipeline specification"

### T0002: UX Principles & Design System ✓
**Completed:** 2026-02-10 22:07 SGT  
**Owner:** Loki  
**Output:** Ive-level design system (12.5KB) - typography, colors, components, accessibility  
**Branch:** `docs/ux-principles` (merged to main)  
**Commit:** `4e2f831` - "docs: add UX principles and design system"

### T0001: Repo Skeleton + Mission Control Setup ✓
**Completed:** 2026-02-10 19:10 SGT  
**Owner:** Jarvis (main)  
**Output:** Git repo initialized, Mission Control files created, task breakdown defined  
**Branch:** `main`  
**Commit:** `cef68f3` - "chore: initialize repo skeleton with Mission Control"

### Phase A: Requirements Gathering ✓
**Completed:** 2026-02-10  
**Owner:** Jarvis (main)  
**Output:** Product requirements, UX bar definition, Phase B plan created  
**Commits:** N/A (pre-repo)

---

## Subagent Roster

| Agent ID | Role | Specialty | When to Invoke |
|----------|------|-----------|----------------|
| main | Mission Control, integrator, architect | Coordination, architecture, integration | Always active |
| loki | Content & microcopy | UX writing, error messages, user-facing text | UI copy, documentation, user guides |
| fury | Research & validation | Threat modeling, competitive analysis, QA strategy | Security analysis, user research synthesis |
| friday | Implementation | Code, build systems, testing infrastructure | Heavy lifting on features, refactors, test harness |

*Note: Subagents produce artifacts → main integrates → Vir approves.*

---

## Day Mode vs Night Mode

**Day Mode (Vir active):**
- Propose next 1-3 tasks
- Tight loops with checkpoints
- Vir chooses priorities

**Night Mode (Vir offline):**
- Safe, unblocked, low-risk tasks only (docs, refactors, tests, UI polish, bugfixes)
- No architecture changes or destructive rewrites
- Leave Morning Handoff summary

**Current Mode:** Day

---

## Branching/Commit Conventions

**Branch naming:**
- Feature: `feat/short-description`
- Fix: `fix/short-description`
- Docs: `docs/short-description`
- Chore: `chore/short-description`

**Commit messages:**
- Format: `<type>: <subject>` (50 chars max)
- Types: `feat`, `fix`, `docs`, `chore`, `refactor`, `test`
- Example: `feat: add intent bar UI component`

**Merge strategy:**
- Small, incremental commits
- Never push without Vir approval
- Prepare for GitHub but don't create repo yet

---

## Quick Stats

- **Total Tasks Defined:** 12
- **In Progress:** 3 (T0008, T0009, T0010) - Awaiting Vir approval
- **Queued:** 0
- **Blocked:** 0
- **Shipped:** 10 (Phase A, T0001-T0007, T0011, T0012)

**Night Work Complete:** All design foundation tasks shipped (157KB docs + working Tauri scaffold + Mission Control UI)

---

## Key Decisions (2026-02-10)

**MVP Notification Channel:** Email-first (draft mode by default, explicit opt-in for send)  
**LLM Providers:** Multi-provider by design (OpenAI + Anthropic out of box, extensible registry)  
**Mission Control:** Must be actual dashboard UI, not just markdown  
**Day Mode Path:** Hybrid (T0002 UX + T0006 Token + T0007 Tauri scaffold)
