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
### Teach Once (Rule Cards) — MVP Slice
Owner: Loki + Jarvis + Friday + Fury
Status: In progress
Scope:
- add `rule_cards` + `rule_match_events` persistence and indexes
- add deterministic rule proposal flow from Guide (`submit_guidance` + `propose_rule_from_guidance`)
- add rule approval/enable/disable commands
- apply bounded rule overlays at run preflight (`noise_suppression`, `daily_brief_scope`, `reply_style`)
- add rule provenance to receipts + Context Receipt
- add minimal Rule Cards panel + approve/reject proposal UI in Connections/Guide surface
Acceptance:
- rule proposals are bounded, approval-gated, and cannot expand protected capabilities
- active rules change future run behavior only via allowed overlays (no new primitives/capabilities)
- receipts/context show applied rule titles/effect summaries
- `cargo test`, `npm test`, `npm run lint`, `npm run build` pass
Verification:
- `cd src-tauri && cargo fmt --check`
- `cd src-tauri && cargo test`
- `npm test`
- `npm run lint`
- `npm run build`

## Next
1. Behavior-suggested Rule Cards from repeated signals (approval-gated)
2. Mission outcomes integration: first-class mission receipts/outcomes on object surfaces
3. Voice object + rule extraction approval flow (P0.11/P0.12) on top of Rule Cards

## Non-goals (MVP)
- arbitrary end-user code execution
- plugin marketplace
- OpenClaw compatibility
- hosted always-on runner
