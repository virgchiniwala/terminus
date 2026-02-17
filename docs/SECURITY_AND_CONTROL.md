# SECURITY_AND_CONTROL.md
Last updated: 2026-02-17

## Security Posture
Terminus is control-first and deny-by-default.

Security is expressed as predictable product behavior, not hidden technical complexity.

## Action Boundaries (Per Autopilot)
Each Autopilot must define:
- what can be read
- what can be written
- what is explicitly disallowed
- cost and send constraints

## Deny-by-Default Enforcement
Runtime denies actions unless explicitly allowlisted by plan and policy.

Deny conditions include:
- primitive not allowlisted
- non-allowlisted domain/source
- out-of-scope storage access
- unapproved write/send actions

Failure text must be human-readable.

## Outbound Policy (MVP)
Default is compose-only.

Send is allowed only when all are true:
- per-autopilot send enabled
- per-run explicit approval
- recipient/domain allowlist present
- max sends/day set
- quiet hours policy satisfied

## Secrets and Sensitive Data
- Secrets are stored in OS keychain only.
- Secrets are never written to repo files, logs, receipts, or export bundles.
- Receipts and learning records must be redacted and bounded.

## Export / Portability
- Local vault remains user-owned.
- Export artifacts exclude secrets.
- Snapshot/restore supports recovery without widening permissions.

## Learning Layer Security Constraints
Learning can adjust bounded profile knobs only.

Learning cannot:
- add primitives
- relax allowlists
- enable send
- add recipients/domains
- create executable tools

See `docs/LEARNING_LAYER.md`.
