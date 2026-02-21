# Mission Control — Terminus
Last updated: 2026-02-18

## Current State
- Mode: Day
- Branch: `codex/learning-layer-hardening`
- Product shape: local-first, object-first Personal AI OS

## Strategic Guardrails
- Home remains object-first (`Autopilots / Outcomes / Approvals / Activity`)
- No chat-first or harness-first product drift
- Deny-by-default primitives
- Compose-first outbound behavior
- Secrets only in OS keychain
- Shared runtime for all three MVP presets
- Intent Bar may be conversational input, but outputs must always resolve to objects (run draft or autopilot draft)

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

## Now
### P0.1 / P0.2 / P0.3 — Universal Intake Foundations
Owner: Friday + Loki
Status: Done
Scope:
- global Intent Bar entrypoint (`⌘K`) in desktop shell
- intent classification to `one_off_run` vs `draft_autopilot`
- calm Draft Plan Card preview (`will read / will create / approvals / spend / run now`)
Acceptance:
- intent flow resolves to object drafts (no chat-thread end state)
- one-line classifier reason shown and overridable by rerun intent
- primary CTA starts run via existing tick runner path
Verification:
- `cargo test`
- `npm run build`
- manual `⌘K` flow test in app

## Next
1. P0.4: Gmail + M365 OAuth connection (keychain-backed tokens)
2. P0.5: inbox watcher + dedupe + throttling while app open/background
3. P0.6: Safe Effector email send/reply policy gates + typed approval payloads

## Non-goals (MVP)
- arbitrary end-user code execution
- plugin marketplace
- OpenClaw compatibility
- hosted always-on runner
