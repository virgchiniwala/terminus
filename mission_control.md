# Mission Control — Terminus
Last updated: 2026-02-22

## Current State
- Mode: Day
- Branch: `codex/pr22-state-clarification-visibility`
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
### PR22 — State correctness + suppression visibility (audit follow-up)
Owner: Friday + Fury + Loki
Status: In progress
Scope:
- add dedicated `needs_clarification` run state (non-terminal) for one-slot clarification pauses
- keep backward compatibility for legacy `blocked + pending clarification` rows in primary outcomes queries
- include `needs_clarification` in backlog and primary outcome surfaces
- expose suppressed Autopilot details (id + name + until time) in Home snapshot and runner banner
Acceptance:
- clarification-paused runs are non-terminal (`needs_clarification`) and no-op until answered
- Home runner banner shows suppressed Autopilot details (not just a count)
- `cargo test` and `npm run build` pass
Verification:
- `cd src-tauri && cargo test`
- `npm run build`

## Next
1. Security/infra hardening follow-up: Tauri capabilities + deeper web fetch anti-rebinding constraints
2. Inbox watcher efficiency/reliability pass: Gmail batching + explicit backoff policy
3. Structural hardening pass: split `App.tsx`, extract runner/provider/web modules, remove dead state variants

## Non-goals (MVP)
- arbitrary end-user code execution
- plugin marketplace
- OpenClaw compatibility
- hosted always-on runner
