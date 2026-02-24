# Mission Control — Terminus
Last updated: 2026-02-24

## Current State
- Mode: Day
- Branch: `codex/pr26-watcher-health-ux`
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
### PR26 — Watcher health UX (provider backoff/error visibility)
Owner: Friday + Loki
Status: In progress
Scope:
- surface provider watcher health in Connections UI from `inbox_watcher_state`
- show per-provider backoff timing, recent failure count, and last watcher error (sanitized)
- keep calm copy (no stack traces / harness jargon)
Acceptance:
- user can see why Gmail/Microsoft watcher is delayed or failing without opening logs
- rate-limit/backoff state is legible in the UI
Verification:
- `cd src-tauri && cargo test`
- `npm run build`
- Connect provider, trigger watcher failure/backoff path, refresh Home -> Connections card shows retry timing/failure count

## Next
1. Frontend test foundation (Vitest + RTL) for polling/debounce/normalization/run-start guards
2. Structural hardening pass: split `App.tsx`, extract runner/provider/web modules, remove dead state variants
3. Voice object + rule extraction approval flow (P0.11/P0.12)

## Non-goals (MVP)
- arbitrary end-user code execution
- plugin marketplace
- OpenClaw compatibility
- hosted always-on runner
