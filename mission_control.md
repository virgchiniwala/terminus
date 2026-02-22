# Mission Control — Terminus
Last updated: 2026-02-22

## Current State
- Mode: Day
- Branch: `codex/pr23-security-infra-hardening`
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
### PR23 — Security/infra follow-up (capabilities + rebinding hardening)
Owner: Friday + Fury + Loki
Status: In progress
Scope:
- add Tauri capability scaffolding for the main window (window-scoped IPC boundary)
- strengthen web fetch against DNS rebinding by pinning resolved IPs into curl (`--resolve`)
- clear expired OAuth session state when refresh token is missing during token access
Acceptance:
- `cargo test` and `npm run build` pass with capability files present
- website and daily brief web-fetch tests still pass after DNS pinning
Verification:
- `cd src-tauri && cargo test`
- `npm run build`

## Next
1. Reliability/cleanup bundle: Gmail batching + watcher rate/backoff policy + DB/query hygiene
2. Structural hardening pass: split `App.tsx`, extract runner/provider/web modules, remove dead state variants
3. Docs/architecture refresh to match shipped learning layer + watcher + background runtime

## Non-goals (MVP)
- arbitrary end-user code execution
- plugin marketplace
- OpenClaw compatibility
- hosted always-on runner
