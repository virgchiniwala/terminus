# Terminus

**Automate anything safely in minutes.**

---

## What This Is

A calm, minimal Personal AI OS for work + life. Describe intent → see what will happen → approve safely → get repeatable outcomes.

**USP:** Anyone can automate anything without touching a terminal. Ive-level UX. Strong safeguards. Low token costs.

---

## Status

**Phase:** MVP Development  
**Platform:** macOS desktop (Tauri)  
**Runner:** Local execution (tick-based, persisted state machine)

See `mission_control.md` for current task status.

## Current Capabilities (Implemented)

- Object-first Home: Autopilots, Outcomes, Approvals, Activity (no chat-first UI).
- Shared Autopilot plan schema for 3 presets (website monitor, inbox triage via paste/forward, daily brief).
- Runner:
  - persisted state machine in SQLite
  - bounded retries with `next_retry_at_ms`
  - tick-style execution (`start_recipe_run` persists and returns, `run_tick` advances bounded, `resume_due_runs` resumes due retries)
  - approvals gate any write/send steps
- Primitives are deny-by-default (guarded at runtime).
- Spend caps + receipts:
  - runtime-enforced caps (USD, shown as currency; no token jargon in UX fields)
  - spend ledger is billing-safe using integer cents (no float money accounting)
  - every terminal run writes a redacted receipt
- Providers/transport:
  - provider abstraction (OpenAI/Anthropic Supported, Gemini Experimental)
  - Local BYOK keys read from macOS Keychain
  - transports:
    - default: Mock (deterministic tests)
    - optional: Local HTTP (real OpenAI + Anthropic) via env flag

## Local BYOK (No UI)

### Keychain setup
Keys are stored in macOS Keychain only (never in SQLite).

Service names:
- OpenAI: `terminus.openai.api_key`
- Anthropic: `terminus.anthropic.api_key`

Example commands:
```bash
security add-generic-password -a Terminus -s terminus.openai.api_key -w "$OPENAI_API_KEY" -U
security add-generic-password -a Terminus -s terminus.anthropic.api_key -w "$ANTHROPIC_API_KEY" -U
```

### Running with real provider calls
Set:
- `TERMINUS_TRANSPORT=local_http`

Otherwise, the app uses Mock transport by default.

---

## Architecture

- **Frontend:** React (calm, minimal UI)
- **Backend:** Rust (local runner, QMD pipeline, workflow engine)
- **Data:** SQLite (automations, activity log, cache)
- **Secrets:** macOS Keychain

---

## Development

**Mission Control:** `mission_control.md`  
**Handoff Log:** `handoff.md`  
**Tasks:** `tasks/`  
**Docs:** `docs/`  

---

## Key Principles

1. **Time-to-first-magic:** <3 minutes
2. **Time-to-first-custom:** <15 minutes
3. **Trust:** Always show what data is used, what actions will happen, stop/undo safely
4. **Delight:** Quiet, confidence-inspiring UI (not a dev tool with a skin)

---

## Workflow

**Day Mode:** Propose 1-3 tasks → prioritizes → execute with checkpoints  
**Night Mode:** Safe, unblocked tasks only (docs, refactors, tests, polish)

---
