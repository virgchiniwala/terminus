# Mission Control — Personal AI OS

**Last Updated:** 2026-02-10 19:10 SGT  
**Current Mode:** Day (Vir active)  
**Branch:** main

---

## Now (In Progress)

### T0002: UX Principles & Design System
**Owner:** Loki  
**Status:** Queued (spawning subagent)  
**Scope:** Document Ive-level design system, visual language, typography, spacing, component principles  
**Risks:** None (documentation)  
**Files:** `docs/ux_principles.md`  
**Verification:** Design system documented, ready for UI implementation  
**Branch:** `docs/ux-principles`  
**Next Action:** Spawn Loki to create UX principles doc

### T0006: Token Strategy & QMD Pipeline
**Owner:** Jarvis (main)  
**Status:** Queued  
**Scope:** Document QMD pipeline, memory budgets, caching strategy, compaction triggers, delta prompting  
**Risks:** None (documentation)  
**Files:** `docs/token_strategy.md`  
**Verification:** QMD pipeline fully documented, implementation-ready  
**Branch:** `docs/token-strategy`  
**Next Action:** Create token strategy doc

### T0007: Tauri App Scaffold
**Owner:** Friday  
**Status:** Queued (spawning subagent)  
**Scope:** Bootstrap Tauri desktop app with React frontend, basic IPC, build config  
**Risks:** Tauri version conflicts, Rust toolchain issues  
**Files:** `src-tauri/`, `src/`, `package.json`, `Cargo.toml`  
**Verification:** App builds and runs, shows basic window  
**Branch:** `feat/tauri-scaffold`  
**Next Action:** Spawn Friday to create Tauri scaffold

### T0011: Mission Control Dashboard UI
**Owner:** Jarvis (main)  
**Status:** Queued  
**Scope:** Build internal Mission Control dashboard (read-only) - shows tasks, status, branches, commits  
**Risks:** None (internal tooling)  
**Files:** `src/mission-control/`, integration with Tauri scaffold  
**Verification:** Dashboard accessible at localhost:PORT, renders mission_control.md data  
**Branch:** `feat/mission-control-ui`  
**Next Action:** Create simple web dashboard that reads mission_control.md

---

## Next (Queued)

---

## Blocked (Needs Vir)

*Nothing blocked yet.*

---

## Shipped (Done)

### T0001: Repo Skeleton + Mission Control Setup ✓
**Completed:** 2026-02-10 19:10 SGT  
**Owner:** Jarvis (main)  
**Output:** Git repo initialized, Mission Control files created, task breakdown defined  
**Branch:** `main`  
**Commit:** `cef68f3` - "chore: initialize repo skeleton with Mission Control"

### Phase A: Requirements Gathering ✓
**Completed:** 2026-02-10  
**Owner:** Jarvis (main)  
**Output:** Product requirements, UX bar definition, Phase B plan created  
**Commits:** N/A (pre-repo)

---

## Subagent Roster

| Agent ID | Role | Specialty | When to Invoke |
|----------|------|-----------|----------------|
| main | Mission Control, integrator, architect | Coordination, architecture, integration | Always active |
| loki | Content & microcopy | UX writing, error messages, user-facing text | UI copy, documentation, user guides |
| fury | Research & validation | Threat modeling, competitive analysis, QA strategy | Security analysis, user research synthesis |
| friday | Implementation | Code, build systems, testing infrastructure | Heavy lifting on features, refactors, test harness |

*Note: Subagents produce artifacts → main integrates → Vir approves.*

---

## Day Mode vs Night Mode

**Day Mode (Vir active):**
- Propose next 1-3 tasks
- Tight loops with checkpoints
- Vir chooses priorities

**Night Mode (Vir offline):**
- Safe, unblocked, low-risk tasks only (docs, refactors, tests, UI polish, bugfixes)
- No architecture changes or destructive rewrites
- Leave Morning Handoff summary

**Current Mode:** Day

---

## Branching/Commit Conventions

**Branch naming:**
- Feature: `feat/short-description`
- Fix: `fix/short-description`
- Docs: `docs/short-description`
- Chore: `chore/short-description`

**Commit messages:**
- Format: `<type>: <subject>` (50 chars max)
- Types: `feat`, `fix`, `docs`, `chore`, `refactor`, `test`
- Example: `feat: add intent bar UI component`

**Merge strategy:**
- Small, incremental commits
- Never push without Vir approval
- Prepare for GitHub but don't create repo yet

---

## Quick Stats

- **Total Tasks Defined:** 11
- **In Progress:** 4 (T0002, T0006, T0007, T0011)
- **Queued:** 3 (T0003, T0004, T0005)
- **Blocked:** 0
- **Shipped:** 2 (Phase A, T0001)

---

## Key Decisions (2026-02-10)

**MVP Notification Channel:** Email-first (draft mode by default, explicit opt-in for send)  
**LLM Providers:** Multi-provider by design (OpenAI + Anthropic out of box, extensible registry)  
**Mission Control:** Must be actual dashboard UI, not just markdown  
**Day Mode Path:** Hybrid (T0002 UX + T0006 Token + T0007 Tauri scaffold)
