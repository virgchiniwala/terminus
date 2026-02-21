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
### P0.10 — Scoped Guide command (safe guidance input)
Owner: Friday + Loki
Status: Done (slice 1)
Scope:
- add scoped guidance command for `autopilot|run|approval|outcome`
- classify guidance into `applied`, `proposed_rule`, or `needs_approval`
- block risky capability-expanding instructions from being auto-applied
- persist guidance events in SQLite with bounded instruction length
Acceptance:
- guidance requires explicit scope and scope id
- risky instructions are stored but returned as `needs_approval` with no capability mutation
- recurring phrasing is stored as rule proposal rather than silent mutation
Verification:
- `cargo test`
- `npm run build`
- manual: submit scope+instruction from Guide panel and verify response mode/message

## Next
1. P0.11/P0.12: Voice object + “Make this a rule” approval flow
2. P1.x: provider routing/caching hardening and security tightening
3. UX cleanup for calm language across approvals/outcomes

## Non-goals (MVP)
- arbitrary end-user code execution
- plugin marketplace
- OpenClaw compatibility
- hosted always-on runner
