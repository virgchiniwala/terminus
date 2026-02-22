# Mission Control — Terminus
Last updated: 2026-02-22

## Current State
- Mode: Day
- Branch: `codex/pr21-audit-hardening`
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
### PR21 — Security + Correctness Hardening (audit fixes bundle)
Owner: Fury + Friday + Loki
Status: In progress
Scope:
- fix `read.sources` SSRF allowlist bypass (use plan allowlist, not self-authorized host)
- add private/local network host rejection for web fetches (initial + redirect)
- harden keychain secret writes (no token JSON in process argv)
- fix clarification recipient-answer resume loop by persisting answer into run plan
- gate learning pipeline on `learning_enabled` and suppress learning on clarification-paused runs
- tighten OAuth redirect URI validation to localhost/app-scheme only
- enable SQLite WAL + busy timeout and surface background tick errors
- improve inbox watcher reliability (429 handling, provider selection, received timestamp parsing)
- frontend reliability/a11y fixes (polling stale closure, debounced policy writes, double-submit, modal/focus behavior)
- production CSP tightening with dev override config
Acceptance:
- `cargo test` passes (including local web server tests)
- `npm run build` passes
- daily brief / website monitor web fetch tests still pass after network hardening
- UI polling no longer uses stale retry closure and settings writes are debounced
Verification:
- `cd src-tauri && cargo test`
- `npm run build`

## Next
1. Typed approvals UI cards from executable action payloads (remove residual draft-review language)
2. Outcome surface cleanup: hide compatibility draft artifacts everywhere user-facing
3. Structural hardening pass: split `App.tsx`, extract runner/provider/web modules, remove dead state variants

## Non-goals (MVP)
- arbitrary end-user code execution
- plugin marketplace
- OpenClaw compatibility
- hosted always-on runner
