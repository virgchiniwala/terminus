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
| Backend Rust (`cargo test`) | 85/85 passing |
| Mission tests | 3/3 passing |
| Frontend component tests | 2 (ConnectionHealthSummary only) |
| Integration tests | 0 |
| **Gaps** | App.tsx (1,253 lines, 0 tests), ApprovalPanel, IntentBar, RunnerStatus |

## Now
### Codex OAuth BYOK Support (OpenAI/Codex sign-in import)
Owner: active session
Status: In progress
Scope:
- Add Codex OAuth BYOK auth mode by importing local Codex CLI OAuth session (`~/.codex/auth.json`)
- Store imported Codex OAuth tokens in Keychain only (never SQLite/logs/receipts)
- Let Local BYOK OpenAI requests use Codex OAuth access token when no manual API key is set
- Add minimal Connections UI for import/remove/status (advanced mode)
- Update packaging/strategy docs so Codex OAuth is marked shipped in BYOK lane
Acceptance:
- Codex OAuth import reads only local `~/.codex/auth.json` and stores credentials in Keychain
- No token values appear in logs/receipts/UI status payloads
- OpenAI BYOK requests work with imported Codex OAuth credentials when API key is absent
- Removing Codex OAuth credentials cleanly disables that auth path
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
2. **Interview onboarding polish + voice/rules UX**
   - first-result path polish, stronger tests, clearer defaults
3. **CallApi follow-up hardening**
   - response parsing presets, method/domain policy expansion, reusable API action templates

## Non-goals (MVP)
- Arbitrary end-user code execution
- Plugin marketplace
- OpenClaw compatibility layer
- Cloud-side run execution (relay is transport, not runner)
- MCP as primitive source (long-term direction; keep PrimitiveId extensible)
