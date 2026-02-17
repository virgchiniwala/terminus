# DIFFERENTIATION.md
Last updated: 2026-02-17

## Why Terminus Exists
People want reliable follow-through from everyday intentions without becoming automation engineers.

## Terminus vs Chat-First Assistants
Terminus differs by default surface:
- Terminus: object-first operating model (Autopilots, Outcomes, Approvals, Activity)
- Chat-first tools: conversational thread as primary container

Terminus favors persistent intention objects and repeatable execution over prompt-thread management.

## Terminus vs OpenClaw-Style Systems
Terminus MVP explicitly excludes:
- arbitrary end-user code execution
- end-user skill/tool authoring
- extension marketplace behavior
- harness knobs exposed as main UX

Terminus keeps capabilities constrained and trust-forward.

## Terminus vs Cloud-Only Agents
Terminus MVP is local-first:
- local run execution
- local vault ownership
- keychain-based secret handling
- clear “runs while app open / Mac awake” truth

Hosted relay is future lane, not MVP baseline.

## Anti-Clone-Drift Checklist
Do not ship changes that move Terminus toward:
- chat as primary home
- harness controls as primary product
- unconstrained tool execution
- capability growth hidden from users

## Core Differentiators (MVP)
1. One shared Autopilot runtime for 3 high-value presets
2. Approval-first write/send boundaries
3. Runtime reliability with receipts and recoverable failures
4. Behavioral self-improvement via bounded Learning Layer
5. Calm, premium UX language with minimal cognitive load
