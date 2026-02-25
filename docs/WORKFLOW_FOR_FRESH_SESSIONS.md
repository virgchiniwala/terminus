# WORKFLOW_FOR_FRESH_SESSIONS.md
Last updated: 2026-02-25

## Session Start Checklist

1. Read these in order (minimum viable context):
   - `docs/Terminus_CONTEXT.md` — what Terminus is + key strategic directions
   - `docs/TERMINUS_AUDIT_AND_PLAN.md` — comprehensive audit + current P0-P8 priority order
   - `mission_control.md` — what is being worked on right now + test coverage baseline
   - `docs/TERMINUS_PRODUCT_STRATEGY_v3.md` — complete product vision

2. For deeper context, also read:
   - `docs/plan.md` — one-page MVP contract
   - `tasks/TERMINUS_TASKLIST_v3.md` — feature backlog with acceptance criteria
   - `docs/PRINCIPLES_AND_CONSTRAINTS.md` — non-negotiable rules
   - `docs/PRIMITIVES.md` — action primitives catalog (includes Custom recipe + MCP direction)
   - `docs/PLAN_SCHEMA.md` — plan object schema (includes Custom recipe notes)
   - `docs/RUNNER_STATE_MACHINE.md` — execution model
   - `docs/SECURITY_AND_CONTROL.md` — safety model
   - `docs/PROVIDERS_AND_PACKAGING.md` — transport architecture (relay primary)
   - `docs/LEARNING_LAYER.md` — feedback loop design
   - `docs/DIFFERENTIATION.md` — why Terminus exists, what it is not

3. **Restate the binding constraints before proposing work** (see snapshot below).

4. **Check clone-drift risk explicitly:**
   - Chat-first drift? (chat is input, not home)
   - Harness-first drift? (users see objects, not primitive controls)
   - Marketplace drift? (no plugin marketplace)
   - Permission expansion drift? (learning/guidance cannot expand capabilities)
   - Relay-as-middleware drift? (relay routes; doesn't rewrite plan logic)
   - MCP-as-marketplace drift? (MCP servers are primitive providers, not plugins)

5. **Propose 1-3 tasks max with:**
   - Acceptance criteria
   - Verification steps (`cargo test`, `npm test`, `npm run lint`, `npm run build`)
   - Non-goals
   - Explicit mapping to task IDs in `tasks/TERMINUS_TASKLIST_v3.md` where applicable

6. **Execute only approved scope.**

---

## Binding Constraints Snapshot (2026-02-25)

- **Object-first product surface** — Autopilots / Outcomes / Approvals / Activity
- **Shared runtime for all 4 recipes** — WebsiteMonitor, InboxTriage, DailyBrief, Custom
- **Deny-by-default primitives** — PrimitiveGuard enforces allowlist before any execution
- **Compose-first sending policy** — SendEmail always requires approval; 5 gates must all pass
- **Local-first storage and execution** — runner stays local; relay is transport only
- **Provider tiers: Supported vs Experimental** — relay handles provider selection for hosted plans
- **Learning loop is bounded and explainable** — cannot expand capabilities or permissions
- **Relay is primary transport** — BYOK is advanced option, not default
- **Outputs must have real side effects** — draft-only runs are failure cases
- **The agent onboards you** — no pre-configuration required before first result
- **PrimitiveId must remain extensible** — do not close the enum; MCP direction is long-term north star

---

## Current Priority (P0 Active)

Dynamic Plan Generation — Custom Recipe. See `mission_control.md` for full scope and acceptance criteria.

---

## Documentation Hygiene

When any doc is changed:
- Add/update `Last updated: YYYY-MM-DD` tag at top
- Avoid duplicate definitions across files — cross-reference instead
- If docs conflict, `docs/TERMINUS_AUDIT_AND_PLAN.md` has the authoritative priority order
- `docs/PRINCIPLES_AND_CONSTRAINTS.md` has the authoritative rules
- When current task changes, update `mission_control.md` immediately
- When strategic direction changes, update `docs/Terminus_CONTEXT.md` "Key Strategic Directions" section
