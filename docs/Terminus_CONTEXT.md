# TERMINUS_CONTEXT.md
Last updated: 2026-02-17

This file is the repo-level orientation for any new agent/session.

## What Terminus Is
A local-first Personal AI OS for non-technical users who want reliable follow-through from everyday intentions.

Core objects:
- Autopilots
- Outcomes
- Approvals
- Activity

Chat is optional input, never the primary product surface.

## Strategic Position
Terminus intentionally avoids clone drift toward:
- chat-first products
- harness-first products
- end-user tool-authoring products

See `docs/DIFFERENTIATION.md`.

## Non-negotiables
- Object-first UX
- Deny-by-default primitive layer
- Reliability as product surface (state, retries, idempotency, receipts)
- Local ownership and portability
- Secrets only in keychain
- Compose-first outbound behavior with strict send gates
- Shared runtime for all presets

See `docs/PRINCIPLES_AND_CONSTRAINTS.md`.

## MVP Presets (Shared Runtime)
1. Website Monitor
2. Inbox Triage (paste/forward only)
3. Daily Brief

All three must run on one plan schema, one primitive set, one runner model.

## Current Runtime Shape
- Tick-based runner: start persists, tick advances bounded, due retries resumed
- Persisted runs/activities/outcomes/approvals
- Spend rails enforced in cents
- Provider/transport seam with local BYOK path
- Learning Layer integrated (Evaluate -> Adapt -> Memory)

## Provider Policy
- Supported: OpenAI, Anthropic
- Experimental: Gemini

## Currency and Cost Policy
- User-facing default currency: SGD
- Runtime rails enforced at integer cents
- Soft rail asks; hard rail blocks before side effects

## Where to Read Next
- `docs/plan.md`
- `docs/PRINCIPLES_AND_CONSTRAINTS.md`
- `docs/PRIMITIVES.md`
- `docs/PLAN_SCHEMA.md`
- `docs/RUNNER_STATE_MACHINE.md`
- `docs/SECURITY_AND_CONTROL.md`
- `docs/PROVIDERS_AND_PACKAGING.md`
- `docs/LEARNING_LAYER.md`
- `docs/UX_SYSTEM.md`
- `docs/WORKFLOW_FOR_FRESH_SESSIONS.md`
