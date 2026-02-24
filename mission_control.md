# Mission Control — Terminus
Last updated: 2026-02-24

## Current State
- Mode: Day
- Branch: `codex/pr27-frontend-test-foundation`
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
### PR27 — Frontend test foundation (Vitest + RTL)
Owner: Friday + Loki
Status: In progress
Scope:
- add Vitest + React Testing Library test foundation
- extract fragile UI normalization/debounce/status logic into testable helpers
- cover normalization, polling error messaging, debounce guards, and connection health rendering
- add `npm test` to CI
Acceptance:
- frontend test runner passes locally and in CI
- high-risk UI logic has deterministic coverage (normalization + debounce + run-start guard)
- no behavior regressions in build/lint
Verification:
- `npm test`
- `npm run lint`
- `npm run build`

## Next
1. Structural hardening pass: split `App.tsx`, extract runner/provider/web modules, remove dead state variants
2. Voice object + rule extraction approval flow (P0.11/P0.12)
3. Watcher health UI follow-up: provider-level reconnect/backoff details in a dedicated status surface (beyond connection cards)

## Non-goals (MVP)
- arbitrary end-user code execution
- plugin marketplace
- OpenClaw compatibility
- hosted always-on runner
