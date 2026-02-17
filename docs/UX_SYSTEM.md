# UX_SYSTEM.md
Last updated: 2026-02-17

## UX Direction
Terminus should feel calm, premium, and quietly capable.

The interface must communicate control and follow-through without exposing implementation complexity.

## Product Surface Hierarchy
Primary objects:
- Autopilots
- Outcomes
- Approvals
- Activity

Home summarizes these objects. Chat input must never dominate layout.

## Copy and Tone
- concise and calm
- no hype language
- no technical implementation jargon in user-facing strings
- explain outcomes and decisions, not mechanisms

## Trust Design Rules
- approvals show clear preview/diff
- receipts explain what happened and why
- failures always include clear recovery options
- spend messages are currency-first and human-readable

## Empty States
Every empty state must include:
- one-line value statement
- one clear next action
- no dead-end “no data” language

## Interaction Constraints
- compose-first by default
- sending requires explicit user gates
- no hidden autonomous capability growth

## Anti-Drift Rules
Do not ship interfaces that look like:
- a developer harness
- a chat command console
- a plugin marketplace manager

See `docs/DIFFERENTIATION.md` and `docs/PRINCIPLES_AND_CONSTRAINTS.md`.
