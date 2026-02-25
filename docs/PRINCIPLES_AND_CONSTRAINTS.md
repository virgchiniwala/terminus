# PRINCIPLES_AND_CONSTRAINTS.md
Last updated: 2026-02-25

## Product Principles

1. **Object-first, not chat-first.** Home surfaces = Autopilots / Outcomes / Approvals / Activity. Chat is an input method; the end state is always an executable object.

2. **Constrained primitives with deny-by-default policy.** Terminus executes only what is explicitly allowlisted in each plan. No primitive executes without being in `allowed_primitives`. PrimitiveGuard is the enforcement layer.

3. **Reliability is a product surface.** State, retries, idempotency, receipts, and human-readable failure reasons are product features, not implementation details.

4. **Local ownership and portability.** Data, vault files, and secrets live on the user's machine. Relay is a transport; runner and data stay local.

5. **Provider flexibility with explicit support tiers.** Supported (OpenAI, Anthropic) vs Experimental (Gemini). Relay handles provider selection for hosted plans; client does not branch on provider strings.

6. **Compose-first trust model for outbound actions.** Default is compose-only. Sending requires 5 explicit policy gates (toggle, per-run approval, allowlist, max/day, quiet hours).

7. **Continuous improvement through bounded feedback.** Learning Layer adjusts bounded knobs. Cannot expand capabilities, add recipients, or bypass approvals.

8. **Harness-first design.** Every new capability must fit within existing safety rails. Architecture constrains rather than expands. Bounded tool catalog > unconstrained tool execution. When adding a new primitive, it must work within PrimitiveGuard — no bypass paths.

9. **Relay-primary packaging.** Hosted plans via relay are the default onboarding and monetization path. BYOK (LocalHttpTransport) is an advanced escape hatch for technical users. BYOK cannot be monetized and receives limited support.

10. **Interview-driven setup.** The agent onboards you. No pre-configuration required before the first result. Every step requiring a form before a result is an onboarding failure. The Intent Bar is the setup mechanism.

11. **No draft-only outputs.** Every run must have at least one real side effect (sent email, filed document, delivered brief). A run that produces only text for the user to copy elsewhere is a failed run, not a success case.

## Hard Constraints (MVP)
- No arbitrary shell/code execution for end users
- No end-user tool authoring UI
- No marketplace execution model
- No OpenClaw compatibility requirements
- No IMAP/OAuth inbox integration (use forwarded email as input for MVP)
- No cloud-side execution (relay is transport only; runner stays local)
- No closing of the PrimitiveId type (must remain extensible for future MCP consumption)

## Shared Runtime Constraint
All 4 recipes (WebsiteMonitor, InboxTriage, DailyBrief, Custom) must run on:
- one plan schema (`AutopilotPlan`)
- one primitive catalog (`PrimitiveId` enum)
- one runner lifecycle (tick-based state machine)
- one approval queue
- one receipt model

## Spend Constraint
- User-facing default currency: SGD
- Runtime hard/soft rails enforced before side effects
- Clear recovery options at hard limit (not silent failure)

## Outbound Constraint
Default compose-only. Send requires all 5 policy gates:
1. Explicit per-autopilot toggle enabled
2. Per-run approval granted
3. Recipient in allowlist
4. Under max/day limit
5. Outside quiet hours

`SendEmail` always requires approval — this is enforced in `validate_and_build_plan()` for Custom plans and in hardcoded recipe steps for presets. No plan can override this.

## Learning Constraint
Learning (Evaluate → Adapt → Memory pipeline) may optimize only bounded knobs:
- website diff sensitivity
- daily brief source/bullet caps
- inbox reply length hint
- suppression windows

Learning may not:
- add new primitives to `allowed_primitives`
- add new email recipients or domains
- enable `SendEmail` if not in original plan
- bypass approval gates
- create new executable capabilities

## Documentation Constraint
- Avoid user-facing technical jargon in public product docs
- Keep canonical definitions in dedicated files (cross-reference, don't duplicate)
- When docs conflict, defer to `docs/TERMINUS_AUDIT_AND_PLAN.md` for priority order and `docs/PRINCIPLES_AND_CONSTRAINTS.md` for rules
- Update `mission_control.md` whenever the current task changes
