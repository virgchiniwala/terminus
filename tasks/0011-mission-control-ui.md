# T0011: Mission Control Dashboard UI

**Owner:** Jarvis (main)  
**Status:** Queued  
**Branch:** `feat/mission-control-ui`  
**Created:** 2026-02-10

---

## Goal

Build internal Mission Control dashboard (read-only) so Vir can see task status, progress, branches, and commits in a UI instead of reading markdown files.

---

## Constraints

- Internal tool (not end-user feature)
- Read-only for MVP (no task editing via UI)
- Should parse `mission_control.md` and render it cleanly
- Simple web interface (can integrate with Tauri scaffold or run standalone)
- Must show: Now/Next/Blocked/Shipped sections, task cards, branch status, commit links

---

## Plan

1. Create simple web dashboard (React or vanilla HTML)
2. Parse `mission_control.md` and `tasks/*.md` files
3. Render task cards with status, owner, files, branch, verification steps
4. Add refresh button (re-reads markdown files)
5. Optionally integrate with Tauri scaffold (internal route)
6. Add link to GitHub commits when repo is pushed

---

## Acceptance Criteria

- [ ] Dashboard accessible (localhost:PORT or Tauri internal route)
- [ ] Renders all sections: Now, Next, Blocked, Shipped
- [ ] Each task card shows: ID, title, owner, status, scope, files, branch, next action
- [ ] Auto-refresh or manual refresh button
- [ ] Clean, minimal design (matches Ive-level bar)
- [ ] Can run independently of main app (dev convenience)

---

## Progress Log

- 2026-02-10 21:40: Task created

---

## Commits

- (pending)
