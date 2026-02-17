# Mission Control — Terminus
Last updated: 2026-02-17

## Current State
- Mode: Day
- Branch: `codex/daily-brief-real-preset`
- Product shape: local-first, object-first Personal AI OS

## Strategic Guardrails
- Home remains object-first (`Autopilots / Outcomes / Approvals / Activity`)
- No chat-first or harness-first product drift
- Deny-by-default primitives
- Compose-first outbound behavior
- Secrets only in OS keychain
- Shared runtime for all three MVP presets

## Provider Policy
- Supported: OpenAI, Anthropic
- Experimental: Gemini

## MVP Presets (Shared Runtime)
1. Website Monitor
2. Inbox Triage (paste/forward only)
3. Daily Brief

## Runtime Baseline (Shipped)
- Persisted run state machine with tick execution
- Approval queue with resume/reject paths
- Retry/backoff with due-run resumption
- Spend rails in cents with pre-side-effect hard stops
- Terminal receipts with redaction
- Provider/transport seam + local BYOK lane
- Learning Layer: Evaluate -> Adapt -> Memory

## Now
### T1008 — Documentation Unification
Owner: Loki + Jarvis
Status: In Progress
Scope:
- unify strategy/architecture/constraints docs
- add canonical docs for principles, primitives, schema, learning, differentiation, user stories, and future extension lane
- create docs index for future agents
Acceptance:
- consistent terminology and constraints
- explicit MVP vs future separation
- no contradictory guidance on scope, providers, or runtime behavior
Verification:
- manual consistency pass + grep for stale contradictions

## Next
- T1009: UI wiring for Learning Layer visibility in object surfaces (no jargon)
- T1010: schedule suggestion UX after first successful run

## Non-goals (MVP)
- arbitrary end-user code execution
- plugin marketplace
- OpenClaw compatibility
- hosted always-on runner
- IMAP/OAuth inbox sync
