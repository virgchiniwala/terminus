Last updated: 2026-02-22

# Terminus Product Strategy (Updated)

## 1) What we are building
**Terminus is a Personal AI OS for non-technical users**: a calm, local-first system that turns a one-line intent into reliable follow-through.

**Core promise:**
- **One-time setup → always-on automation** (while the Mac is awake)
- **It actually does the task** (not just drafts)
- Users can **guide it when it’s stuck** in natural language
- Trust comes from **Approvals + Receipts + Spend rails**

**Not building:**
- A chat app (chat is an input method, not the home screen)
- A developer harness UI / workflow builder
- A plugin marketplace
- Arbitrary code execution

---

## 2) Product model
Terminus has four first-class objects:
- **Autopilot**: a persistent automation (watch + act)
- **Outcome**: the result artifact of a run (email sent, summary delivered, etc.)
- **Approval**: a human gate for side effects or risky changes
- **Activity**: an audit timeline of what happened

Chat is not an object. The conversational layer is **scoped** to objects (“Guide this autopilot/run”).

---

## 3) The magic moment (and how we preserve it without chat-first drift)
### Universal Intent Bar (⌘K)
Users get OpenClaw-like magic through an **Intent Bar**:
- Type a one-liner (“Handle my inbox”, “Monitor this page”, “Send me a daily brief”).
- Terminus outputs the **smallest actionable object**:
  - **One-off Run** (immediate Outcome), or
  - **Draft Autopilot** (Test Run first).

**Key rule:** The end state is never “a chat thread.” The end state is always an object with **Run now**.

---

## 4) MVP wedge: Always-on inbox automation that *acts*
### Why email is the wedge
Email is the highest-frequency “daily repetitive task” surface for non-technical users in both personal and professional life.

**MVP must do real inbox work:**
- Watch inbox continuously (while Mac awake)
- Triage (label/archive/move)
- Draft replies
- **Send replies** (after approval, within policy)

If MVP doesn’t watch the inbox and doesn’t send, it collapses into “copy/paste from ChatGPT.”

---

## 5) Safety strategy: Safe Effectors
Terminus will only be trusted if “doing” is safe.

**Safe Effector definition:** an action primitive that is:
1) Off by default
2) Enabled per-autopilot
3) Requires per-run approval (default)
4) Requires recipient/domain allowlist
5) Enforces max/day + quiet hours
6) Is idempotent (retries never double-send)
7) Produces a receipt with exact payload

This is the non-technical equivalent of “permissions” + “dry run” in dev tooling.

---

## 6) Always-on execution model (local-first truth)
- Terminus runs **locally**.
- Autopilots run only when:
  - the **Mac is awake**, and
  - the **background runner** is enabled (menu bar agent) or the app is open.

This should be a *product surface*, not a hidden limitation:
- “Paused (Mac asleep)”
- “Running (last tick 12s ago)”
- “3 runs pending when you reopen Terminus”

---

## 7) Guidance model: freeform + structured outcomes
Users must be able to guide the system for edge cases.

### Where guidance lives
- Inside Autopilot / Approval / Outcome (never as a global chat home).

### How guidance works
- Provide quick templates for common fixes.
- Provide a **freeform “Tell Terminus what to do”** input.
- Convert guidance into one of:
  1) One-off override for this run
  2) Proposed reusable Rule (user approves)
  3) Question back to user (NeedsInfo)

This balances flexibility with safety.

---

## 8) Personality strategy (Soul.md without config sprawl)
Personality is a key part of the “coworker” feeling.

### Voice object
- Introduce a first-class **Voice** object.
- Default Voice applies globally; Autopilots can override.

### Two layers
- Simple presets/sliders for non-technical users.
- Advanced “Voice Notes” text block (Soul.md equivalent).

Voice is injected into all outputs that matter: email replies, summaries, nudges, approvals.

---

## 9) Learning strategy (on by default, compaction-proof)
Learning is enabled by default, including learning from failure.

### Memory failure modes we must prevent
- Not saved
- Not retrieved
- Destroyed by truncation/compaction

### Terminus approach
- **Deterministic memory flush** at terminal transitions.
- **Auto-recall** before generation/action (no “model decides to search”).
- “Make this a rule” conversion for repeated guidance.

Goal: interaction #100 is better than #1.

---

## 10) Provider strategy (Supported vs Experimental) + routing
### Providers
- Supported: OpenAI, Anthropic
- Experimental: Gemini

### Routing
- Multi-provider routing is allowed.
- Best-effort caching: we design prompts to be stable and cache-friendly.
- Do not swap providers/models mid-run unless using a handoff packet.

### Spend rails
- Runtime-enforced soft/hard caps in currency.
- Graceful degradation before escalation:
  - smaller scope
  - reduced frequency
  - shorter outputs
  - cached artifacts

---

## 11) MVP “wow presets” (shared runtime)
All built on the **same** runner, approval, receipt, and primitive catalog.

1) **Inbox Triage (Always-on)**
   - Watch inbox → classify importance → triage → draft reply → approval → send

2) **Website Monitor**
   - Watch URLs → low-noise diff (normalize/structure/semantic) → approval → send update

3) **Daily Brief**
   - Aggregate sources → produce one outcome → (optional approval) → send

**All three share:**
- Intent Bar creation
- Test run first
- Approvals and receipts
- Spend rails
- Voice + rules

---

## 12) What “production” means for Terminus
To move from side project to production, we must have:

### Reliability
- Idempotency for all effectors
- Persisted state machine + bounded retries
- Human-readable failure reasons
- Deterministic receipts

### Safety
- Deny-by-default primitives
- Safe Effector gates for sending/triage
- Secret storage in Keychain
- Redacted logs
- CSP and tightened desktop permissions

### UX trust
- Calm previews (“what will happen”)
- Approvals show exact payload
- Clear always-on truth and status

### Operability
- Runner health indicators
- Backoff, throttling, dedupe
- Structured provider call records

---

## 13) Near-term roadmap (what we build next)
### Phase 1 — Real actions + always-on inbox
- Gmail + M365 connectors (OAuth)
- Inbox watcher + dedupe + throttling
- Send/Reply + triage primitives with Safe Effector gates
- Background runner (menu bar)
- Typed approvals + receipts

### Phase 2 — Guidance + learning
- Voice object + Voice Notes
- Scoped freeform Guide
- Rule extraction + approval
- Deterministic memory flush + auto-recall

### Phase 3 — Hardening
- CSP + permission tightening
- Better retrieval backend (hybrid search) if needed
- Provider telemetry + best-effort caching improvements

---

## 14) Success metrics (MVP)
- **Time-to-wow:** user connects email + creates first autopilot via ⌘K + sees first successful send within 10 minutes.
- **Automation yield:** % of inbound emails processed without manual copy/paste.
- **Safety:** 0 unapproved sends; 0 sends outside allowlist.
- **Trust:** approvals accepted rate; “disable autopilot” rate.
- **Cost:** median cost per run under soft cap; low variance across repeated runs.

