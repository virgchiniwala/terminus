# PROVIDERS_AND_PACKAGING.md
Last updated: 2026-02-17

## Provider Tier Policy
Supported:
- OpenAI
- Anthropic

Experimental:
- Gemini

Supported providers are required for CI reliability guarantees. Experimental providers are available but excluded from support guarantees.

## Runtime Provider Architecture
- Provider metadata is attached to plan and run records.
- Execution goes through provider + transport abstractions.
- Runner does not branch on provider-specific strings.

## Local-First Packaging (MVP)
- Default lane: local BYOK with keychain-stored credentials.
- No hosted runner in MVP.
- No provider connection UI required to use internal transport seams.

## Future Packaging Lane (Not MVP)
- Hosted relay transport for managed plans.
- Relay is a transport swap, not a runner rewrite.
- Managed lane may provide stronger centralized budget and routing control.

See `docs/FUTURE_EXTENSION.md`.

## Cost and Currency Policy
- User-facing budget policy defaults to SGD.
- Runtime enforces hard/soft rails before side effects.
- Receipts include provider tier and run cost details.

## UX Language Rules
User-facing copy must avoid backend jargon.
Use language such as:
- Connected
- Supported
- Experimental
- Estimated cost
- Hard limit reached
