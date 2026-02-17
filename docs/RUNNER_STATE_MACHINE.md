# RUNNER_STATE_MACHINE.md
Last updated: 2026-02-17

## Goal
Provide reliable, explainable local execution with bounded behavior.

## Execution Model
- `start_recipe_run`: persists run and returns (`ready`)
- `run_tick`: advances at most one bounded step window
- `resume_due_runs`: resumes retrying runs whose backoff window has elapsed

No loop-to-terminal blocking in command handlers.

## Core States
- `draft`
- `ready`
- `running`
- `needs_approval`
- `retrying`
- `succeeded`
- `failed`
- `blocked`
- `canceled`

## State Guarantees
- Run transitions and activity entries are written atomically.
- Terminal states generate a terminal receipt.
- Retry metadata (`retry_count`, backoff, next retry time) is persisted atomically.
- Idempotency prevents duplicate side effects on retries/resume.

## Retry Policy
- Retry only retryable failures.
- Bounded by `max_retries`.
- Backoff schedule uses persisted `next_retry_at_ms`.
- Non-retryable failures terminate with recoverable reason text.

## Approval Flow
- If step requires approval:
  - create approval row
  - set run `needs_approval`
  - halt execution
- Approve:
  - update approval status
  - emit decision event
  - resume run
- Reject:
  - update approval status
  - emit decision event
  - terminate run (`canceled` or `blocked` based on context)

## Spend and Gate Interactions
- Hard spend rail: block before side effects.
- Soft spend rail: requires explicit approval to continue.
- Compose-only default is enforced in execution layer.

## Learning Hook
On terminal run:
1. evaluate run
2. apply bounded adaptation
3. refresh memory cards
4. enrich terminal receipt

All learning operations are idempotent and do not expand capabilities.

See `docs/LEARNING_LAYER.md`.
