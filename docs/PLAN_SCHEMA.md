# PLAN_SCHEMA.md
Last updated: 2026-02-22

## Purpose
Defines the shared Autopilot plan object used by all MVP presets.

One schema must support Website Monitor, Inbox Triage, and Daily Brief without branching into separate runtimes.

## Canonical Plan Shape (Conceptual)
- `schema_version`
- `recipe`
- `intent`
- `provider`
  - `id`
  - `tier`
  - `default_model`
- `web_source_url` (optional)
- `web_allowed_domains` (list)
- `inbox_source_text` (optional)
- `daily_sources` (list)
- `allowed_primitives` (list)
- `steps` (ordered list)

Step shape:
- `id`
- `label`
- `primitive`
- `requires_approval`
- `risk_tier`

## Constraints
- Shared schema across all 3 presets.
- Deny-by-default: runtime validates every step primitive.
- Scheduling and vault-read are not auto-allowlisted by default.
- Provider metadata is attached to plan and persisted on run.

## Approval Model
- Read-only actions can auto-run.
- Write actions default to approval-required where user trust demands it.
- Inbox triage action execution is approval-gated (`triage.email`).
- Send remains gated separately by policy and provider context.

## Profile Overlay (Learning Layer)
Runtime profile can modify bounded execution parameters without mutating core plan capabilities:
- website diff sensitivity
- daily brief source/bullet caps
- inbox reply length hint
- suppression windows

Profile overlay must never:
- add new primitives
- relax allowlists
- enable sending
- add recipients/domains

## Reference
- Runtime implementation: `src-tauri/src/schema.rs`
- Example plans: `docs/plan_schema_examples.json`
- Learning overlay rules: `docs/LEARNING_LAYER.md`
