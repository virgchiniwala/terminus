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
### P0.6 / P0.7 — Provider-backed send + triage execution
Owner: Friday + Loki
Status: Done (slice 1)
Scope:
- replace simulated send with provider-backed execution seam
- add inbox triage execution primitive with typed approval payload
- persist triage/send execution receipts with idempotent outcome writes
Acceptance:
- send executes via connected provider context (Gmail/Microsoft) with policy gates
- triage execution is approval-gated and writes explicit outcome receipt
- tests remain deterministic in mock mode and pass in CI/local
Verification:
- `cargo test`
- `npm run build`
- manual: inbox run -> triage approval -> draft approval -> send approval

## Next
1. P0.8/P0.9: menubar/background runner with missed-run reconciliation
2. P0.10+: scoped Guide, Voice object, and rule extraction without chat-first drift
3. P1.x: provider routing/caching hardening and security tightening

## Non-goals (MVP)
- arbitrary end-user code execution
- plugin marketplace
- OpenClaw compatibility
- hosted always-on runner
