# Mission Control — Terminus
Last updated: 2026-02-22

## Current State
- Mode: Day
- Branch: `codex/pr24-reliability-cleanup-bundle`
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
### PR24 — Reliability/cleanup bundle (watchers + query hygiene + architecture refresh)
Owner: Friday + Fury + Loki
Status: In progress
Scope:
- add Gmail batch details fetch (reduce list+detail N+1 pattern) with safe sequential fallback
- add provider-level inbox watcher backoff state for rate-limit/retryable failures
- prevent one provider watcher failure from aborting the whole watcher cycle
- optimize primary outcomes query shape (limit recent runs first via CTE) and add subquery indexes
- remove dead `RunState::Draft` variant and refresh `ARCHITECTURE.md`
Acceptance:
- `cargo test` and `npm run build` pass
- watcher batching/backoff tests pass
- primary outcomes query behavior remains compatible
Verification:
- `cd src-tauri && cargo test`
- `npm run build`

## Next
1. Structural hardening pass: split `App.tsx`, extract runner/provider/web modules, move `main.rs` business logic
2. DB compatibility cleanup: legacy float spend columns/timestamp normalization/cascade strategy
3. Fine-grained Tauri app command ACLs (per-command permissions beyond window boundary)

## Non-goals (MVP)
- arbitrary end-user code execution
- plugin marketplace
- OpenClaw compatibility
- hosted always-on runner
