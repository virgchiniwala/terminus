# Terminus: Comprehensive Audit + Execution Plan

**Audit date:** 2026-02-24 | **Updated:** 2026-02-25
**Audit branch:** `claude/angry-burnell` (historical snapshot; use `mission_control.md` + `handoff.md` for current merged state)
**Produced by:** Claude Sonnet 4.6 via full codebase exploration + strategic review session + research synthesis

---

## Quick Context (Read First)

**Terminus** is a local-first macOS desktop app (Tauri + React + Rust + SQLite) that is both a **Personal AI OS** and a **personal agent harness** for non-technical users. It turns a one-line intent into always-on automation that actually acts — not just drafts. The product is object-first (Home = Autopilots / Outcomes / Approvals / Activity). The Intent Bar (⌘K) is an input method; the product outputs are executable objects.

**What "personal agent harness" means:** Terminus provides the same structural guarantees that the best engineering teams (Stripe, OpenAI, Anthropic) build internally for their coding agents — architecture as guardrails (PrimitiveGuard), bounded tool catalog (currently 12 primitives), documented preferences (Voice / Rules / Soul.md), and planning before execution (classify → preview → approve → run). The difference: Terminus brings this to professional knowledge work for non-technical users.

**Target quadrant:** Professional back-office automation — comms coordination, ops-lite, finance-adjacent, legal-adjacent. This is the sparse, high-demand quadrant where automation doesn't yet work for non-technical users. Current presets demonstrate the runtime; professional templates demonstrate the differentiation.

**Tech stack:**
- Frontend: React 19, TypeScript (strict), Vite, Vitest + RTL
- Backend: Tauri 2, Rust, SQLite (WAL mode)
- Providers: OpenAI + Anthropic (supported), Gemini (declared but disabled)
- Secrets: macOS Keychain only — never SQLite, never logs

**Key files:**
- `src-tauri/src/runner.rs` — core tick-based state machine (~236KB)
- `src-tauri/src/db.rs` — SQLite schema + all queries (~55KB)
- `src-tauri/src/main.rs` — all Tauri IPC commands (~36KB)
- `src-tauri/src/missions.rs` — mission orchestration (~33KB)
- `src-tauri/src/transport/mod.rs` — `ExecutionTransport` trait (critical seam)
- `src-tauri/src/transport/local_http.rs` — BYOK transport
- `src-tauri/src/transport/mock.rs` — test transport
- `src/App.tsx` — entire frontend UI (1,253 lines, monolithic)
- `src/uiLogic.ts` — Tauri response normalization (snake_case → camelCase)
- `src/types.ts` — TypeScript types
- `docs/TERMINUS_PRODUCT_STRATEGY_v3.md` — product strategy
- `tasks/TERMINUS_TASKLIST_v3.md` — prioritized feature list
- `ARCHITECTURE.md` — system design reference

**Verification baseline (all must pass before and after any change):**
```bash
cd src-tauri && cargo fmt --check
cd src-tauri && cargo test
npm test
npm run lint
npm run build
```

---

## Part 1: What Works (Do Not Regress)

### Core Runtime — Production-Grade
The tick-based persisted state machine is solid. Key invariants:
- `start_recipe_run()` creates a run and returns immediately (no loop-to-terminal)
- `run_tick(run_id)` advances one bounded step
- State transitions + activity rows written in a single SQLite transaction (atomic)
- Spend rails enforced in integer cents *before* side effects execute
- Idempotency keys throughout: same email never processed twice, retries never double-send
- 73 Rust tests, all passing — cover state machine, retry logic, spend caps, approvals, learning layer, missions

### Safety Model — Actually Implemented, Not Just Documented
- Deny-by-default primitives: capability must be explicitly allowlisted per autopilot
- DNS rebinding protection: `curl --resolve` IP pinning per fetch
- Private IP range rejection (10.*, 172.16-31.*, 192.168.*)
- Redirect re-validation per hop
- Secrets only in macOS Keychain (never SQLite, never logs)
- No raw email/web payloads stored in learning tables
- Safe Effector gate for email send: 5 conditions must all be true before send executes

### Learning Layer — Principled
`Evaluate → Adapt → Memory` pipeline. Hard constraints:
- Cannot expand primitive allowlists via learning
- Cannot add new email recipients via learning
- No raw content stored
- Bounded growth with manual compaction (`compact_learning_data`)

### Object-First Product Model — Architecturally Enforced
No chat threads as first-class objects in schema or runtime. Home surfaces = Autopilots / Outcomes / Approvals / Activity. Intent Bar always resolves to an executable object, never a free-text response. This is enforced at the data model level, not just as a doc rule.

### Mission Orchestration — Partially Shipped
Tables, commands, and 3 tests exist and pass:
- `mission_start_fans_out_child_runs_with_unique_idempotency_keys`
- `contract_blocks_when_child_is_blocked`
- `mission_waits_until_children_terminal_then_aggregates`

Missing: mission terminal-state tests, N>2 children tests, tick boundary tests, UI surface.

---

## Part 2: Technical Debt — What to Fix

### CRITICAL: Frontend Test Coverage is ~10%

**Problem:** `App.tsx` (1,253 lines) has zero tests. `ConnectionPanel.tsx` (392 lines) has zero tests. Only 2 component tests exist (ConnectionHealthSummary). The entire approval flow, Intent Bar, run status surface, and connection panel are tested manually only.

**Why it matters:** The UI is the trust surface for non-technical users. Bugs here destroy confidence in ways backend bugs don't. This is the highest-risk gap before real users.

**What to add:**
1. Approval flow tests: pending approval renders correctly, approve/reject transitions work, typed payload (EmailSendApprovalPayload) renders fields correctly
2. Intent Bar tests: classification result renders, "Make recurring" / "Run once" toggle works, "Run now" CTA is present and callable
3. Run status surface tests: all 11 diagnostic states render correctly, intervention buttons appear for correct states
4. E2E test: intent → draft → start run → outcome visible

**Test coverage baseline (tracked):**
- Backend Rust: 73/73 passing
- Mission tests: 3/3 passing
- Frontend component tests: 2 (ConnectionHealthSummary only)
- Integration tests: 0
- Gaps: App.tsx, ApprovalPanel, IntentBar, RunnerStatus

**Files to modify:** `src/test/` — add new test files per surface. Use existing `vitest` + `@testing-library/react` setup (already configured in `vite.config.ts`).

---

### SIGNIFICANT: App.tsx at 1,253 Lines — Decomposition Required

**Problem:** All surfaces (Autopilots, Outcomes, Approvals, Activity, Missions, Clarifications, Diagnostics, Connection Panel, Runner Status, Intent Bar) share one component with 40+ useState hooks. Adding a new feature risks breaking an unrelated surface.

**Recommended extraction order:**
1. `src/components/ApprovalPanel.tsx` — approval queue + detail view (highest criticality)
2. `src/components/IntentBar.tsx` — ⌘K overlay, input, classification result, CTA
3. `src/components/AutopilotList.tsx` — autopilot list + per-autopilot detail
4. `src/components/ActivityFeed.tsx` — audit timeline
5. `src/components/OutcomeList.tsx` — outcomes surface
6. `src/components/RunnerStatus.tsx` — mode/status line/backlog/watcher state
7. `src/components/DiagnosticsPanel.tsx` — "Needs Attention" supervisor surface

**Rule:** `App.tsx` should be a shell that composes surfaces. No business logic. No direct Tauri `invoke()` calls from `App.tsx` — those belong in the extracted surface components.

---

### MODERATE: Eliminate the snake_case ↔ camelCase Normalization Layer

**Problem:** `uiLogic.ts` manually converts Rust snake_case field names to camelCase for every Tauri command response. Every new field requires three changes: Rust struct field, normalization entry, TypeScript type.

**Fix:** Add `#[serde(rename_all = "camelCase")]` to all Rust structs serialized to Tauri commands. Then delete the normalization functions in `uiLogic.ts`.

**Warning:** Refactor that touches many files. Run full verification after each struct conversion. Do not do all at once.

---

### MODERATE: Mission Orchestration Test Coverage Is Partial

**What's still missing:**
- Mission `succeeded` state only after aggregation summary is persisted
- Mission `failed` when child fails without recovery path
- Mission with N>2 children
- Mission tick boundary: does not advance past `waiting_children` if any child is `needs_approval`

**Pattern to follow:** Existing mission tests use `MockTransport` + `rusqlite::Connection::open_in_memory()`.

---

### MINOR: Gemini BYOK Explicitly Disabled — Remove from UI or Test It

`local_http.rs` line 279-281 returns a non-retryable error for Gemini. Remove from UI provider selection until relay handles it centrally.

---

## Part 3: Strategy — Honest Assessment

### The Core Thesis Is Correct

Non-technical users want automation that actually acts, not more drafts. Trust is the moat. Object-first + deny-by-default safety is the right differentiated position. The current competitors either (a) produce drafts only, or (b) require technical setup that non-technical users can't complete.

### Part 3.5: Harness Engineering Positioning

Based on research synthesis (harness engineering essay, Claude Code Remote Control, Vercel Chat SDK, Mastra Code, Cowork onboarding analysis, production AI agent teams):

Terminus has already built what the best engineering teams describe as a "harness":
- **Architecture as guardrails** → PrimitiveGuard (deny-by-default enforcement)
- **Tools as foundation** → 11 PrimitiveIds (bounded, audited tool catalog)
- **Documentation as system of record** → Soul.md / Voice config (the user's AGENTS.md)
- **Planning before execution** → classify → preview → approve → run (already implemented)

**Positioning:** "Terminus is your AI's operating environment — not an assistant that suggests, but a structured harness that makes AI act safely on your behalf."

The research also shows:
- The relay transport is validated by Claude Code Remote Control's production architecture: local process, outbound-only HTTPS, relay routes messages. This enables **remote approval** (approve from phone/Slack), not just API proxying.
- The Vercel Chat SDK pattern enables a **Slack bot on the relay**: approve pending runs inline in Slack, receive daily briefs, get blocked-run alerts. OpenClaw experience without a native mobile app.
- Interview-driven onboarding (Cowork critique) applies directly: blank canvas → agent introduces itself → interviews you → recommends first autopilot → guides setup inline.
- MCP as primitive source is the long-term north star: `terminus load-mcp box` → BoxRead primitive. Don't close the PrimitiveId type.
- Attended vs unattended trust progression: day-1 approval on everything → day-30 graduated trust → per-autopilot trusted mode.

### The Product Lives in the Wrong Quadrant (Currently)

Current presets (email triage, website monitor, daily brief) are in the **low-autonomy personal** quadrant — the most crowded space. Target is the **professional back-office quadrant** — sparse, high demand:

| Use case | Terminus fit | Safety mechanism |
|----------|-------------|-----------------|
| Comms + coordination: thread → decision + next actions + reply in my voice | High | Approval-gated draft, then sends |
| Follow-up automation: chase if no response in 48h | High | Approval before each send |
| Ops-lite: Friday briefing from sources → one brief → send to team | High (mission model) | Approval before send |
| Personal CRM-lite: relationship state → next touch | Medium | Approval-gated |
| Finance adjacent: parse receipt/invoice → categorize → note | High | Never transacts; prepares only |
| Legal adjacent: summarize contract clause + risks | High | Read-only; no signing |

### Non-Negotiable: Outputs Cannot Be Drafts

If a Terminus run produces text the user has to copy into Gmail, it failed. Every autopilot must have at least one real side effect: a sent email, a filed document, a delivered brief. Draft-only output = failure mode, not success mode.

### Non-Negotiable: Inputs Must Match OpenClaw Ease

Non-technical users must be able to type "handle my inbox" and have the system guide setup inline. Every place a user must make a structured decision before seeing results is an onboarding drop-off. The agent onboards you — no pre-configuration required.

### The BYOK Problem and the Relay Solution

**Problem:** BYOK (bring your own API key) is a hard onboarding blocker for non-technical users and a monetization dead-end.

**Solution:** Terminus sells hosted plans (Free: 50 runs/month, Pro: 500 runs/month). The app calls a Terminus relay that proxies to providers, enforces per-tier rate limits, handles provider selection centrally, and routes remote approvals. BYOK is an "advanced" feature for technical users.

**Relay also enables remote interaction:** pending approvals route to Slack or mobile via relay. Users approve email drafts inline in Slack without opening the Mac app. This is the "OpenClaw on mobile" pattern.

**Why immediately buildable:** The `ExecutionTransport` trait in `src-tauri/src/transport/mod.rs` already has this slot. `RelayTransport` is a drop-in. The relay design must include a push channel (WebSocket/SSE) from day 1 — not just REST dispatch — to support approval routing.

```rust
// src-tauri/src/transport/relay.rs (new file)
pub struct RelayTransport {
    relay_url: String,
    subscriber_token: String, // stored in Keychain
}
```

---

## Part 4: Prioritized Execution Plan

### Priority 0 — Dynamic Plan Generation from Unstructured Input (MERGED)

**Why first:** Turns 3 hardcoded templates into infinite user-described workflows. This IS the planning-before-execution pattern validated by every high-performing AI team. Users describe any professional workflow → LLM generates a valid `AutopilotPlan` using the bounded primitive catalog as vocabulary → user sees and approves the plan before committing → safety invariants enforced server-side regardless of LLM output.

**What to build:**
- New `RecipeKind::Custom` in `schema.rs`
- `generate_custom_plan()` function: sends primitives catalog + intent to LLM, gets plan JSON
- `validate_and_build_custom_plan()` + `validate_custom_execution_plan()`: structural + safety enforcement (SendEmail always requires approval, max 10 steps, no invented primitives)
- Update `classify_recipe()` to detect Custom-eligible intents via professional signal words
- Update `draft_intent()` to call LLM plan generation for Custom recipes
- Update runner `ReadWeb` gate to allow Custom recipe (2-line change)
- Update `start_recipe_run` to accept pre-generated plan via `plan_json` parameter
- Frontend: pass Custom plan JSON in `runDraft()` invocation
- Tests for plan parsing, safety overrides, step count bounds, unknown primitives, and classification regression (plus existing full suite)

**Files to modify:**
- `src-tauri/src/schema.rs` — +5 lines
- `src-tauri/src/main.rs` — +200 lines new, ~30 lines modified
- `src-tauri/src/runner.rs` — ~2 lines (ReadWeb gate)
- `src/App.tsx` — ~5 lines (planJson pass-through)

---

### Priority 1 — Relay Transport + Remote Approval + Slack Bot

**Why first after P0:** Unblocks non-technical users at onboarding AND creates the monetization model AND enables the OpenClaw mobile experience. The relay architecture slot already exists; this is implementation + service work.

**Backend relay service (new service, outside Tauri app):**
- `POST /dispatch` — accepts `{subscriber_token, provider_request}`, returns `ProviderResponse`
- WebSocket/SSE channel — push events to connected clients
- `POST /relay/approve/{run_id}/{step_id}` — resolves pending approval from any surface
- Rate limiting per subscriber_token + tier
- No raw content logged on relay side (privacy)

**Slack bot (via Vercel Chat SDK pattern):**
- Write bot logic once → deploy to Slack (and optionally Teams/Discord)
- Users receive daily brief in Slack
- Users approve pending email drafts inline in Slack with one click
- Blocked-run alerts with link to Mac app for complex interventions
- Dependencies: relay must be built first

**Tauri client changes:**
- `src-tauri/src/transport/relay.rs` — new `RelayTransport` implementing `ExecutionTransport`
- `relay_url` + `subscriber_token` stored in Keychain
- `requires_keychain_key()` returns `false`
- Transport selection: subscriber_token present → RelayTransport; else → LocalHttpTransport
- New Tauri commands: `set_subscriber_token`, `get_subscription_tier`, `remove_subscriber_token`

**UI changes:**
- Onboarding: "Sign in to Terminus" → subscriber token → stored in Keychain
- Settings: plan tier + usage this month
- BYOK: clearly labeled "Advanced" section

**Pricing model:**
- **Free tier:** 50 runs/month, relay-backed, all presets
- **Pro tier:** 500 runs/month, relay-backed, professional templates, Custom recipe
- **Advanced/BYOK:** unlimited via own keys, limited support

**Files to modify:**
- `src-tauri/src/transport/relay.rs` — new file
- `src-tauri/src/transport/mod.rs` — export RelayTransport
- `src-tauri/src/providers/keychain.rs` — subscriber token storage
- `src-tauri/src/main.rs` — subscriber token commands + transport selection
- `src/App.tsx` — onboarding + settings integration

---

### Priority 2 — Interview-Driven Onboarding

**Why second:** Without this, non-technical users hit empty tabs and ⌘K with no guidance. The agent should onboard you — no pre-configuration required before the first run. This is the highest-leverage UX change for user acquisition.

**What to build:**
- `onboarding_state` table (or flag in settings table): tracks completion steps
- First-launch detection: if `onboarding_complete = false` → show interview UI instead of empty tabs
- Onboarding conversation using existing Intent Bar: agent asks role, responsibilities, biggest pain points, goals
- Agent recommends first autopilot based on answers → runs setup inline
- Agent guides provider connection step (OAuth or relay sign-in) within the conversation
- `onboarding_complete = true` after first successful run

**UX flow:**
1. Blank canvas → large "Hello" text
2. After 1 second: input bar appears + "I'm your AI coworker. What do you work on?"
3. User responds in natural language
4. Agent recommends first autopilot
5. Agent guides any needed setup (OAuth, relay sign-in) inline
6. First run starts — user sees result before touching any settings panel

**Files to modify:**
- `src-tauri/src/db.rs` — add `onboarding_state` or `settings` flag
- `src-tauri/src/main.rs` — `get_onboarding_state`, `complete_onboarding` commands
- `src/App.tsx` — first-launch branch renders interview UI instead of object surfaces

---

### Priority 3 — Voice / Soul.md Object (P0.11)

**Why third:** Every output currently sounds generic. Voice is the "coworker feeling" — single highest-leverage user-facing feature not yet shipped. Needs to exist before Slack bot (P1) to ensure bot messages have personality.

**What to build:**
- New DB table: `voices (id, name, tone_preset, length_preset, humor_preset, notes_text, created_at)`
- New DB table: `autopilot_voice_overrides (autopilot_id, voice_id)`
- Rust struct: `VoiceConfig { tone: Tone, length: Length, humor: Humor, notes: Option<String> }`
- Default voice is global; autopilots can override
- Voice injected into: email replies, summaries, daily briefs, approval explanations, system status messages
- Onboarding interview can capture voice preferences naturally

**Acceptance criteria (from tasklist P0.11):**
- `Voice` object with tone (Professional/Neutral/Warm), length (Concise/Normal/Detailed), humor (Off/Light)
- Advanced "Voice Notes" freeform text block (Soul.md equivalent)
- Voice injected into email replies, summaries, briefs, approval explanations
- Global default + optional per-autopilot override

**Files to modify:**
- `src-tauri/src/db.rs` — voices + autopilot_voice_overrides tables
- `src-tauri/src/runner.rs` — inject voice config into LLM prompt construction
- `src-tauri/src/main.rs` — get_voice, set_voice, set_autopilot_voice commands
- `src/App.tsx` (or extracted components) — Voice settings panel
- `src/types.ts` — VoiceConfig type

---

### Priority 4 — Rule Extraction + "Make This a Rule" (P0.12)

**Why fourth:** Closes the learning loop. Without this, every user correction is ephemeral. With it, the system compounds: interaction #100 is better than #1. Pairs with Voice (P3) — rules govern behavior, voice governs style.

**What to build:**
- New DB table: `rules (id, scope [autopilot|global], autopilot_id, body_text, created_at, disabled_at)`
- New DB table: `rule_applications (run_id, rule_id, applied_at)`
- When user provides guidance → system proposes a Rule card: "When X, do Y; otherwise do Z"
- User approves rule creation (treated as Approval object — fits existing approval flow)
- Rules injected into LLM prompt at run start (similar to voice injection)
- "Make this a rule" CTA on Outcome cards and Approval cards

---

### Priority 5 — One Professional-Grade Autopilot Template

**Why fifth:** Demonstrates differentiation, not just runtime. The professional templates show *professional work* can be automated.

**Recommended first template: Comms + Coordination Autopilot**
- Trigger: new email thread with ≥2 back-and-forth messages and unresolved question/action item
- Steps: classify thread → extract key decision/question/who → draft reply in user's voice → optional "chase if no response in 48h"
- Approval: always required for send
- Recipe addition: `RecipeKind::CommsCoordinator` (or delivered as a Custom plan preset)

---

### Priority 6 — Proactive Delivery / Push Notifications

**Why sixth:** Extends the relay (P1). Outcomes currently require opening the app. For a product whose USP is "always working for you," this is a missing layer.

**What to build:**
- macOS system notifications via `tauri-plugin-notification`
- Notification for: new Outcome, Approval required, run failed/blocked
- Natural language notification text using Voice config
- Relay push channel routes these when app is background

---

### Priority 7 — App.tsx Decomposition + Frontend Test Coverage

Before any real user exposure. Approval flow and Intent Bar have zero test coverage.

**Decomposition order:** ApprovalPanel → IntentBar → AutopilotList → ActivityFeed → OutcomeList → RunnerStatus → DiagnosticsPanel

**Tests to add:** ApprovalPanel (typed payload renders, approve/reject), IntentBar (classification + CTA), RunnerStatus (11 diagnostic states), E2E mock (intent → draft → run → outcome)

---

### Priority 8 — Context Receipt (Workstream C MVP)

Surfaces the learning layer to users. "View receipt" on each Outcome card → what voice/rules/memory were used for this run.

**What to build:**
- `get_context_receipt(run_id)` → `{voice_snapshot, rules_applied, memory_cards_recalled, sources, policy_active}`
- `src-tauri/src/main.rs` + `src-tauri/src/db.rs` + UI receipt modal on Outcome cards

---

## Part 5: Deferred / Cut

| Feature | Status | Reason |
|---------|--------|--------|
| `website_monitor_batch` mission template | Defer | Validate mission contract with daily_brief first |
| `inbox_triage_batch` mission template | Defer | Same |
| Multi-provider routing / P1.1 | Keep at P1 | Relay handles provider selection centrally |
| Gemini BYOK | Disabled | Remove from UI until relay enables it |
| Arbitrary code execution | Permanent non-goal | Do not implement |
| Plugin marketplace | Permanent non-goal | Do not implement |
| Hosted always-on runner (cloud execution) | Defer past relay | Relay is transport; cloud execution is separate evolution |
| MCP as primitive source | Long-term direction | Note as architectural north star; keep PrimitiveId extensible |
| Slack bot | Medium-term (P1 dependency) | Requires relay + Voice; then build on Chat SDK pattern |
| Trusted/unattended autopilot mode | Medium-term | Requires Voice + Rules + run history; architecture already supports it |

---

## Part 6: Strategic Non-Negotiables (Do Not Regress)

These apply to any session working on Terminus:

1. **Home is object-first.** Autopilots / Outcomes / Approvals / Activity. No chat threads as first-class objects.
2. **Intent Bar output is always an executable object.** Never a free-text response without a "Run now" CTA.
3. **Deny-by-default primitives.** No action executes without being in the allowlist.
4. **Outputs must have a real side effect.** If a run produces only a draft with no downstream action, it is a failure mode, not a success mode.
5. **Secrets only in Keychain.** Never in SQLite, never in logs, never in receipts.
6. **Approval gate defaults on.** Write/send actions require approval unless explicitly disabled per autopilot.
7. **Learning cannot expand capabilities.** Rules cannot add new recipients, enable new primitives, or bypass approvals.
8. **Idempotency on all effectors.** Same email never processes twice; retries never double-send.
9. **Every state transition is auditable.** Activity row written atomically with state change.
10. **BYOK is an advanced option, not the primary onboarding path.** Relay + hosted plans are the default.
11. **Relay is the primary transport.** BYOK is an advanced escape hatch. Cannot monetize BYOK.
12. **Outputs must have real side effects.** Draft-only runs are failure cases, not success cases.
13. **The agent onboards you.** Setup never requires pre-configured forms before the first result. Every step requiring a form before a result is an onboarding failure.
14. **Harness-first design.** Every new primitive fits within existing safety rails. Bounded tool catalog > unconstrained tool execution. Architecture constrains, it doesn't expand.
15. **Do not close the PrimitiveId type.** The primitives system should remain extensible for future MCP consumption without a major rewrite.
