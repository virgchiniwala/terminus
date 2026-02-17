# PRINCIPLES_AND_CONSTRAINTS.md
Last updated: 2026-02-17

## Product Principles
1. Object-first, not chat-first
2. Constrained primitives with deny-by-default policy
3. Reliability is a product surface
4. Local ownership and portability
5. Provider flexibility with explicit support tiers
6. Compose-first trust model for outbound actions
7. Continuous improvement through bounded feedback

## Hard Constraints (MVP)
- No arbitrary shell/code execution for end users
- No end-user tool authoring UI
- No marketplace execution model
- No OpenClaw compatibility requirements
- No IMAP/OAuth inbox integration
- No hosted always-on runner

## Shared Runtime Constraint
All 3 MVP presets must run on:
- one plan schema
- one primitive catalog
- one runner lifecycle
- one approval queue
- one receipt model

## Spend Constraint
- User-facing default currency: SGD
- Runtime hard/soft rails enforced before side effects
- Clear recovery options at hard limit

## Outbound Constraint
Default compose-only.

Send requires all policy gates:
- explicit toggle
- per-run approval
- allowlist checks
- max/day limit
- quiet hours

## Learning Constraint
Learning may optimize only bounded knobs.
Learning may not expand capabilities or permissions.

## Documentation Constraint
- avoid user-facing technical jargon in public product docs
- keep canonical definitions in dedicated files
- cross-reference instead of duplicating competing definitions
