# DIFFERENTIATION.md
Last updated: 2026-02-25

## Why Terminus Exists

Non-technical professionals are surrounded by AI that suggests. Terminus provides AI that acts — via a personal agent harness with architecture-as-guardrails, bounded tools, documented preferences, and plan-before-execute discipline.

This is what the best engineering teams (Stripe Minions, OpenAI internal platform, Anthropic harness research) are building internally for coding agents. Terminus brings it to professional knowledge work for non-technical users — comms coordination, ops-lite, finance/legal-adjacent workflows.

**The target:** professional back-office quadrant, where automation demand is high and supply is nearly zero for non-technical users.

---

## Adaptive but Predictable (Positioning)

Traditional automation is often brittle. It assumes stable inputs and exact mappings, then breaks when the real world drifts.

Opaque agents have the opposite problem: they may adapt, but they are hard to predict and hard to trust.

Terminus is intentionally in the middle:
- **Adaptive:** dynamic plans, learning, rules, webhook triggers, and relay approvals help it handle messy professional workflows
- **Predictable:** bounded primitives, approvals, spend rails, idempotency, and receipts constrain behavior and make it legible

This is the core framing:
- not brittle automation
- not opaque autonomy
- **adaptive but predictable execution**

---

## What We Are Building vs What We Are Not

| Are building | Are NOT building |
|---|---|
| Personal agent harness for professional work | Developer harness / workflow builder |
| Bounded primitives, deny-by-default (PrimitiveGuard) | Unconstrained tool execution |
| Objects (Autopilots/Outcomes/Approvals) | Chat threads as first-class objects |
| Relay + hosted plans (primary transport) | BYOK-primary |
| Interview-driven setup (agent onboards you) | Form-driven configuration |
| Any described workflow (Dynamic Plan Generation) | Exactly 3 hardcoded templates |
| Real side effects (sent email, filed doc) | Draft-only outputs |
| Attended approval on write/send actions | Autonomous unchecked execution |

---

## Terminus vs Chat-First Assistants

Chat assistants optimize for suggestion breadth. Terminus optimizes for reliable execution depth.

- Terminus: object-first operating model (Autopilots, Outcomes, Approvals, Activity)
- Chat-first tools: conversational thread as primary container

One chatbot interaction produces a draft. One Terminus run produces a sent email, a filed document, a delivered brief. Terminus favors persistent intention objects and repeatable execution over prompt-thread management.

---

## Terminus vs OpenClaw-Style Systems

OpenClaw is a coding agent harness with arbitrary tool execution, user-authored skills, and an extension marketplace. Terminus is explicitly NOT this for business automation.

Terminus MVP excludes:
- Arbitrary end-user code execution
- End-user skill/tool authoring
- Extension marketplace behavior
- Harness knobs exposed as main UX

Terminus keeps capabilities constrained and trust-forward. The harness is invisible to users — they see Autopilots and Outcomes, not primitive configurations.

**The borrowing from OpenClaw:** the interaction pattern. Users message their agent in plain text → agent runs continuously → outputs arrive without further prompting. The Intent Bar implements this. The relay enables it on Slack/mobile.

---

## Terminus vs Cloud-Only Agents

Terminus is local-first:
- Local run execution (runner stays on Mac)
- Local vault ownership
- Keychain-based secret handling
- Clear "runs while Mac awake" truth

Relay is a transport (API proxying + remote approval), not a cloud runner. The computation stays local.

---

## Terminus vs "More Templates" Tools

Generic automation tools (Zapier, Make, n8n) require technical setup and often produce brittle, form-defined flows. Terminus's Dynamic Plan Generation lets users describe professional workflows in natural language, then validates the generated plan server-side against a bounded primitive catalog. No workflow builder, no hidden capability growth.

---

## Anti-Clone-Drift Checklist

Do not ship changes that move Terminus toward:
- Chat as primary home screen
- Harness controls as primary product surface
- Unconstrained tool execution (anything not in the bounded primitive catalog)
- Capability growth hidden from users
- Relay-as-middleware drift (relay routes, doesn't rewrite plan logic)
- MCP-as-marketplace drift (MCP servers are primitive providers, not plugins)

---

## Core Differentiators (Updated 2026-02-25)

1. **Shared Autopilot runtime for all recipes** — same state machine, approval model, receipt model, and learning layer for WebsiteMonitor, InboxTriage, DailyBrief, and Custom plans

2. **Dynamic Plan Generation** — users describe any professional workflow in natural language → LLM generates a safe, validated execution plan → user sees and approves before committing. Not 3 templates — any described workflow.

3. **Approval-first write/send boundaries** — `SendEmail` always requires approval; approval gates are enforced server-side and cannot be bypassed by LLM output

4. **Runtime reliability with receipts and recoverable failures** — tick-based persisted state machine, idempotency keys, bounded retries, human-readable failure reasons

5. **Behavioral self-improvement via bounded Learning Layer** — Rules, Voice, memory cards compound over time; interaction #100 is better than #1; learning cannot expand capabilities

6. **Relay + Remote Approval** — pending approvals routed to Slack or mobile via relay; users approve from wherever they work without opening the Mac app

7. **Interview-driven onboarding** — blank canvas → agent interviews you → recommends first autopilot → guides setup inline; no empty tabs on day 1

8. **Calm, premium UX** — object-first, minimal cognitive load, clear always-on truth, no technical jargon

9. **Relay-backed webhook triggers (bounded integration path)** — real inbound integrations now, without jumping to unrestricted external API execution

10. **Bounded `CallApi` primitive (custom integrations without connector sprawl)** — allowlisted outbound HTTP with approval gating and Keychain key refs unlocks CRM/PM/custom APIs while preserving receipts and safety boundaries
