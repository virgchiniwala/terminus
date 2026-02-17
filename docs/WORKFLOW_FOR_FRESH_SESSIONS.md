# WORKFLOW_FOR_FRESH_SESSIONS.md
Last updated: 2026-02-17

## Session Start Checklist
1. Read these in order:
   - `docs/Terminus_CONTEXT.md`
   - `docs/plan.md`
   - `docs/PRINCIPLES_AND_CONSTRAINTS.md`
   - `docs/PRIMITIVES.md`
   - `docs/PLAN_SCHEMA.md`
   - `docs/RUNNER_STATE_MACHINE.md`
   - `docs/SECURITY_AND_CONTROL.md`
   - `docs/LEARNING_LAYER.md`
   - `mission_control.md`
   - `handoff.md`

2. Restate the binding constraints before proposing work.

3. Check clone-drift risk explicitly:
   - chat-first drift?
   - harness-first drift?
   - marketplace drift?
   - permission expansion drift?

4. Propose 1-3 tasks max with:
   - acceptance criteria
   - verification steps
   - non-goals

5. Execute only approved scope.

## Binding Constraints Snapshot
- Object-first product surface
- Shared runtime for 3 presets
- Deny-by-default primitives
- Compose-first sending policy
- Local-first storage and execution
- Provider tiers: Supported vs Experimental
- Learning loop is bounded and explainable

## Documentation Hygiene
When docs are changed:
- add/update `Last updated` tag
- avoid duplicate definitions across files
- keep canonical source references current
- add cross-reference links instead of copy-paste duplication
