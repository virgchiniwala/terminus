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
### PR19 — Completed outcomes + approval canonicalization (compatibility-safe)
Owner: Friday + Fury + Loki
Status: In progress (slice 2)
Scope:
- persist canonical `CreateOutcomeAction` execution records for write-step completions
- make Outcomes home count use primary outcome semantics (run-based, internal artifacts hidden)
- add `list_primary_outcomes` backend query for executed / pending approval / blocked clarification
- keep legacy `*_draft` artifacts as compatibility internals
Acceptance:
- completed write steps create `actions` + `action_executions` (`create_outcome`)
- double approve remains idempotent (single action execution row)
- primary outcomes query excludes internal draft artifacts
Verification:
- `cd src-tauri && cargo test`
- `npm run build`

## Next
1. Full approvals UI cards from typed action payloads (render exact execution intent)
2. Clarification queue UI (single-slot card + answer/resume)
3. Security hardening follow-up (CSP dev/prod split + redaction regression tests)

## Non-goals (MVP)
- arbitrary end-user code execution
- plugin marketplace
- OpenClaw compatibility
- hosted always-on runner
