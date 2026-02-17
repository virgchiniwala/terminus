# Handoff
Last updated: 2026-02-17

## What Is Shipped
- Object-first desktop shell and home surfaces
- Shared plan schema for Website Monitor, Inbox Triage, Daily Brief
- Deny-by-default primitive guard
- Persisted tick runner with approvals, retries, receipts, and idempotency
- Spend rails enforced in cents with hard-stop behavior before side effects
- Provider/transport abstraction with supported/experimental tiers
- Local secret handling via OS keychain (no secret persistence in repo/db)
- Learning Layer (Evaluate -> Adapt -> Memory) integrated at terminal run boundary

## Current Verification Baseline
- `cd src-tauri && cargo test` passes
- `npm run build` passes

## Operational Truths
- Local-first runtime
- Runs execute while app is open (and additional awake/background behavior is policy-gated)
- Inbox triage ingestion in MVP is paste/forward only
- Compose-first outbound policy remains default

## Immediate Risks to Watch
- Clone drift toward chat-first or harness-first UI
- Scope drift into marketplace/tool-authoring behavior
- Silent capability expansion from adaptation logic
- Contradictory currency policy across docs/UI/runtime naming

## Next Suggested Work
1. Wire learning outcomes into object surfaces with calm, non-technical language.
2. Complete schedule suggestion UX gate (only after first successful run).
3. Add policy tests around outbound quiet hours and recipient/domain allowlists.
