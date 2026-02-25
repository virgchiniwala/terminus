# TERMINUS_CONTEXT.md
Last updated: 2026-02-25

This file is the repo-level orientation for any new agent/session. Read this before anything else.

## What Terminus Is

A local-first Personal AI OS **and personal agent harness** for non-technical users who want reliable follow-through from everyday intentions.

**Harness framing:** Terminus provides the same structural guarantees that high-performing engineering teams (Stripe Minions, OpenAI internal platform, Anthropic) build for their coding agents — but for professional knowledge work by non-technical users:
- Architecture as guardrails → PrimitiveGuard (deny-by-default)
- Bounded tool catalog → 11 audited primitives
- Documented preferences → Voice / Rules / Soul.md (the user's AGENTS.md)
- Planning before execution → classify → preview → approve → run

Positioning: "Terminus is your AI's operating environment — not an assistant that suggests, but a structured harness that makes AI act safely on your behalf."

**Target quadrant:** Professional back-office automation — comms coordination, ops-lite, finance-adjacent, legal-adjacent. Presets demonstrate the runtime; professional templates demonstrate the differentiation.

**Core objects:**
- Autopilots
- Outcomes
- Approvals
- Activity

Chat is optional input, never the primary product surface.

## Strategic Position

Terminus intentionally avoids clone drift toward:
- chat-first products (chat is an input method, not the home screen)
- harness-first products (no arbitrary tool execution, no end-user workflow builders)
- end-user tool-authoring products (no plugin marketplace)

See `docs/DIFFERENTIATION.md`.

## Non-negotiables
- Object-first UX (Autopilots / Outcomes / Approvals / Activity)
- Deny-by-default primitive layer (PrimitiveGuard)
- Reliability as product surface (state, retries, idempotency, receipts)
- Local ownership and portability
- Secrets only in Keychain (never SQLite, never logs)
- Compose-first outbound behavior with strict send gates
- Shared runtime for all recipes (including Custom)
- Relay is the primary transport; BYOK is advanced-only
- Outputs must have real side effects; draft-only runs are failure cases
- The agent onboards you; no pre-configuration before first result

See `docs/PRINCIPLES_AND_CONSTRAINTS.md`.

## Recipes (Shared Runtime)
1. Website Monitor
2. Inbox Triage (paste/forward and always-on watching)
3. Daily Brief
4. **Custom (Dynamic Plan Generation)** — LLM generates `AutopilotPlan` from natural language using existing 11 primitives as vocabulary; server-side validation enforces safety invariants

All recipes run on one plan schema, one primitive set, one runner model.

## Current Runtime Shape
- Tick-based runner: start persists, tick advances bounded, due retries resumed
- Persisted runs/activities/outcomes/approvals
- Spend rails enforced in integer cents
- Provider/transport seam: RelayTransport (primary, P1 in development), LocalHttpTransport (BYOK/advanced), MockTransport (tests)
- Learning Layer integrated (Evaluate → Adapt → Memory)
- Mission orchestration: tables + commands + 3 tests exist; templates + UI surface pending

## Provider Policy
- Supported: OpenAI, Anthropic (relay handles selection centrally for hosted plans)
- Experimental: Gemini (disabled in BYOK; relay handles when available)
- Primary transport: relay (subscriber_token in Keychain); BYOK is advanced fallback

## Currency and Cost Policy
- User-facing default currency: SGD
- Runtime rails enforced at integer cents
- Soft rail asks; hard rail blocks before side effects

## Key Strategic Directions (2026-02-25)

1. **Harness engineering positioning:** Terminus is the safe execution environment. Every safety feature (PrimitiveGuard, approval gates, spend caps) is a harness component, not just a constraint.

2. **Professional back-office quadrant:** comms coordination, ops-lite, finance/legal-adjacent workflows. Presets demonstrate runtime; professional templates demonstrate differentiation.

3. **Relay as primary transport:** hosted plans via relay are the default onboarding path. Relay also enables remote approval (phone/Slack) not just API proxying. Must include push channel (WebSocket/SSE) from day 1.

4. **Interview-driven onboarding:** blank canvas → agent interview → first autopilot. The agent onboards you. No pre-configuration before first result.

5. **Dynamic Plan Generation (P0, current):** Custom recipe = infinite user-described workflows. LLM generates plans using 11 primitives as vocabulary; validation enforces safety. Turns 3 templates into any described professional workflow.

6. **MCP direction (long-term):** PrimitiveId should eventually map to MCP tool calls. Box MCP, Slack MCP as primitive sources without hardcoding. Keep PrimitiveId extensible.

7. **Attended → unattended trust progression:** day-1 approval on everything → day-30 graduated trust → per-autopilot trusted mode (bypasses approvals for historically-always-approved recurring steps).

## Where to Read Next (in order)
1. `docs/plan.md` — one-page MVP contract
2. `docs/TERMINUS_PRODUCT_STRATEGY_v3.md` — complete vision + roadmap
3. `docs/TERMINUS_AUDIT_AND_PLAN.md` — current audit + P0-P8 priority order
4. `tasks/TERMINUS_TASKLIST_v3.md` — feature backlog with acceptance criteria
5. `docs/PRINCIPLES_AND_CONSTRAINTS.md` — non-negotiable rules
6. `docs/PRIMITIVES.md` — action primitives catalog
7. `docs/PLAN_SCHEMA.md` — plan object schema (includes Custom recipe)
8. `docs/RUNNER_STATE_MACHINE.md` — execution model
9. `docs/SECURITY_AND_CONTROL.md` — safety model
10. `docs/PROVIDERS_AND_PACKAGING.md` — transport architecture (relay primary)
11. `docs/LEARNING_LAYER.md` — feedback loop design
12. `docs/DIFFERENTIATION.md` — why Terminus exists + what it is not
13. `mission_control.md` — current session state + now/next
14. `docs/WORKFLOW_FOR_FRESH_SESSIONS.md` — session checklist
