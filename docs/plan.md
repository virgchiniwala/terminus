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

## Token Strategy / Routing / Budgets

## Token Budgeting + Guardrail Escalation Router (Backend) / USD Budgets (Frontend)

### Goals
- Backend thinks in **tokens + model-cost curves**; frontend shows **USD** (no token talk by default).
- Maintain profitability via:
  1) cheapest-model-first routing
  2) escalation only on **guardrail failure**
  3) aggressive caching/compaction
  4) hard stops against runaway loops
- Preserve “wow” by avoiding brittle daily caps: **monthly allowance is the true hard cap; daily is a safety rail** with graceful degradation.

### User-facing principles
- Default UI: “This run costs about **$X**” + “Today: **$Y** / Month: **$Z**”
- Advanced toggle (optional): reveals token usage and routing decisions (“Escalated because schema failed”)
- Never show “tokens” unless Advanced is enabled.

---

### A) Token Ledger + Cost Computation (Relay-authoritative for Terminus Plan)

#### 1) Model catalog (versioned)
Maintain a `model_catalog` table in Relay with:
- `provider` (openai|anthropic|gemini)
- `model_id` (string)
- `tier` (0..4) where 0=cheapest, 4=ultimate backup
- `usd_per_m_input`, `usd_per_m_output`
- `usd_per_m_cached_input` (nullable)
- `supports_prompt_caching` (bool)
- `max_output_tokens_default` (int)
- `notes` (ops metadata, not user-facing)

Costs should reference current provider pricing pages and be updatable without client changes.
(OpenAI pricing includes cached input discounts; cached input should be tracked separately when supported.)

#### 2) Usage ledger (authoritative)
Relay persists `usage_events` per request:
- `event_id` (uuid)
- `user_id`
- `autopilot_id` (nullable)
- `run_id` (nullable)
- `step_id` (nullable)
- `provider`, `model_id`, `tier`
- `input_tokens`, `output_tokens` (actual)
- `cached_input_tokens` (0 if not supported/used)
- `cost_usd` (computed from catalog at time of event)
- `created_at`

Additionally persist rollups:
- `daily_spend_usd` (user_id, date, spend)
- `monthly_spend_usd` (user_id, month, spend)

Client may compute *estimates* for UX previews, but Relay is the source of truth.

#### 3) Budget policy primitives
Define a `budget_policy` per user/plan:
- `monthly_allowance_usd` (hard cap unless top-up/upgrade)
- `daily_soft_rail_usd`
- `daily_hard_rail_usd`
- `per_run_soft_usd`
- `per_run_hard_usd`
- `max_concurrent_runs`
- `max_steps_per_run`
- `max_retries_per_step`
- `max_output_tokens_per_step` (default; can be lowered by savings mode)

Key rule:
- **Monthly allowance** is the true hard cap.
- **Daily rails** are used to trigger graceful degradation, not immediate “dead” stoppage (except for non-critical autopilots at hard rail).

---

### B) Guardrail-driven Escalation Router (Cheapest-first, Tier 4 last resort)

#### 1) Router contract
For each “LLM step” in a run:
- Attempt using the cheapest tier that is eligible.
- Validate output against guardrails.
- Escalate tier only if guardrails fail and policy allows.
- Stop escalation at `tier_max` (e.g., tier 4) and return a recoverable failure outcome.

**Do not re-run the whole pipeline on escalation**. Escalate at **step-level**.

#### 2) Tier ladder (conceptual, provider-agnostic)
- Tier 0: tiny/mini (classification, small extraction, trivial summaries)
- Tier 1: fast/general (most transforms)
- Tier 2: strong (messy extraction, heavier synthesis)
- Tier 3: premium (high reliability reasoning)
- Tier 4: best-available ultimate backup (rare, policy-gated)

#### 3) Guardrails (machine-checkable)
Minimum guardrails set (extend per primitive):
- **Schema validation**: JSON output must match schema (when required)
- **Completeness checks**: required fields present, non-empty, valid enums
- **Provenance/citations**: outputs reference source ids for any step requiring evidence
- **No disallowed actions**: output must not request unapproved primitives or leak secrets
- **Budget compliance**: projected/actual tokens must not exceed per-step/per-run caps

Guardrail result object:
- `passed: bool`
- `fail_reasons: [code]` (e.g., SCHEMA_INVALID, MISSING_FIELDS, NO_PROVENANCE, OVER_BUDGET)
- `suggested_fix` (internal hint for retry prompt)

#### 4) Escalation rules
- If guardrail fails due to **format/schema**: allow at most 1 retry at same tier with a stricter “repair” prompt; if still fails, escalate.
- If fails due to **budget**: do NOT escalate first; run graceful degradation ladder (below).
- If fails due to **insufficient evidence**: first attempt to fetch/normalize sources (if allowed), else escalate one tier.
- If fails due to **tool mismatch** (wrong primitive): stop; this is a planner bug, not a model problem.

#### 5) Router pseudocode (step-level)
```text
route_step(step, context, policy):
tier = choose_initial_tier(step, policy.mode) # savings/balanced/quality
while tier <= policy.tier_max:
request = build_request(step, context, tier, policy)
resp = call_model(request)
guard = validate(step.guardrails, resp, context, policy)

if guard.passed:
  record_usage(resp.usage, tier)
  return SUCCESS(resp)

if guard.fail_reason includes OVER_BUDGET:
  degraded = degrade(step, context, policy)
  if degraded.applied:
    continue            # retry same tier with reduced scope/output
  else:
    return FAIL_BUDGET() # cannot degrade further

if guard.is_repairable and step.repair_attempts < 1:
  step.repair_attempts += 1
  context = add_repair_instructions(context, guard)
  continue              # retry same tier once

tier += 1               # escalate
return FAIL_NEEDS_HUMAN(guard.summary)
```

---

### C) Graceful Degradation (Before Escalating Models)

When nearing daily rails or budget-related guardrail failures, apply degradation in order:

1) **Shorten output** (lower max output tokens; stricter “concise” instruction)
2) **Reduce scope** (fewer sources, smaller diff window, fewer items)
3) **Lower frequency** (hourly -> daily; daily -> weekdays)
4) **Use cached artifacts** (reuse extraction; only re-run diff)
5) **Savings Mode**:
   - bias tier selection downward
   - prevent Tier 4 usage unless explicitly approved
6) **Pause non-critical autopilots** at daily hard rail until tomorrow

Degradation must be recorded in receipts (“Ran in Savings Mode; reduced sources from 6->3”).

---

### D) “Cheaper Mode” and Quality Modes (User-facing abstraction)
Expose only 3 modes:
- **Max Savings**: prefers lower tiers; aggressive compaction; stricter output caps
- **Balanced (default)**: cheapest-first with escalation on guardrail failure
- **Best Quality**: higher starting tier for specific steps; still guardrail + budget enforced

Mode influences `choose_initial_tier` and degradation aggressiveness. Never exposes model names by default.

---

### E) Budget Enforcement Behavior (Relayed Plan vs BYOK)
- **Terminus Plan (Relay)**: authoritative enforcement (monthly hard cap, daily rails, per-run caps).
- **BYOK**: client-side estimation + local stops (best-effort). Receipts clearly say “Billed to your provider.”

---

### F) Required instrumentation + tests (MVP quality gates)
1) Unit tests for guardrail validators (schema, provenance, completeness)
2) Integration tests for escalation behavior (Tier 0 fail -> Tier 1 pass, etc.)
3) Budget tests:
   - nearing daily soft rail triggers Savings Mode
   - daily hard rail pauses non-critical autopilots
   - monthly hard cap stops runs unless top-up
4) Cache/compaction tests:
   - cache hit reduces token usage
   - compaction triggers at threshold and preserves required memory items

References / rationale:
- Cached inputs can be materially cheaper; system should track cached vs uncached usage where available.
- “Cheapest model first, escalate only when needed” is a standard routing pattern in production LLM systems.

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
  - MVP note: daily hard rail is a hard stop today; graceful degradation is a planned follow-on.
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
