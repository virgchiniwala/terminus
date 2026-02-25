# Mission Control — Terminus
Last updated: 2026-02-26

## Fresh Session Note
Read these in order before starting work:
1. `docs/Terminus_CONTEXT.md` — what Terminus is + key strategic directions
2. `docs/TERMINUS_AUDIT_AND_PLAN.md` — comprehensive audit + current P0-P8 priority order
3. `docs/TERMINUS_PRODUCT_STRATEGY_v3.md` — complete product vision
4. `docs/WORKFLOW_FOR_FRESH_SESSIONS.md` — session checklist

## Current State
- Mode: Day
- Branch: `codex/relay-transport-doc-sync`
- Product shape: local-first, object-first Personal AI OS + personal agent harness

## Strategic Guardrails
- Home remains object-first (`Autopilots / Outcomes / Approvals / Activity`)
- No chat-first drift, harness-first drift, or marketplace drift
- Deny-by-default primitives (PrimitiveGuard)
- Outputs must have real side effects; draft-only runs are failure cases
- Compose-first outbound behavior with gated sending
- Secrets only in OS Keychain (never SQLite, never logs)
- Shared runtime for all recipes (Website Monitor, Inbox Triage, Daily Brief, Custom)
- Intent Bar outputs must always resolve to executable objects, never free-text responses
- Relay is the primary transport; BYOK is advanced-only
- The agent onboards users; no pre-configuration required before first result

## Provider Policy
- Supported: OpenAI, Anthropic
- Experimental: Gemini (disabled in BYOK lane)
- Primary transport: RelayTransport (P1 in development); BYOK via LocalHttpTransport (advanced)

## Recipes (Shared Runtime)
1. Website Monitor
2. Inbox Triage (paste/forward + always-on watching)
3. Daily Brief
4. Custom (Dynamic Plan Generation — P0, current work)

## Runtime Baseline (Shipped)
- Persisted run state machine with tick execution
- Approval queue with resume/reject paths
- Retry/backoff with due-run resumption
- Spend rails in cents with pre-side-effect hard stops
- Terminal receipts with redaction
- Provider/transport seam (LocalHttpTransport BYOK + RelayTransport hosted + MockTransport tests)
- Learning Layer: Evaluate → Adapt → Memory
- OAuth provider connections + inbox watcher cadence controls
- Safe send policy gates + typed approval payload columns
- Mission orchestration: tables + commands + 3 tests (fan-out, contract blocking, aggregation)
- Supervisor diagnostics: 11 run-health states + intervention commands

## Test Coverage Baseline
| Category | Status |
|----------|--------|
| Backend Rust (`cargo test`) | 77/77 passing |
| Mission tests | 3/3 passing |
| Frontend component tests | 2 (ConnectionHealthSummary only) |
| Integration tests | 0 |
| **Gaps** | App.tsx (1,253 lines, 0 tests), ApprovalPanel, IntentBar, RunnerStatus |

## Now (P1b)
### Remote Approval Path + Provenance Hooks
Owner: active session
Status: In progress
Scope:
- Add relay-aware approval resolution commands (`approve_run_approval_remote`, `reject_run_approval_remote`)
- Route local + remote approvals through shared command helpers using canonical `RunnerEngine::approve/reject`
- Persist approval decision provenance on `approvals` (`decided_channel`, `decided_by`)
- Surface approval resolution provenance in terminal receipts (`approval_resolutions`)
Acceptance:
- Remote approval hooks resolve pending approvals without bypassing existing idempotent approval logic
- Local approvals still work and now record local decision provenance
- Terminal receipts can include approval resolution source metadata (channel / actor)
- `cargo test`, `npm test`, `npm run lint`, `npm run build` pass
Verification:
```bash
cd src-tauri && cargo fmt --check
cd src-tauri && cargo test
npm test
npm run lint
npm run build
```

## Next
1. **P1c: Relay Push Channel + Slack Bot**
   - WebSocket/SSE push for approval routing
   - Slack bot via Vercel Chat SDK pattern (approve from Slack)
   - Remote callback auth and decision routing

2. **P2: Interview-Driven Onboarding**
   - Blank canvas first-launch experience
   - Agent interview flow using Intent Bar
   - `onboarding_state` flag + first-run detection

3. **P3: Voice / Soul.md Object (P0.11)**
   - Voice object (tone/length/humor presets + Voice Notes freeform)
   - Global default + per-autopilot override
   - Injection into emails/summaries/approvals/system messages

4. **P4: Rule Extraction / "Make This a Rule" (P0.12)**
   - Rule object + rule_applications table
   - "Make this a rule" CTA on Outcome + Approval cards

## Non-goals (MVP)
- Arbitrary end-user code execution
- Plugin marketplace
- OpenClaw compatibility layer
- Cloud-side run execution (relay is transport, not runner)
- MCP as primitive source (long-term direction; keep PrimitiveId extensible)
