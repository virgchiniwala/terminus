# PRIMITIVES.md
Last updated: 2026-02-22

## Purpose
Primitives are Terminus runtime actions. They are constrained by design and deny-by-default.

A plan can only execute primitives explicitly allowlisted in that plan.

## Primitive Catalog (MVP)
Read:
- `read.web`
- `read.forwarded_email`
- `read.sources`

Write:
- `triage.email` (approval-gated provider action)
- `write.outcome_draft`
- `write.email_draft`

Restricted / disabled in MVP by default:
- `send.email` (blocked unless strict policy gates pass)
- `schedule.run` (manual-first policy; not auto-allowlisted)
- `read.vault_file` (path-scoped only after explicit vault setup)

## Safety Rules
- Unknown primitive: fail with human-readable message.
- Non-allowlisted primitive: fail with human-readable message.
- No arbitrary shell or code execution primitive.
- No primitive that installs or executes third-party end-user tools.

## Recipe-to-Primitive Mapping
Website Monitor:
1. `read.web`
2. `write.outcome_draft` (approval)
3. `write.email_draft` (approval)

Inbox Triage:
1. `read.forwarded_email`
2. `triage.email` (approval)
3. `write.outcome_draft`
4. `write.email_draft` (approval)

Daily Brief:
1. `read.sources`
2. `aggregate.daily_summary` behavior via provider execution step
3. `write.outcome_draft` (approval)

Note: Aggregation behavior is represented in plan/runner as a constrained runtime step, not an open plugin hook.

## What Primitives Cannot Do
- expand permissions at runtime
- bypass approvals for write/send actions
- change allowlists autonomously
- create new executable capabilities

See `docs/SECURITY_AND_CONTROL.md` and `docs/LEARNING_LAYER.md`.
