# Mission Control — Personal AI OS

**Last Updated:** 2026-02-10 19:10 SGT  
**Current Mode:** Day (Vir active)  
**Branch:** main

---

## Now (In Progress)

*No tasks in progress yet. First 10 tasks are being defined.*

---

## Next (Queued)

### T0001: Repo Skeleton + Mission Control Setup
**Owner:** Jarvis (main)  
**Status:** Queued  
**Scope:** Initialize git repo, create Mission Control files, define first 10 tasks  
**Risks:** None (documentation only)  
**Files:** `mission_control.md`, `handoff.md`, `tasks/`, `docs/plan.md`, `.gitignore`, `README.md`  
**Verification:** Repo structure exists, Mission Control populated, first 10 tasks defined  
**Branch:** `main`  
**Next Action:** Create repo skeleton and task breakdown

---

## Blocked (Needs Vir)

*Nothing blocked yet.*

---

## Shipped (Done)

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

- **Total Tasks Defined:** 0
- **In Progress:** 0
- **Queued:** 1
- **Blocked:** 0
- **Shipped:** 1 (Phase A)
