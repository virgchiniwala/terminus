# LEARNING_LAYER.md
Last updated: 2026-02-17

## Purpose
The Learning Layer makes Autopilots improve over time using bounded behavioral feedback.

Loop:
Plan -> Run -> Receipt -> Evaluate -> Adapt -> Memory

This is not user tool authoring. It is controlled runtime refinement.

## Design Goals
- local-first persistence
- deterministic behavior
- explainable updates
- bounded impact
- no capability growth

## Data Model
- `decision_events`: append-only behavior signals
- `run_evaluations`: one evaluation per terminal run
- `adaptation_log`: append-only profile change record
- `autopilot_profile`: current learning mode + bounded knobs
- `memory_cards`: compact preference/state memory

## Evaluate
Evaluate runs at terminal state and computes:
- quality score
- noise score
- cost score
- key signals

Signals are bounded metadata only. No raw source payloads.

## Adapt
Adaptation reads recent evaluations/events and updates only allowlisted knobs.

Allowed updates:
- mode (`max_savings | balanced | best_quality`)
- website sensitivity/suppression bounds
- daily brief source/bullet bounds
- inbox reply length hint/suppression bounds

Adaptation cannot:
- add primitives
- relax allowlists
- enable send
- add recipients/domains
- create executable behavior

Every adaptation is logged with rationale codes.

## Memory
Memory cards are compact, structured, and bounded.

Examples:
- preferred reply style
- noise suppression rationale
- source scope preference

Memory recall is bounded:
- max 5 cards
- max 1500 chars total

Receipts list memory titles used.

## Runtime Integration
On terminal run:
1. evaluate run (idempotent)
2. adapt profile (bounded)
3. refresh memory cards
4. enrich receipt

At run preflight:
- profile and suppression can adjust execution parameters
- suppression can early-exit safely with clear receipt text

## Verification Expectations
- evaluation idempotency tests
- adaptation bounds/invariant tests
- suppression no-side-effect tests
- memory recall size-bound tests
- no-raw-content persistence tests
