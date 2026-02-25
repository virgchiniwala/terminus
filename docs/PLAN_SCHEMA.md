# PLAN_SCHEMA.md
Last updated: 2026-02-25

## Purpose
Defines the shared Autopilot plan object used by all recipes. One schema supports all four recipes (Website Monitor, Inbox Triage, Daily Brief, Custom) without branching into separate runtimes.

## Canonical Plan Shape

```rust
// src-tauri/src/schema.rs
pub struct AutopilotPlan {
    pub schema_version: String,        // "1.0"
    pub recipe: RecipeKind,            // WebsiteMonitor | InboxTriage | DailyBrief | Custom
    pub intent: String,                // user's original natural language intent
    pub provider: ProviderMetadata,    // id, tier, default_model
    pub web_source_url: Option<String>,
    pub web_allowed_domains: Vec<String>,  // allowlist for ReadWeb primitive
    pub inbox_source_text: Option<String>,
    pub daily_sources: Vec<String>,
    pub allowed_primitives: Vec<PrimitiveId>,  // deny-by-default: only these primitives can execute
    pub steps: Vec<PlanStep>,          // ordered execution sequence
}

pub struct PlanStep {
    pub id: String,
    pub label: String,              // human-readable step description
    pub primitive: PrimitiveId,
    pub requires_approval: bool,
    pub risk_tier: RiskTier,        // Low | Medium | High
}
```

## Recipe Values

| RecipeKind | Source | Steps populated by |
|---|---|---|
| `WebsiteMonitor` | `AutopilotPlan::from_intent()` | Hardcoded sequence |
| `InboxTriage` | `AutopilotPlan::from_intent()` | Hardcoded sequence |
| `DailyBrief` | `AutopilotPlan::from_intent()` | Hardcoded sequence |
| `Custom` | `generate_custom_plan()` | LLM-generated + validated |

## Constraints
- Shared schema across all 4 recipes.
- Deny-by-default: PrimitiveGuard validates every step primitive against `allowed_primitives`.
- `ScheduleRun` and `ReadVaultFile` are not auto-allowlisted by default.
- Provider metadata is attached to plan and persisted on run.
- Maximum 10 steps per plan (enforced in `validate_and_build_custom_plan()` / `validate_custom_execution_plan()` for Custom; conventions for presets).

## Custom Recipe Notes (Dynamic Plan Generation)

For `RecipeKind::Custom`:
- `steps` are populated by `generate_custom_plan()` in `src-tauri/src/main.rs`, not by `from_intent()`
- `web_allowed_domains` must contain every domain that a `ReadWeb` step will fetch
- `recipient_hints` must contain every email address that a `SendEmail` step will target
- `allowed_primitives` is computed from actual steps server-side, not from LLM-declared list
- **Safety invariants enforced server-side regardless of LLM output:**
  - `SendEmail` always gets `requires_approval: true` and `risk_tier: High`
  - `WriteOutcomeDraft`, `WriteEmailDraft`, `TriageEmail` always get `requires_approval: true`
  - `ScheduleRun` and `ReadVaultFile` are rejected for Custom plans
  - Max 10 steps hard cap
  - Unknown primitive IDs → validation error (no plan created)
  - `ReadWeb` requires detected URL + allowlisted domain; `ReadSources` requires detected source URLs
- The generated plan is shown to users in the Draft Plan Card before committing
- `start_recipe_run` accepts a `plan_json` parameter for Custom plans (pre-generated in `draft_intent`)

## Approval Model
- Read-only actions (`ReadWeb`, `ReadSources`, `ReadForwardedEmail`) can auto-run with `requires_approval: false`.
- Write actions (`TriageEmail`, `WriteOutcomeDraft`, `WriteEmailDraft`) default to approval-required.
- `SendEmail` is **always** approval-required, enforced before execution (Safe Effector gate).
- For Custom plans: server-side `validate_and_build_custom_plan()` and `validate_custom_execution_plan()` enforce approval rules regardless of LLM output.

## Profile Overlay (Learning Layer)
Runtime profile can modify bounded execution parameters without mutating core plan capabilities:
- website diff sensitivity
- daily brief source/bullet caps
- inbox reply length hint
- suppression windows

Profile overlay must never:
- add new primitives to `allowed_primitives`
- relax `requires_approval` constraints
- enable `SendEmail` if it wasn't in the original plan
- add recipients/domains to allowlists

## Reference
- Runtime implementation: `src-tauri/src/schema.rs`
- Plan generation: `src-tauri/src/main.rs` (`generate_custom_plan`, `validate_and_build_custom_plan`, `validate_custom_execution_plan`)
- PrimitiveGuard enforcement: `src-tauri/src/primitives.rs`
- Example plans: `docs/plan_schema_examples.json` (if it exists)
- Learning overlay rules: `docs/LEARNING_LAYER.md`
- Primitive catalog: `docs/PRIMITIVES.md`
