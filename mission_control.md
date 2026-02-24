# Mission Control — Terminus
Last updated: 2026-02-22

## Current State
- Mode: Day
- Branch: `codex/pr25-ci-quality-gates`
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
### PR25 — CI + baseline quality gates
Owner: Friday + Fury
Status: In progress
Scope:
- add GitHub Actions CI workflow for PRs/main (macOS runner)
- enforce baseline checks: `cargo fmt --check`, `cargo test`, `npm run lint`, `npm run build`
- add lightweight ESLint config + `npm run lint` script (minimal friction)
Acceptance:
- local and CI checks are aligned and passing
- lint runs without requiring broad frontend refactors
Verification:
- `cd src-tauri && cargo fmt --check`
- `cd src-tauri && cargo test`
- `npm run lint`
- `npm run build`

## Next
1. Watcher health UX: surface provider backoff/error state in UI
2. Frontend test foundation (Vitest + RTL) for polling/debounce/normalization/run-start guards
3. Structural hardening pass: split `App.tsx`, extract runner/provider/web modules, remove dead state variants

## Non-goals (MVP)
- arbitrary end-user code execution
- plugin marketplace
- OpenClaw compatibility
- hosted always-on runner
