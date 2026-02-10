# Personal AI OS — MVP Plan

**Product:** Calm, minimal Personal AI OS for automating work + life. Anyone can describe intent, approve safely, get repeatable outcomes.

**USP:** "Automate anything safely in minutes. Get addicted to the feeling of leverage."

---

## MVP Scope

### In Scope (Core Loop)
- **Onboarding:** Install app → connect one LLM provider → run starter automation → see magic in <3min
- **Intent → Plan:** Describe outcome in plain English → system proposes calm, linear plan (4-8 cards) → user reviews
- **Approve:** User sees data sources, permissions, actions, cost estimate → approve with confidence
- **Run:** Test run immediately → see results + activity log → save as automation
- **Iterate:** "Describe a change" in English → system proposes diff → approve → apply (0-token edits when possible)
- **Habit:** Automations list + manual/scheduled runs + notifications

### In Scope (MVP Features)
- 3 Starter Automations (time-to-first-magic <3min):
  1. Monitor website → summarize changes → post to Slack
  2. Research topic → structured summary → save to file
  3. Inbox triage (Slack) → draft responses → require approval before sending
- Custom automation wizard (time-to-first-custom <15min)
- Safe Mode defaults: read-only + drafts-only for outbound comms
- Per-run token budget (25k soft, 40k hard) + daily spend cap ($10 default)
- Activity log (user-facing, JSON export)
- QMD pipeline: compact at 12-15k tokens, cache distilled artifacts, delta prompting
- macOS desktop app (Tauri) + local runner

### Out of Scope (Phase 2+)
- Multi-user/profiles
- Gmail/Calendar/Notion integrations (start with Slack OR email, not both)
- Import existing OpenClaw workspaces
- Visual workflow editor (drag-drop graph)
- Mobile app
- Cloud execution
- Marketplace/sharing

---

## Key UX Flows

### Flow 1: Onboarding (First Magic <3min)
1. **Welcome:** "Automate anything safely. Let's start with something simple."
2. **Connect LLM:** One-click OAuth or paste API key → stored in keychain → never logged
3. **Pick Starter:** Show 3 cards (website monitor, research, inbox triage) → user taps one
4. **Quick Setup:** One question (e.g., "Which website?" or "What topic?")
5. **Run Test:** "Running your first automation..." → show live activity → results
6. **Save:** "Want to run this daily?" → schedule or save as manual

### Flow 2: Custom Automation (First Custom <15min)
1. **Intent Bar:** "Describe what you want to automate"
2. **Propose Plan:** System shows 4-8 cards: data sources, actions, approvals, cost
3. **Connect Integrations:** Any card needing access shows "Connect Slack" button → OAuth → explain scopes
4. **Review & Approve:** Permissions summary + cost estimate → "Run Test" button
5. **Test Run:** Live activity feed → results → "Looks good?" → Save
6. **Iterate:** "Describe a change" → proposed diff → approve → apply

### Flow 3: Edit Automation (Low/Zero Tokens)
1. **Select Automation:** Tap from list → see plan cards
2. **Edit via NL:** "Also include mentions of 'urgent'" → system proposes:
   - "Update: Add filter 'urgent' to Slack scan"
   - "Cost impact: +~200 tokens/run"
   - "No new permissions needed"
3. **Confirm:** Tap "Apply" → done (0 tokens if direct edit)
4. **Undo:** One-tap undo if needed

---

## Architecture Sketch

### Components
```
┌─────────────────────────────────────┐
│  Tauri Desktop App (macOS-first)   │
│  ├─ React UI (calm, minimal)       │
│  ├─ Intent Bar + Automations List  │
│  ├─ Plan View (cards)              │
│  ├─ Activity Feed                  │
│  └─ Settings (connections, costs)  │
└──────────┬──────────────────────────┘
           │ IPC (Tauri commands)
┌──────────▼──────────────────────────┐
│  Rust Backend (local runner)       │
│  ├─ Workflow Engine                │
│  │   ├─ State machine (queued →   │
│  │   │   running → waiting →       │
│  │   │   retry → success/fail)     │
│  │   ├─ Tool registry              │
│  │   └─ Permission gate            │
│  ├─ QMD Pipeline                    │
│  │   ├─ memory.md (strict budget)  │
│  │   ├─ Delta prompting            │
│  │   └─ Cache (goal, plan, tools)  │
│  ├─ LLM Client (OpenAI OR Anthropic)│
│  ├─ Secrets (macOS Keychain)       │
│  └─ Audit Log (JSON, user-visible) │
└──────────┬──────────────────────────┘
           │
┌──────────▼──────────────────────────┐
│  Local Data (SQLite)                │
│  ├─ automations.db (specs + state) │
│  ├─ activity.db (audit log)        │
│  └─ cache.db (QMD artifacts)       │
└─────────────────────────────────────┘
```

### Data Flow
1. User describes intent → React UI
2. UI sends to Rust backend via Tauri IPC
3. Backend uses QMD to decompose + plan
4. Backend proposes plan → UI renders cards
5. User approves → backend runs with permission gates
6. Results stream to UI via Tauri events
7. State + audit saved to SQLite

---

## Safeguard Model

### Permission Tiers
- **Read-only** (default): web fetch, file read (sandboxed), Slack read
- **Write-safe** (explicit approval): file write (sandboxed), save summaries
- **Write-risky** (explicit approval + preview): Slack post, email send, external API writes

### Per-Integration Toggles
- Slack: read / post
- Email: read / send (drafts vs send)
- Filesystem: read / write (sandboxed workspace only)

### Cost Gates
- Show estimate before first run
- Soft cap: 25k tokens/run (warn, allow override)
- Hard cap: 40k tokens/run (stop, require manual increase)
- Daily cap: $10/day (stop, show alert)

### Prompt Injection Resistance
- External content marked as untrusted
- Separate "instructions" from "data" in prompts
- Tool allowlists: only registered tools callable
- Approval gates on all outbound actions

### Audit
- Every run logged: timestamp, automation, tools called, cost, outcome
- User-visible as "Activity" feed
- JSON export available

---

## Token Strategy (QMD + Caching)

### Memory Budget
- `memory.md`: strict 2k token budget
  - Product constraints (what/why)
  - Current architecture (tool registry, permission model)
  - Decisions made + rationale
  - Known pitfalls
  - Active TODOs
- Rewrite memory.md when it exceeds budget (summarize, keep essence)

### Task Decomposition
- Every automation = sequence of small steps with explicit inputs/outputs
- Each step sends only:
  - Current subtask goal
  - Minimal relevant excerpt (not full logs)
  - Latest memory.md

### Caching Strategy
- **Cache artifacts:**
  - Distilled goal + constraints
  - Proposed plan + tool selection
  - Step graph
  - Integration configs
- **Cache keys:** hash(goal_text + tool_set + schema_version)
- **Reuse:** If goal unchanged, reuse cached plan (0 tokens)
- **Invalidation:** Only when re-plan triggers fire (new tool, core objective change, risk escalation)

### Delta Prompting
- Don't resend full automation spec every step
- Send only: current step goal + diff from previous step
- Use structured summaries: "Step 3 succeeded. Output: [summary]. Next: Step 4."

### Compaction Trigger
- When working prompt >12-15k tokens → compact memory.md
- When plan spec >10k tokens → summarize non-critical details
- Never dump full logs to model (use structured summaries)

### Cheap Mode
- Smaller model (GPT-4o-mini)
- More aggressive QMD (5k token memory budget, 8k compaction trigger)
- Fewer multi-pass checks
- Single-shot planning (no refinement)

---

## Milestones

### M1: MVP Core (Week 1-2)
- [ ] Tauri app scaffold + React UI skeleton
- [ ] Rust backend + workflow engine (state machine, tool registry, permission gate)
- [ ] QMD pipeline (memory.md, delta prompting, caching)
- [ ] One LLM provider (OpenAI or Anthropic)
- [ ] One tool (web fetch + parse)
- [ ] Intent → plan → approve → run flow (end-to-end)
- [ ] Activity log + JSON export

### M2: Integrations + Starters (Week 3)
- [ ] Slack post integration (OAuth, permission model)
- [ ] Sandboxed filesystem (read/write)
- [ ] 3 Starter Automations (website monitor, research, inbox triage)
- [ ] Scheduler (manual + scheduled runs)
- [ ] Cost gates (estimate, soft/hard caps, daily limit)

### M3: Edit + Iterate (Week 4)
- [ ] "Describe a change" NL editing
- [ ] Direct edits (0 tokens): schedule, recipients, filters, etc.
- [ ] Assisted edits (low tokens): add filter, change threshold, etc.
- [ ] Re-plan logic (detect when necessary, cache when possible)
- [ ] Undo (one-tap)

### M4: Polish + Test (Week 5)
- [ ] Design system finalized (typography, spacing, colors)
- [ ] Key screens polished (onboarding, plan view, activity feed)
- [ ] Error states (plain-English, one-click fix paths)
- [ ] Usability test with 5 non-technical users
- [ ] Iteration based on feedback

### Beta: Enterprise Upsell Hooks (Phase 2)
- Multi-user/team workspaces
- Advanced integrations (Gmail, Calendar, Notion)
- Workflow sharing + marketplace
- Cloud execution option
- Advanced audit + compliance features

---

## Acceptance Criteria (Measurable)

### Time-to-First-Magic
- [ ] 80%+ of users complete a starter automation in <3min (no help)

### Time-to-First-Custom
- [ ] 80%+ of users create a custom automation in <15min (no help)

### Trust
- [ ] 100% of runs show: data sources, actions, permissions, cost before execution
- [ ] 100% of risky actions require explicit approval + preview
- [ ] Audit log captures 100% of tool calls + decisions

### Reliability
- [ ] 80%+ of test runs succeed without manual debugging
- [ ] Failures produce actionable next steps (plain English, one-click fix) 90%+ of the time

### Token Efficiency
- [ ] Median run for starter automations: <15k tokens
- [ ] Direct edits: 0 tokens (100%)
- [ ] Assisted edits: <2k tokens (95%+)

### Delight (Qualitative)
- [ ] 5/5 test users describe UI as "calm" or "confidence-inspiring"
- [ ] 0/5 test users report confusion about what will happen or need to see spec/YAML

---

## Next Steps (Phase B Execution)
1. Create remaining artifacts:
   - `ux_principles.md` (design system + visual language)
   - `wireframes.md` (text spec for key screens)
   - `microcopy.md` (UI copy for each interaction)
   - `threat_model.md` (practical risks + mitigations)
   - `token_strategy.md` (detailed QMD pipeline + budgets)
2. Scaffold Tauri app + React UI
3. Build workflow engine (Rust)
4. Implement QMD pipeline
5. Integrate first LLM provider
6. Build first tool (web fetch)
7. End-to-end flow: intent → plan → run
8. Iterate based on plan

**Status:** Plan approved. Ready to execute.
