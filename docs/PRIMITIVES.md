# PRIMITIVES.md
Last updated: 2026-02-25

## Purpose
Primitives are Terminus runtime actions. They are constrained by design and deny-by-default.

A plan can only execute primitives explicitly allowlisted in that plan (`allowed_primitives` field). This is enforced by `PrimitiveGuard` in `src-tauri/src/primitives.rs` — an unknown or non-allowlisted primitive fails with a human-readable error before any side effect executes.

## Triggers vs Primitives (Important)
- **Triggers** decide *when* an Autopilot run starts (manual, schedule, inbox watcher, webhook).
- **Primitives** decide *what* the run is allowed to do after it starts.

Webhook Trigger MVP is a relay-backed trigger surface, not a new primitive. Webhook payloads can start runs, but they do not expand runtime capabilities. All existing primitive allowlists, approval gates, spend rails, and receipts still apply.

## Primitive Catalog (Complete)

**Read (low risk, no approval needed by default):**
- `ReadWeb` — fetch content from a URL; requires `web_allowed_domains` to include the host
- `ReadForwardedEmail` — read an email that was forwarded or pasted
- `ReadSources` — read multiple configured source URLs
- `ReadVaultFile` — path-scoped file read (after explicit vault setup)

**Aggregate (medium risk, no approval needed by default):**
- `AggregateDailySummary` — synthesize multiple source contents into a cohesive summary

**Write (medium risk, approval required by default):**
- `TriageEmail` — classify and label an email (archive, label, flag)
- `WriteOutcomeDraft` — create a completed outcome card (summary, brief, analysis)
- `WriteEmailDraft` — draft an email for the approval queue

**Restricted / High Risk (always approval required):**
- `SendEmail` — send an approved email; all 5 Safe Effector gates must pass; **always requires approval regardless of plan or LLM output**
- `CallApi` — bounded outbound HTTP API call (HTTP/HTTPS, GET/POST MVP), Keychain secret refs only, allowlisted domains, **approval-gated by default**
- `ScheduleRun` — schedule a future run; manual-first policy; not auto-allowlisted
- `NotifyUser` — send a system notification (low risk)

## Safety Rules
- **Unknown primitive:** fail with human-readable message. `PrimitiveGuard::validate()` rejects.
- **Non-allowlisted primitive:** fail with human-readable message. PrimitiveGuard enforces before execution.
- **`SendEmail` always requires approval.** Server-side validation (`validate_and_build_plan()`) enforces this regardless of plan source (preset or LLM-generated).
- No arbitrary shell or code execution primitive.
- No primitive that installs or executes third-party end-user tools.

## Recipe-to-Primitive Mapping

**Website Monitor:**
1. `ReadWeb` → fetch source URL
2. `WriteOutcomeDraft` → diff summary (approval)
3. `WriteEmailDraft` → notification draft (approval)

**Inbox Triage:**
1. `ReadForwardedEmail` → read pasted/forwarded content
2. `TriageEmail` → classify + triage action (approval)
3. `WriteOutcomeDraft` → triage summary
4. `WriteEmailDraft` → reply draft (approval)

**Daily Brief:**
1. `ReadSources` → read configured source URLs
2. `AggregateDailySummary` → synthesize content
3. `WriteOutcomeDraft` → daily brief card (approval)

**Custom (Dynamic Plan Generation):**
Any subset of {`ReadWeb`, `ReadSources`, `ReadForwardedEmail`, `TriageEmail`, `AggregateDailySummary`, `WriteOutcomeDraft`, `WriteEmailDraft`, `SendEmail`, `NotifyUser`, `CallApi`} as determined by LLM plan generation + server-side validation. The `allowed_primitives` field is computed from actual steps, not from LLM-declared list. Safety invariants apply regardless of what the LLM outputs.

## What Primitives Cannot Do
- Expand permissions at runtime
- Bypass approvals for write/send actions
- Change allowlists autonomously
- Create new executable capabilities
- Access system resources not in the bounded catalog
- Accept arbitrary inbound webhook payloads as code/transforms

## Integration Boundary (Current)
- **Shipped:** relay-backed inbound webhook triggers (bounded JSON event ingress -> run enqueue)
- **Shipped:** `CallApi` primitive MVP (approval-gated, allowlisted outbound HTTP using Keychain key refs)
- **Planned next:** Codex OAuth BYOK auth mode (OpenAI/Codex sign-in path) and broader rule-driven operator teaching loops

## PrimitiveGuard Enforcement
`PrimitiveGuard` in `src-tauri/src/primitives.rs` is the deny-by-default enforcement layer:
- `PrimitiveGuard::new(allowed_primitives)` — constructed from plan's allowlist
- `validate(primitive_id)` — returns `PrimitiveGuardError::NotAllowed` if not in allowlist
- Called in `execute_step()` before any primitive logic runs
- Failure is non-retryable and results in a human-readable error on the Outcome

## MCP Direction (Long-term Architectural Note)

The current primitive catalog is hardcoded (12 `PrimitiveId` enum variants). The long-term direction is to make primitives MCP-consumable:
- `terminus load-mcp box` → BoxRead/BoxWrite primitives
- `terminus load-mcp slack` → SlackRead/SlackSend primitives
- `terminus load-mcp calendar` → CalendarRead primitive

**Design implication:** Do NOT make `PrimitiveId` a closed/exhaustive enum. Keep it extensible so future MCP tool IDs can be added without breaking existing match arms. See `docs/FUTURE_EXTENSION.md`.

See `docs/SECURITY_AND_CONTROL.md` and `docs/LEARNING_LAYER.md` for full safety model.
