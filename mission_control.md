# Mission Control — Terminus
Last updated: 2026-02-18

## Current State
- Mode: Day
- Branch: `codex/learning-layer-hardening`
- Product shape: local-first, object-first Personal AI OS

## Strategic Guardrails
- Home remains object-first (`Autopilots / Outcomes / Approvals / Activity`)
- No chat-first or harness-first product drift
- Deny-by-default primitives
- Compose-first outbound behavior
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

## Now
### P0.4 / P0.5 (Foundations) — Email Connections + Watcher Tick
Owner: Friday + Fury
Status: In Progress
Scope:
- Gmail + Microsoft 365 OAuth-ready connection flow
- keychain-only OAuth token storage
- provider connection status surfaced in app
- manual inbox watcher tick command (real fetch + dedupe + run queue)
Acceptance:
- OAuth setup can be saved and connection can be completed with auth code
- no OAuth tokens persisted in SQLite
- watcher tick dedupes by provider message id and enqueues inbox triage runs
Verification:
- `cargo test`
- `npm run build`
- manual: connect provider -> poll inbox -> confirm queued runs in Activity

## Next
1. P0.5 completion: scheduled/background watcher loop + throttling controls + clearer paused/running state
2. P0.6: Safe Effector email send/reply policy gates + typed approval payloads
3. P0.7: provider-backed triage actions (label/archive or folder/category)

## Non-goals (MVP)
- arbitrary end-user code execution
- plugin marketplace
- OpenClaw compatibility
- hosted always-on runner
