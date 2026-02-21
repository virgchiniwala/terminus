Last updated: 2026-02-22

# Terminus Build Task List (User Stories + Acceptance Criteria)

**Scope:** Update Terminus to deliver the promised “one-liner → always-on follow-through” experience for non-technical users, while staying object-first (**Autopilots / Outcomes / Approvals / Activity**) and deny-by-default on side effects.

**Non‑negotiables (must remain true):**
- Home surface is object-based (not chat threads).
- Deny-by-default primitives (capabilities must be explicitly enabled per Autopilot).
- Reliability surface: persisted run state machine, idempotency, bounded retries, human-readable failure reasons.
- Spend rails in **currency** (SGD defaults) with soft/hard caps.
- Scheduling: manual by default; suggest scheduling only after first successful test run.
- Outbound messaging (email) is *allowed* in MVP **only** behind: per-autopilot enable + per-run approval + allowlists + max/day + quiet hours + idempotency.
- MVP email ingestion is **always-on** (no manual forwarding/paste as the primary path).

---

## P0 — Universal Intake (Intent Bar) for Any Task

### P0.1 Global Intent Bar (⌘K) as universal entry point
**User story:** As a user, I can type a one-liner describing *any* task and Terminus turns it into the smallest actionable object (one-off Run or Autopilot draft) without me filling forms.

**Acceptance criteria:**
- System-wide shortcut (⌘K) opens an Intent Bar overlay.
- Intent Bar accepts freeform text + optional paste (URLs, email text, etc.).
- On submit, Terminus produces **exactly one** of:
  - **One-off Run Draft** (leads to Outcome/Approvals), OR
  - **Draft Autopilot** (leads to Test Run).
- No “chat thread” is created as an end state.
- Intent Bar always lands the user into object screens (Draft Run / Draft Autopilot) with a primary CTA: **Run now**.

### P0.2 One-off vs Autopilot classification
**User story:** As a user, Terminus correctly decides if my request is a one-time task or a recurring automation.

**Acceptance criteria:**
- Heuristic classifier chooses:
  - Autopilot when user indicates recurrence ("every", "daily", "whenever", "monitor", "keep an eye", "always") OR request implies ongoing watch (inbox monitoring, page monitoring).
  - One-off Run otherwise.
- UI shows a secondary action:
  - If one-off: “Make recurring” (only when plausible).
  - If autopilot: “Run once” (for test run).
- Classification decision is shown as a single sentence in the Draft card (“Looks recurring → created a Draft Autopilot”).
- User can override classification with one tap.

### P0.3 Draft Plan Card (calm preview)
**User story:** As a user, I can understand what will happen before I run anything.

**Acceptance criteria:**
- Draft card shows:
  - What it will read
  - What it will write/do
  - What approvals will be required
  - Estimated spend range (currency)
- Draft card includes: **Run now** (primary), **Edit** (secondary).
- “Edit” does not expose harness knobs; it exposes only safe user-level toggles (voice, approvals required, schedule after success, recipients allowlist, etc.).

---

## P0 — Always-on Email Automation (Real Inbox Watching + Real Sending)

### P0.4 Connect Email Provider (OAuth) — Gmail + Microsoft 365
**User story:** As a non-technical user, I can connect my Gmail/Workspace or Microsoft 365 account once and Terminus can monitor and send emails on my behalf.

**Acceptance criteria:**
- Provider connection flow exists for:
  - Google (Gmail/Workspace)
  - Microsoft (Graph / Exchange Online)
- Tokens are stored in **OS Keychain**.
- Connection status is visible in Settings and Autopilot prerequisites.
- If admin policies block scopes, user sees a clear, non-technical error with next steps.

### P0.5 Inbox Watcher (while Mac awake) + dedupe
**User story:** As a user, my inbox triage runs automatically without me forwarding emails.

**Acceptance criteria:**
- Background watcher runs when:
  - App is open OR background runner enabled (menu bar agent) AND Mac is awake.
- Watcher ingests new messages incrementally (Gmail historyId / Graph delta or equivalent).
- Dedupe guarantees:
  - Same email event cannot create duplicate processing runs.
  - Dedupe key is provider_message_id (and thread_id where needed).
- Watcher respects throttles:
  - Max N new emails per polling interval.
  - Backoff on provider errors.
- Every ingest event is receipted in Activity.

### P0.6 Safe Email Send effector (real action)
**User story:** As a user, Terminus can send an email reply/new message automatically *after I approve*, from my connected account.

**Acceptance criteria:**
- Primitive exists: `Email.Send` and `Email.Reply` (provider-backed).
- Sending is blocked unless **all** are satisfied:
  1) Autopilot has “Allow sending” enabled.
  2) Recipient/domain allowlist is satisfied.
  3) Per-run approval is granted.
  4) Within max/day + quiet hours.
  5) Idempotency key prevents duplicate sends on retry.
- Approval UI shows exact payload (To/Cc/Bcc/Subject/Body + thread context).
- On approval, system sends and records:
  - provider message id
  - timestamp
  - idempotency key
- Retries never double-send.

### P0.7 Email triage actions (label/archive)
**User story:** As a user, Terminus can keep my inbox clean automatically.

**Acceptance criteria:**
- Provider-backed primitives exist for minimal triage:
  - Gmail: label + archive
  - M365: folder move + category/flag (choose one minimal path)
- Triage actions require either:
  - Approval (default), OR
  - Autopilot-level “Auto-triage allowed” toggle (explicit).
- All actions are receipted.

---

## P0 — Background Runner (Always-on truth)

### P0.8 Menu bar / background agent
**User story:** As a user, my Autopilots can run when I’m not actively using Terminus, as long as my Mac is awake.

**Acceptance criteria:**
- User can enable “Background runner” (toggle).
- When enabled, runner continues ticking even if main window closed.
- UI clearly communicates when automation is paused (Mac asleep / background off / app closed).
- Runner health is shown (last tick timestamp, backlog size).

### P0.9 Due-run scheduler loop
**User story:** As a user, scheduled and due runs are executed predictably.

**Acceptance criteria:**
- Local scheduler wakes every X seconds to call `resume_due_runs`.
- Backlog is bounded per tick (no unbounded loops).
- Failures transition into states with human-readable reason.

---

## P0 — Guidance & Personality Without Becoming Chat-first

### P0.10 Scoped “Guide” with freeform input
**User story:** As a user, when Terminus is stuck or I want to change behavior, I can tell it what to do in my own words.

**Acceptance criteria:**
- “Guide” is accessible from:
  - Autopilot detail screen
  - Approval card
  - Outcome card
- “Guide” supports:
  - Suggested templates (quick actions)
  - Freeform instructions (text box)
- Guidance is always scoped to an object (Autopilot/Run) and appears in receipts.
- Guidance cannot enable new capabilities silently; it can only:
  - Modify policies within existing permissions
  - Propose enabling new capability (requires explicit user approval & connectors)

### P0.11 Voice (Soul.md equivalent) as first-class object
**User story:** As a user, Terminus has a consistent personality/voice that makes outputs feel like a coworker.

**Acceptance criteria:**
- Introduce `Voice` object (global default + optional per-autopilot override).
- Voice has:
  - Simple controls (Professional/Neutral/Warm, Concise/Normal/Detailed, Humor Off/Light)
  - Advanced “Voice Notes” freeform text.
- Voice is injected into:
  - Email replies
  - Summaries
  - Daily briefs
  - Approval explanations

### P0.12 Rule extraction + “Make this a rule”
**User story:** As a user, I can turn guidance into a reusable rule so the system improves over time.

**Acceptance criteria:**
- When user provides guidance, system proposes a human-readable Rule card:
  - “When X, do Y; otherwise do Z.”
- User can approve rule creation.
- Rules are scoped to:
  - Autopilot (default)
  - Global (optional, explicit)
- Rules are applied automatically on subsequent runs.

---

## P0 — Memory/Learning That Actually Works (and learns from failures)

### P0.13 Learning on by default + deterministic memory flush
**User story:** As a user, Terminus improves over time and does not forget key decisions.

**Acceptance criteria:**
- Learning is enabled by default.
- Before any compaction/truncation or terminalization, system performs a **memory flush** step that saves:
  - decisions
  - preferences
  - rules
  - failure causes + fixes
- Flush output is structured (no fluff) and stored durably.

### P0.14 Auto-recall (no “agent decides to search”)
**User story:** As a user, Terminus uses my past rules and preferences automatically.

**Acceptance criteria:**
- Before generating any plan or message, runtime injects relevant:
  - voice
  - rules
  - past similar-case memory
- Recall is deterministic (top-k retrieval using hybrid search when available).

### P0.15 Learn from failed runs
**User story:** As a user, Terminus becomes more reliable after things go wrong.

**Acceptance criteria:**
- When a run fails or needs user intervention, system records:
  - failure category
  - what was tried
  - user-provided fix/guidance
- Next time, system applies learned fix automatically (within permissions) and indicates it in receipts.

---

## P0 — Approvals/Receipts: “Exactly what will happen”

### P0.16 Typed approval payloads
**User story:** As a user, I see precisely what will be sent/changed before I approve.

**Acceptance criteria:**
- Replace freeform `preview` strings with typed payloads:
  - EmailSendApprovalPayload
  - EmailTriageApprovalPayload
  - WebFetchApprovalPayload (if needed)
  - RuleCreateApprovalPayload
- Approval UI renders fields deterministically.

### P0.17 Receipts as an audit log
**User story:** As a user, I can trust Terminus because I can see what happened and why.

**Acceptance criteria:**
- Every run produces a receipt with:
  - inputs (summarized)
  - actions taken
  - approvals granted
  - spend
  - failures + retries
  - “why it didn’t run” when relevant
- Receipts are queryable per autopilot/run.

---

## P1 — Provider Routing & Best-effort Caching (Practical)

### P1.1 Multi-provider routing policy
**User story:** As a user, Terminus can use different providers while staying predictable.

**Acceptance criteria:**
- Router selects provider/model by task class (Routine/Moderate/Complex) and spend mode.
- System does **not** swap provider/model mid-run unless it uses a handoff packet.
- Handoff packet is stored in artifacts and receipted.

### P1.2 Cache-friendly prompt layout (best effort)
**User story:** As a user, repeated runs are cheaper/faster.

**Acceptance criteria:**
- Introduce canonical prompt template ordering:
  1) Stable policy + tool contracts
  2) Voice + rules summary
  3) Autopilot profile summary
  4) Session/run context
  5) New dynamic inputs
- Tool definitions are stable across a run; do not add/remove tools mid-run.
- Record cache metrics if provider returns them (optional).

---

## P1 — Security Hardening

### P1.3 Secret redaction + log hygiene
**User story:** As a user, my tokens and sensitive data never appear in logs.

**Acceptance criteria:**
- Central redaction layer applied to:
  - stdout/stderr logs
  - activity messages
  - receipts
- Test cases assert secrets never leak.

### P1.4 CSP + Tauri permission tightening
**User story:** As a user, the desktop app is resilient to webview attacks.

**Acceptance criteria:**
- CSP enabled with minimal allowlist.
- Tauri allowlist reduced to required APIs.

---

## P2 — Quality / UX Polish

### P2.1 First-run: “Test before schedule”
**User story:** As a user, I’m not surprised by always-on behavior.

**Acceptance criteria:**
- After creating Draft Autopilot, primary CTA is **Run a test**.
- Only after success does UI suggest enabling always-on scheduling/background.

### P2.2 Calm failure handling
**User story:** As a user, when something fails, I know what to do.

**Acceptance criteria:**
- Failures are categorized (Auth expired, Allowlist blocked, Approval needed, Provider down).
- Each category shows one recommended action.
- “Guide” is available on failures.

---

## Data/Schema Work (Cross-cutting)

### DB: Email ingestion + state
- Tables for:
  - connected_accounts
  - inbox_watch_state (historyId/delta cursor)
  - ingested_messages (dedupe)
  - sent_messages (provider ids + idempotency)

**Acceptance criteria:**
- Migrations added.
- Unique constraints enforce dedupe/idempotency.
- DB queries are indexed for “latest events” and “by autopilot”.

### DB: Voice + Rules
- Tables for:
  - voices
  - autopilot_voice_override
  - rules
  - rule_scopes
  - guidance_events

**Acceptance criteria:**
- Rules are versioned and auditable.
- Rules can be disabled without deletion.

---

## Test Plan (Must ship with P0)

### Reliability tests
- Duplicate email event → only one run created.
- Provider error → retry schedule respected; no double-send.
- Approval race → only one approval applies.

### Safety tests
- Disallowed recipient/domain → send blocked with clear reason.
- Quiet hours/max/day exceeded → blocked with clear reason.

### UX integrity tests
- Intent Bar creates correct object type.
- Receipts contain “what happened” and “why”.

