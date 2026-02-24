# Mission Control — Terminus
Last updated: 2026-02-24

## Fresh Session Note
- For the agentic-orchestration plan, implemented supervisor diagnostics slice, and latest cross-session context summary, read `docs/AGENTIC_BEST_PRACTICES_PLAN_AND_STATUS_2026-02-24.md` first.

## Current State
- Mode: Day
- Branch: `codex/pr28-refactor-prep`
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
### PR28 — Structural refactor / prep (no behavior change)
Owner: Friday
Status: In progress
Scope:
- split large UI surface code in `App.tsx` by extracting `ConnectionPanel`
- extract `main.rs` pure guidance/log-sanitization/retry helpers into `guidance_utils.rs`
- preserve behavior while reducing file size and creating cleaner seams for Voice/Rules + further decomposition
Acceptance:
- `cargo test`, `npm test`, `npm run lint`, `npm run build` all pass
- no behavior change in connections / watcher / guide controls
- `App.tsx` and `main.rs` are smaller with clearer boundaries
Verification:
- `cd src-tauri && cargo test`
- `npm test`
- `npm run lint`
- `npm run build`

## Next
1. Voice object + rule extraction approval flow (P0.11/P0.12)
2. Structural hardening follow-up: split intent overlay / clarification panel and continue `runner.rs` decomposition
3. Watcher health UI follow-up: provider-level reconnect/backoff details in a dedicated status surface (beyond connection cards)

## Non-goals (MVP)
- arbitrary end-user code execution
- plugin marketplace
- OpenClaw compatibility
- hosted always-on runner
