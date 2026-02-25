# FUTURE_EXTENSION.md
Last updated: 2026-02-25

## Purpose
Capture post-MVP extension ideas without polluting current scope. Items here are explicitly NOT in the current P0-P8 execution plan.

---

## Relay Transport (Status: P1 — Active Development)

The relay transport was previously listed here as "future." It is now in **active development (P1)**. See:
- `docs/PROVIDERS_AND_PACKAGING.md` — full relay design requirements
- `docs/TERMINUS_AUDIT_AND_PLAN.md` — P1 relay implementation plan

---

## MCP as Primitive Source (Long-term)

Currently Terminus has 11 hardcoded `PrimitiveId` enum variants. The long-term direction is to make primitives MCP-consumable:

- `terminus load-mcp box` → BoxRead/BoxWrite primitives
- `terminus load-mcp slack` → SlackRead/SlackSend primitives
- `terminus load-mcp calendar` → CalendarRead primitive
- `terminus load-mcp notion` → NotionRead/NotionWrite primitives

This eliminates the need to hardcode integrations: any MCP server becomes Terminus primitives. The `validate_and_build_plan()` server-side validation ensures any MCP-sourced primitive still passes through safety invariants (approval gating, risk tier, spend caps).

**Design implication (NOW):** Do not make `PrimitiveId` a closed/exhaustive Rust enum. Keep it extensible so future MCP tool IDs can be added without breaking existing match arms. This is an active architectural constraint, not just a future idea.

**Why not now:** MCP server discovery, schema validation, and sandboxing require significant infrastructure. Build RelayTransport (P1) first to establish the relay as the primitive-routing layer.

---

## Slack / Teams Bot Integration (Medium-term)

Once relay transport (P1) is built:
- Slack bot using Vercel Chat SDK pattern (write once, deploy to Slack, Teams, Discord)
- Users receive daily brief as Slack message
- Users approve pending email drafts inline in Slack with one-click buttons
- Blocked-run alerts delivered to Slack with link to Mac app for complex interventions
- Approval decision routes back to local runner via relay WebSocket/SSE channel

This is the OpenClaw "message your agent" experience on existing professional platforms — without requiring a native mobile app. The relay's push channel (WebSocket/SSE) is the communication layer.

**Dependencies:** Relay transport with push channel (P1). Voice/Soul.md (P3) for Slack messages to have personality.

---

## Trusted / Unattended Autopilot Mode (Medium-term)

Currently Terminus only supports attended mode: every write/send action requires per-run approval. The trust progression:

- **Day 1 (Attended):** all autopilots require approval on every write/send
- **Day 30+ (Graduated trust):** user has 10+ approved runs with consistent approval → can explicitly enable "trusted mode" per autopilot → approval gates bypass for that autopilot's recurring steps
- **Per-autopilot, not global:** trust is explicit and scoped, not global

**Architecture:** `requires_approval` field already exists per `PlanStep`. Trusted mode = runtime skips approval creation for steps where `requires_approval` was true but user has a proven approval history. A `trusted_mode` flag per autopilot + approval history query would implement this.

**Safety:** trust is per-autopilot and explicit (toggle exposed in autopilot settings). Not automatic. Not global. Requires Voice + Rules (P3/P4) to ensure quality is high enough to trust without human oversight.

**Dependencies:** Voice (P3), Rule Extraction (P4), run history (already exists in activity table).

---

## Power Mode (Optional, Post-P8)

Power Mode is an opt-in advanced lane for technical users who want deeper control:
- Advanced execution diagnostics
- Richer run policy controls (custom spend caps per autopilot, custom quiet hours)
- Multi-user policy layers (team accounts via relay)
- Raw activity log access

**Guardrails for Power Mode:**
- Explicitly opt-in, separated from default onboarding
- Constrained by policy and receipts
- Compatible with object-first UX
- Must not turn Terminus into a developer harness or unconstrained execution environment

---

## Future Ingestion Lane

- Authenticated inbox connectors (Gmail OAuth for always-on watching — this is P0.4 in tasklist, not truly future)
- Richer source connectors (RSS, API polling, webhooks)
- Team-level data governance controls

---

## Future Learning Lane

- Richer quality/noise scoring features
- Safer personalization controls (user sees exactly what the learning layer remembers)
- Explainability dashboards for profile changes (what rules are active, why)
- Context Receipt improvements (Workstream C evolution)

All learning enhancements must preserve bounded adaptation and permission invariants. Learning can never expand capabilities.

---

## What Stays Permanent Non-Goals

These are never-build items, regardless of market pressure:
- Arbitrary shell/code execution for end users
- Plugin marketplace with end-user tool authoring
- OpenClaw compatibility layer (different product)
- Hosted cloud runner (runner stays local; relay is transport only)
