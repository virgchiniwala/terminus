# PROVIDERS_AND_PACKAGING.md
Last updated: 2026-02-25

## Provider Tier Policy
Supported:
- OpenAI
- Anthropic

Experimental:
- Gemini (disabled in BYOK lane; relay handles when available)

Supported providers are required for CI reliability guarantees. Experimental providers are available but excluded from support guarantees.

## Runtime Provider Architecture
- Provider metadata is attached to plan and run records.
- Execution goes through provider + transport abstractions.
- Runner does not branch on provider-specific strings.
- For relay-backed plans, the relay selects provider/model based on task class and tier; the client does not specify.

## Transport Architecture (Updated 2026-02-25)

Three transport implementations exist on the `ExecutionTransport` trait:

1. **RelayTransport** (primary, shipped desktop-side; relay service rollout continues):
   - Client sends `ProviderRequest` + subscriber token to Terminus relay
   - Relay validates token + tier, selects optimal provider, returns `ProviderResponse`
   - Relay enforces per-tier rate limits (50 runs/mo Free, 500 runs/mo Pro)
   - No provider API keys on client side; relay handles centrally
   - `requires_keychain_key()` returns `false`
   - Enables remote approval (see Relay Design Requirements below)

2. **LocalHttpTransport** (advanced/BYOK):
   - User provides own API keys stored in Keychain
   - Limited support; cannot be monetized
   - Accessible via "Advanced" settings for technical users
   - Gemini is explicitly disabled in this transport (returns non-retryable error)

3. **MockTransport** (test fixture):
   - Returns predictable responses for unit and integration tests
   - No external network calls

**Transport selection at runtime:** if `subscriber_token` present in Keychain → `RelayTransport`; else → `LocalHttpTransport`.

## Relay Design Requirements

The relay service (outside the Tauri app) must implement:

- **`POST /dispatch`**: accepts `{subscriber_token, provider_request: ProviderRequest}`, returns `ProviderResponse`
- **WebSocket/SSE channel**: push events to connected clients (approval routing, notifications)
- **`POST /relay/approve/{run_id}/{step_id}`**: resolves pending approval from any authenticated surface (Slack DM, web, mobile)
- **Rate limiting**: per subscriber_token + tier
- **No raw content logged** on relay side (privacy)
- **Slack bot integration**: relay routes approval decisions from Slack DM → local runner via WebSocket/SSE

The relay enables the "OpenClaw on mobile" pattern: pending approvals route to Slack, users approve inline without opening the Mac app. This is why the push channel is not optional — it is a day-1 relay requirement.

## Pricing Model

| Tier | Runs/month | Features | BYOK |
|------|------------|----------|------|
| Free | 50 | All presets + Custom recipe | No |
| Pro | 500 | + Professional templates | Optional |
| Advanced | Unlimited via own keys | Limited support | Yes |

## Onboarding Flow (Relay)

1. First launch: agent presents "Sign in to Terminus" inline (not a pre-requisite form)
2. User signs up → subscriber token issued
3. Token stored in Keychain → `RelayTransport` activated automatically
4. No provider API key setup required

## BYOK Authentication Modes (Advanced)
- API keys (current): OpenAI / Anthropic provider keys stored in macOS Keychain
- Arbitrary API key refs (current): used by the bounded `CallApi` primitive (`terminus.api_key_ref.<ref>`)
- **Planned next:** Codex OAuth (ChatGPT sign-in) for OpenAI/Codex in BYOK advanced mode, stored in Keychain with refresh handling

## Cost and Currency Policy
- User-facing budget policy defaults to SGD
- Runtime enforces hard/soft rails before side effects
- Receipts include provider tier and run cost details
- Relay tracks per-token usage for tier enforcement

## UX Language Rules
User-facing copy must avoid backend jargon. Use language such as:
- "Connected" (not "RelayTransport initialized")
- "Terminus plan" (not "subscriber_token")
- "Estimated cost" (not "token count")
- "Monthly limit reached" (not "rate limit exceeded")
- "Advanced mode" (not "BYOK")

See `docs/FUTURE_EXTENSION.md` for Slack bot design and MCP primitive source direction.
