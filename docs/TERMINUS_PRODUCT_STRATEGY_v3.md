Last updated: 2026-03-02

# Terminus Product Strategy (v3 — Updated)

## 1) What we are building
**Terminus is a Personal AI OS and personal agent harness for non-technical users**: a calm, local-first system that turns a one-line intent into reliable follow-through.

**Core promise:**
- **One-time setup → always-on automation** (while the Mac is awake)
- **It actually does the task** (not just drafts)
- Users can **guide it when it's stuck** in natural language
- Trust comes from **Approvals + Receipts + Spend rails**

**The harness framing:** Terminus provides the same structural guarantees that the best engineering teams (Stripe Minions, OpenAI internal platform, Anthropic) build internally for their coding agents — but for professional knowledge work by non-technical users:
- Architecture as guardrails → PrimitiveGuard (deny-by-default enforcement)
- Bounded tool catalog → 11 audited primitives (not arbitrary tool execution)
- Documented preferences → Voice / Rules / Soul.md (the user's AGENTS.md)
- Planning before execution → classify → preview → approve → run

Positioning: "Terminus is your AI's operating environment — not an assistant that suggests, but a structured harness that makes AI act safely on your behalf."

Positioning shorthand: **adaptive but predictable**.
- Adaptive: dynamic plans, learning, rules, and webhook triggers help with messy real-world workflows.
- Predictable: bounded primitives, approvals, spend rails, and receipts preserve trust and control.

**Not building:**
- A chat app (chat is an input method, not the home screen)
- A developer harness / workflow builder / plugin marketplace
- Arbitrary code execution

---

## 2) Target quadrant: Professional back-office

The current 3 presets (email triage, website monitor, daily brief) are in the **low-autonomy personal** quadrant — the most crowded space in automation products. The target is the **professional back-office quadrant** — sparse demand, high value, where automation doesn't yet work for non-technical users.

| Use case | Terminus fit | Safety |
|----------|-------------|--------|
| Comms + coordination: thread → decision + next actions + reply in my voice | High | Approval-gated send |
| Follow-up automation: chase if no response in 48h | High | Approval before each send |
| Ops-lite: Friday briefing from sources → send to team | High (mission model) | Approval before send |
| Personal CRM-lite: relationship state → next touch | Medium | Approval-gated |
| Finance adjacent: parse receipt/invoice → categorize → note | High | Never transacts, prepares only |
| Legal adjacent: summarize contract clause + flag risks | High | Read-only; no signing |

The current presets **demonstrate the runtime**. Professional templates **demonstrate the differentiation**.

---

## 3) Product model
Terminus has four first-class objects:
- **Autopilot**: a persistent automation (watch + act)
- **Outcome**: the result artifact of a run (email sent, summary delivered, etc.)
- **Approval**: a human gate for side effects or risky changes
- **Activity**: an audit timeline of what happened

Chat is not an object. The conversational layer is **scoped** to objects ("Guide this autopilot/run").

---

## 4) The magic moment (and how we preserve it without chat-first drift)

### Universal Intent Bar (⌘K)
Users get OpenClaw-like magic through an **Intent Bar**:
- Type a one-liner ("Handle my inbox", "Monitor this page", "Send me a daily brief", "Parse this invoice and categorize expenses").
- Terminus outputs the **smallest actionable object**:
  - **One-off Run** (immediate Outcome), or
  - **Draft Autopilot** (Test Run first).

**Key rule:** The end state is never "a chat thread." The end state is always an object with **Run now**.

**Dynamic Plan Generation:** For intents that don't match the 3 preset recipes, the LLM generates a valid execution plan from the user's description using the existing 11 primitives as vocabulary. Users see the generated plan (reads, writes, approvals) in the Draft Plan Card before committing. Safety invariants are enforced server-side regardless of LLM output. This turns 3 templates into any described workflow.

---

## 5) Interview-driven onboarding

**The agent onboards you.** No pre-configuration required before the first run. Every step requiring a form before a result is an onboarding failure.

**First-launch flow:**
1. Blank canvas → large "Hello" text on screen
2. After 1 second: input bar appears + "I'm your AI coworker. What do you work on?"
3. User responds in natural language (voice encouraged)
4. Agent asks: responsibilities, biggest pain points, where AI can provide leverage
5. Agent recommends first autopilot based on answers
6. Agent guides provider setup or relay sign-in inline
7. First run starts — user sees result before touching any settings panel

This is the "Growth Plan" model: the agent customizes itself through conversation, not through users navigating settings panels.

---

## 6) MVP wedge: Always-on inbox automation that *acts*

### Why email is the wedge
Email is the highest-frequency "daily repetitive task" surface for non-technical users in both personal and professional life.

**MVP must do real inbox work:**
- Watch inbox continuously (while Mac awake)
- Triage (label/archive/move)
- Draft replies
- **Send replies** (after approval, within policy)

If MVP doesn't watch the inbox and doesn't send, it collapses into "copy/paste from ChatGPT."

**Input model:** the agent should be able to guide setup. Users should not need to pre-configure OAuth before typing their first intent. Guide setup inline.

---

## 7) Safety strategy: Safe Effectors

Terminus will only be trusted if "doing" is safe.

**Safe Effector definition:** an action primitive that is:
1) Off by default
2) Enabled per-autopilot
3) Requires per-run approval (default)
4) Requires recipient/domain allowlist
5) Enforces max/day + quiet hours
6) Is idempotent (retries never double-send)
7) Produces a receipt with exact payload

This is the non-technical equivalent of "permissions" + "dry run" in dev tooling.

---

## 8) Always-on execution model (local-first truth)

- Terminus runs **locally**.
- Autopilots run only when the Mac is awake + background runner is enabled.

This should be a *product surface*, not a hidden limitation:
- "Paused (Mac asleep)"
- "Running (last tick 12s ago)"
- "3 runs pending when you reopen Terminus"

---

## 9) Relay + Hosted Plans (Primary Packaging)

BYOK (bring your own API key) is a hard onboarding blocker for non-technical users and a monetization dead-end. The primary packaging model is **relay + hosted plans**.

**How it works:**
- User signs up for a Terminus hosted plan → receives subscriber token → stored in Keychain
- App sends ProviderRequests to Terminus relay → relay enforces tier limits, selects provider, returns response
- BYOK is an "Advanced" option for technical users with limited support

**Pricing:**
- **Free:** 50 runs/month, relay-backed, all 3 presets + Custom recipe
- **Pro:** 500 runs/month, relay-backed, professional templates, priority support
- **Advanced/BYOK:** unlimited via own keys, limited support, cannot be monetized

**Remote interaction (the OpenClaw on mobile pattern):**
- The relay also enables remote approval: pending approvals route to Slack or mobile via relay
- Users approve email drafts inline in Slack without opening the Mac app
- Relay includes a push channel (WebSocket/SSE) for approval routing and notifications
- Slack bot built on Vercel Chat SDK pattern: daily brief in Slack, inline approval buttons, blocked-run alerts

**Architecture:** The `ExecutionTransport` trait in `src-tauri/src/transport/mod.rs` already has this slot. `RelayTransport` is a drop-in addition alongside existing `LocalHttpTransport` (BYOK) and `MockTransport` (tests).

---

## 10) Guidance model: freeform + structured outcomes

Users must be able to guide the system for edge cases.

### Where guidance lives
- Inside Autopilot / Approval / Outcome (never as a global chat home).

### How guidance works
- Quick templates for common fixes.
- Freeform "Tell Terminus what to do" input.
- Convert guidance into one of:
  1) One-off override for this run
  2) Proposed reusable Rule (user approves)
  3) Question back to user (NeedsInfo)

---

## 11) Personality strategy: Voice / Soul.md

Personality is a key part of the "coworker" feeling. Without it, every output sounds generic.

### Voice object
- First-class **Voice** object. Default Voice applies globally; Autopilots can override.

### Two layers
- Simple presets/sliders for non-technical users (tone: Professional/Neutral/Warm, length: Concise/Normal/Detailed, humor: Off/Light).
- Advanced "Voice Notes" text block (Soul.md equivalent — freeform personality instructions).

Voice is injected into: email replies, summaries, nudges, approvals, system status messages, and Slack bot messages.

---

## 12) Learning strategy (on by default, compaction-proof)

Learning is enabled by default, including learning from failure.

### Memory failure modes we must prevent
- Not saved → deterministic memory flush at terminal transitions
- Not retrieved → auto-recall before generation/action (no "model decides to search")
- Destroyed by truncation → bounded growth with compaction controls

### Terminus approach
- "Make this a rule" conversion for repeated guidance
- Rules scoped to Autopilot (default) or Global (explicit)
- Rules injected at run start, alongside Voice config

Goal: interaction #100 is better than #1.

---

## 13) Provider strategy (Supported vs Experimental) + routing

### Providers
- Supported: OpenAI, Anthropic (relay handles selection centrally for hosted plans)
- Experimental: Gemini (disabled in BYOK; relay handles when available)

### Routing
- Multi-provider routing: relay selects optimal provider per task class + tier
- Client does not specify provider for relay-backed plans
- Best-effort caching: prompts designed to be stable and cache-friendly

### Spend rails
- Runtime-enforced soft/hard caps in currency
- Graceful degradation: smaller scope → reduced frequency → shorter outputs → cached artifacts

---

## 14) MVP "wow presets" + Custom recipe (shared runtime)

All built on the **same** runner, approval, receipt, and primitive catalog.

1) **Inbox Triage (Always-on)**
   - Watch inbox → classify → triage → draft reply → approval → send

2) **Website Monitor**
   - Watch URLs → low-noise diff → approval → send update

3) **Daily Brief**
   - Aggregate sources → one outcome → (optional approval) → send

4) **Custom (Dynamic Plan Generation)**
   - User describes any professional workflow in natural language
   - LLM generates `AutopilotPlan` using existing 11 primitives as vocabulary
   - Server-side validation enforces safety invariants
   - User sees and approves generated plan before committing

**All four share:** Intent Bar creation, test run first, approvals and receipts, spend rails, Voice + rules, PrimitiveGuard safety.

---

## 15) What "production" means for Terminus

### Reliability
- Idempotency for all effectors
- Persisted state machine + bounded retries
- Human-readable failure reasons
- Deterministic receipts

### Safety
- Deny-by-default primitives (PrimitiveGuard)
- Safe Effector gates for sending/triage
- Secret storage in Keychain (never SQLite, never logs)
- Redacted logs and receipts
- CSP and tightened desktop permissions

### UX trust
- Calm previews ("what will happen")
- Approvals show exact payload
- Clear always-on truth and status
- Interview-driven onboarding (no empty tabs on day 1)

### Operability
- Runner health indicators
- Backoff, throttling, dedupe
- Structured provider call records
- Relay connection status

---

## 16) Near-term roadmap (revised 2026-03-02)

### Phase 0 — Dynamic Plan Generation (CURRENT)
- `RecipeKind::Custom` in schema
- `generate_custom_plan()` + `validate_and_build_plan()` in main.rs
- Runner ReadWeb gate update for Custom
- Frontend planJson pass-through
- 8 new tests

### Phase 1 — Relay + Remote Access
- `RelayTransport` in transport/relay.rs
- Subscriber token in Keychain
- Push channel for approval routing
- Relay-backed webhook triggers (inbound events -> bounded run enqueue)
- Slack bot via Chat SDK pattern
- Onboarding: "Sign in to Terminus" flow

### Phase 2 — Interview-Driven Onboarding
- Blank canvas first-launch experience
- Agent interview flow using Intent Bar
- onboarding_complete flag

### Phase 3 — Voice + Rules
- Voice object (global default + per-autopilot override)
- "Make this a rule" CTA
- Rule injection at run start

### Phase 4 — Professional Templates
- Comms + Coordination autopilot template
- Additional back-office templates

### Phase 5 — Hardening
- App.tsx decomposition + frontend tests
- CSP + permission tightening
- Context Receipt (Workstream C)

---

## 17) Success metrics (MVP)
- **Time-to-wow:** user signs in → first autopilot created → first successful send within 10 minutes.
- **Onboarding completion:** % of users who complete the interview flow without hitting a form.
- **Automation yield:** % of inbound emails processed without manual copy/paste.
- **Safety:** 0 unapproved sends; 0 sends outside allowlist.
- **Trust:** approvals accepted rate; "disable autopilot" rate.
- **Cost:** median cost per run under soft cap; low variance across repeated runs.
- **Custom recipe adoption:** % of users who describe a workflow not in the 3 presets within first week.
