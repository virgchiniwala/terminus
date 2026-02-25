# Mission Control — Terminus
Last updated: 2026-02-25

## Fresh Session Note
Read these in order before starting work:
1. `docs/Terminus_CONTEXT.md` — what Terminus is + key strategic directions
2. `docs/TERMINUS_AUDIT_AND_PLAN.md` — comprehensive audit + current P0-P8 priority order
3. `docs/TERMINUS_PRODUCT_STRATEGY_v3.md` — complete product vision
4. `docs/WORKFLOW_FOR_FRESH_SESSIONS.md` — session checklist

## Current State
- Mode: Day
- Branch: `codex/callapi-primitive-mvp`
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
- Primary transport: RelayTransport (shipped desktop-side transport + remote approval path); BYOK via LocalHttpTransport (advanced)

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
- Dynamic Plan Generation (Custom recipe) with server-side plan validation
- Relay approval callback + sync polling + push channel consumer (desktop-side)
- Interview-driven onboarding (first-run guided recommendation flow)
- Voice object MVP (global + per-Autopilot overrides)
- Relay-backed Webhook Trigger MVP (bounded inbound event -> run enqueue)
- `CallApi` primitive MVP (bounded outbound HTTP, approval-gated, Keychain key refs)

## Test Coverage Baseline
| Category | Status |
|----------|--------|
| Backend Rust (`cargo test`) | 91/91 passing |
| Mission tests | 3/3 passing |
| Frontend component tests | 2 (ConnectionHealthSummary only) |
| Integration tests | 0 |
| **Gaps** | App.tsx (1,253 lines, 0 tests), ApprovalPanel, IntentBar, RunnerStatus |

## Now
### Gmail PubSub Trigger Path (Gmail-only, polling fallback preserved)
Owner: active session
Status: In progress
Scope:
- Add Gmail PubSub trigger state + event log tables (Gmail-only) and relay callback ingestion path
- Reuse existing inbox watcher Gmail fetch/queue path (PubSub triggers fetch; no direct message processing)
- Add trigger mode (`polling|gmail_pubsub|auto`) with polling fallback when PubSub is inactive/expired
- Add minimal Connections UI for watch health, mode, config, and recent PubSub events
- Update docs/handoff so trigger model and next phases stay aligned
Acceptance:
- Valid PubSub callback enqueues Gmail fetch path (through existing inbox watcher path), not a parallel runner path
- Duplicate PubSub callbacks do not create duplicate runs
- Invalid/malformed callback payloads fail with human-readable event status
- `auto` mode falls back to polling when PubSub is expired/error/disabled
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
1. **Rule extraction / "Make This a Rule" (P0.12)**
   - rule object + rule applications + approval-gated creation
2. **Relay multi-device routing + device targeting foundations**
   - explicit preferred target / queue-until-online semantics
3. **Ownership leases + doctor surfaces**
   - runner/push consumer ownership locks and operator-readable health/runbooks

## Non-goals (MVP)
- Arbitrary end-user code execution
- Plugin marketplace
- OpenClaw compatibility layer
- Cloud-side run execution (relay is transport, not runner)
- MCP as primitive source (long-term direction; keep PrimitiveId extensible)
