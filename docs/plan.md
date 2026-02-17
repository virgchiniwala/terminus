# Terminus MVP Plan
Last updated: 2026-02-17

## Mission
Terminus is a calm, local-first Personal AI OS for people who want follow-through, not setup overhead.

Terminus is object-based, not chat-based:
- Autopilots
- Outcomes
- Approvals
- Activity

Chat is optional input only.

## Product Identity
Terminus is not:
- chat with plugins
- harness-style tooling for developers
- an end-user tool marketplace

Terminus is:
- intention capture -> repeatable follow-through
- constrained, trustworthy action boundaries
- reliable local execution with clear receipts
- continuous improvement from user behavior

See `docs/DIFFERENTIATION.md` and `docs/PRINCIPLES_AND_CONSTRAINTS.md`.

## MVP Outcomes
MVP ships one shared Autopilot runtime with three presets:
1. Website Monitor: monitor -> summarize change -> approval -> draft email
2. Inbox Triage: paste/forward text -> classify -> draft reply -> approval
3. Daily Brief: read configured sources -> aggregate brief -> approval

Shared across all three:
- one plan schema
- one primitive catalog
- one runner/state model
- one approval model
- one receipt format

## MVP In / Out
In:
- Local-first desktop app (Tauri + React + Rust + SQLite)
- Deny-by-default primitives
- Compose-first outbound behavior
- Runtime spend enforcement in SGD
- Provider tiers (Supported vs Experimental)
- Learning Layer (Evaluate -> Adapt -> Memory)

Out:
- Hosted always-on runner
- IMAP/OAuth inbox sync
- End-user code/shell execution
- End-user custom tool authoring UI
- Marketplace-style extension system
- OpenClaw compatibility/import

## Architecture (MVP)
- UI: React desktop surfaces for objects
- Runtime: Rust tick runner (`start_recipe_run`, `run_tick`, `resume_due_runs`)
- Storage: SQLite + local vault directory
- Secrets: OS keychain only

Reference:
- `docs/RUNNER_STATE_MACHINE.md`
- `docs/PLAN_SCHEMA.md`
- `docs/PRIMITIVES.md`
- `docs/SECURITY_AND_CONTROL.md`
- `docs/LEARNING_LAYER.md`

## Provider Policy
- Supported: OpenAI, Anthropic
- Experimental: Gemini

Supported providers are CI-gated and reliability-scoped. Experimental providers are available but excluded from support guarantees.

See `docs/PROVIDERS_AND_PACKAGING.md`.

## Spend and Sending Policy
Currency policy:
- User-facing default currency: SGD
- Runtime caps enforced as integer cents values
- Internal field names may still use legacy `usd_cents_*` naming while policy is SGD-first

Default rails:
- Daily soft/hard: S$3 / S$5
- Per-run soft/hard: S$0.40 / S$0.80

Behavior:
- Soft rail: explicit confirmation
- Hard rail: block before side effects with recovery options

Outbound:
- Compose-first by default
- Send allowed only with explicit enablement + per-run approval + recipient/domain allowlist + max sends/day + quiet hours

See `docs/SECURITY_AND_CONTROL.md`.

## Reliability Contract
- Persisted run state machine
- Atomic transition + activity writes
- Idempotency keys to prevent duplicate side effects
- Bounded retries for retryable failures only
- Human-readable failure reasons

## Learning Contract
Every terminal run can produce:
- evaluation signals
- bounded adaptation updates
- compact memory cards for future runs

Learning never expands capabilities or permissions.

See `docs/LEARNING_LAYER.md`.

## Definition of Done (Pilot)
- User can configure all three presets in under 30 minutes total
- Each preset completes at least two successful runs
- User approves at least three drafts through approval queue
- Failures provide recoverable guidance
- User reports meaningful dependence on Terminus outcomes
