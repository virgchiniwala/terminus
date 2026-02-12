# Terminus Agent Workflow

## Why This Exists
Terminus is a trust product. Agent behavior must be constrained, deterministic, and auditable.

## Operating Model
- Day Mode: 1-3 tasks max, explicit approval gates, user-visible checkpoints.
- Night Mode: safe/unblocked only (tests, docs, refactors, polish). No credentials work.
- Never pivot to chat-first or harness-first user surfaces.

## Skill Routing (Table-of-Contents Style)
- `Friday`:
  - App shell, Rust runner, SQLite schema, IPC integration.
- `Fury`:
  - Permission model, deny-by-default policy, audit/receipts, threat controls.
- `Jarvis`:
  - Skill routing, memory compaction, caching, model routing, cost controls.
- `Loki`:
  - UX copy, clarity, trust messaging, non-jargon language.

Use one owner per task. Co-own only when security/runtime overlap is explicit.

## Hard Safety Rules
- Deny by default for all runtime primitives.
- No arbitrary shell/code execution exposed to end users.
- Any write/send action requires approval by default.
- Human-readable failure reasons only in user-facing fields.
- Idempotency required for side effects.

## Harness-Informed Engineering Rules
- Do not combine:
  - high autonomy
  - broad tool/file/network permissions
  - weak task constraints
- Every new capability must include:
  - explicit action boundary
  - bounded retries with retryable/non-retryable distinction
  - transaction-safe state transition + activity receipt
  - regression test for happy path + failure path

## Required Build Loop Per Task
1. Define scope and non-goals.
2. Implement smallest vertical slice.
3. Add deterministic tests and failure-injection test.
4. Record verification steps in handoff.
5. Stop for approval before expanding surface area.

## Validation Before Merge
- Unit tests green.
- State machine transitions persisted.
- Approval gates exercised.
- Receipts/activity rows present.
- No provider guarantee drift:
  - Supported: OpenAI, Anthropic.
  - Experimental: Gemini (must be labeled; not CI-blocking).
