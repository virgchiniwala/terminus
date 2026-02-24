# Mission Control — Terminus
Last updated: 2026-02-24

## Fresh Session Note
- For the agentic-orchestration plan and cross-session context, read `docs/AGENTIC_BEST_PRACTICES_PLAN_AND_STATUS_2026-02-24.md` first.

## Current State
- Mode: Day
- Branch: `codex/mission-orchestration-mvp`
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
### Context + Memory Provenance UX (Legibility Layer) — Iteration 3 MVP
Owner: Loki + Jarvis + Friday
Status: In progress
Scope:
- add read-only `Context Receipt` API per run (sources, memory titles/cards, policy constraints, runtime profile overlays, redaction flags, rationale codes)
- add memory provenance read APIs and safe suppression controls (`suppress/unsuppress`)
- add minimal mission-detail Context Receipt panel (read-only first)
- update agentic best-practices status doc to mark Iteration 2 shipped and Iteration 3 active
Acceptance:
- context receipt is readable for mission child runs without exposing raw source/email/provider payloads
- memory cards list is autopilot-scoped and suppression toggles do not allow freeform edits
- `build_memory_context` excludes suppressed memory cards
- `cargo test`, `npm test`, `npm run lint`, `npm run build` pass
Verification:
- `cd src-tauri && cargo fmt --check`
- `cd src-tauri && cargo test`
- `npm test`
- `npm run lint`
- `npm run build`

## Next
1. Mission outcomes integration: first-class mission receipts/outcomes on object surfaces
2. Notification/readiness gate wiring for mission/context legibility (only after context surfaces stabilize)
3. Voice object + rule extraction approval flow (P0.11/P0.12)

## Non-goals (MVP)
- arbitrary end-user code execution
- plugin marketplace
- OpenClaw compatibility
- hosted always-on runner
