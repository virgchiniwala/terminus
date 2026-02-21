# Mission Control — Terminus
Last updated: 2026-02-21

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
- Intent Bar may be conversational input, but outputs must always resolve to executable objects

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
### PR2-PR3 Transition — Draft-to-Action canonicalization
Owner: Friday + Fury + Loki
Status: In progress
Scope:
- add canonical `actions` + `action_executions` storage
- link approvals to `action_id` with idempotent approval paths
- add clarification queue primitives (`clarifications`, answer/resume command)
- reframe UI/home copy from draft review to action authorization
- add provider call observability table hooks (`provider_calls`)
Acceptance:
- no core home/approval copy describes drafts as primary user outcome
- action rows persist for approval-gated steps with idempotency key
- approval rows include `action_id`
- clarification answer resumes the same run
- provider calls are logged for LLM steps
Verification:
- `cd src-tauri && cargo test`
- `npm run build`

## Next
1. PR4: Completed outcome semantics on outcome surfaces
2. PR5: Single-slot clarification UX polish
3. PR6/PR7: deterministic provider scaffolding + CSP/redaction hardening

## Non-goals (MVP)
- arbitrary end-user code execution
- plugin marketplace
- OpenClaw compatibility
- hosted always-on runner
