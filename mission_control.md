# Mission Control — Terminus
Last updated: 2026-02-22

## Current State
- Mode: Day
- Branch: `codex/p0-safe-effector-send-policy`
- Product shape: local-first, object-first Personal AI OS

## Strategic Guardrails
- Home remains object-first (`Autopilots / Outcomes / Approvals / Activity`)
- No chat-first or harness-first product drift
- Deny-by-default primitives
- Completed outcomes over draft-only workflows
- Compose-first outbound behavior with gated sending
- Secrets only in OS keychain
- Shared runtime for all three MVP presets
- Intent Bar may be conversational input, but outputs must always resolve to objects (run draft or autopilot draft)

## Provider Policy
- Supported: OpenAI, Anthropic
- Experimental: Gemini

## MVP Presets (Shared Runtime)
1. Website Monitor
2. Inbox Triage (moving to real always-on inbox watching)
3. Daily Brief

## Runtime Baseline (Shipped)
- Persisted run state machine with tick execution
- Approval queue with resume/reject paths
- Retry/backoff with due-run resumption
- Spend rails in cents with pre-side-effect hard stops
- Terminal receipts with redaction
- Provider/transport seam + local BYOK lane
- Learning Layer: Evaluate -> Adapt -> Memory
- OAuth provider connections + inbox watcher cadence controls
- Safe send policy gates + typed approval payload columns

## Now
### P0.8 — Background runner window-closed execution
Owner: Friday + Loki
Status: Done (slice 1)
Scope:
- keep app running in background when window closes and background mode is enabled
- add menubar tray controls (`Open Terminus`, `Run Cycle Now`, `Quit`)
- run bounded runner ticks from backend thread while app is alive and background mode is enabled
Acceptance:
- closing main window hides to tray instead of exiting when background mode is on
- background thread executes tick cycle without UI polling dependency
- manual tray action can trigger a cycle immediately
Verification:
- `cargo test`
- `npm run build`
- manual: enable background mode -> close window -> verify tray remains and cycles continue

## Next
1. P0.10+: scoped Guide, Voice object, and rule extraction without chat-first drift
2. P1.x: provider routing/caching hardening and security tightening
3. UX cleanup for runner state surfaces and missed-run messaging

## Non-goals (MVP)
- arbitrary end-user code execution
- plugin marketplace
- OpenClaw compatibility
- hosted always-on runner
