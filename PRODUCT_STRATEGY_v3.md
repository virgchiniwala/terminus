# PRODUCT_STRATEGY_v3
Last updated: 2026-02-21

Canonical product strategy now lives in `docs/TERMINUS_PRODUCT_STRATEGY_v3.md`.

This branch enforces these invariants in code and copy:
- Terminus is object-first, not chat-first.
- Primary completion is executed outcomes or one-tap approvals.
- Draft text is internal payload, not the primary user artifact.
- Risky actions remain policy-gated (allowlist + approval + limits + quiet hours).
- Runner remains local-first and tick-based with persisted state and receipts.
