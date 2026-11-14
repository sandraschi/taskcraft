# taskcraft PRD

## What it is

A local-first issue tracker for a 1-10 person team (Linear / Jira / Asana replacement for the small case), with git commit linking and inputs from the other back-office craft apps. Pure Rust, ArtCraft pattern. Suite plan: `BACKOFFICE_CRAFT_SUITE.md`, section 4.2.

## What this replaces

- **Linear / Jira / Asana / ClickUp**: Replaces multi-tenant cloud SaaS trackers with a local-first SQLite WAL database, removing vendor subscriptions, latency, and cloud lock-in.
- **GitHub Projects / Trello**: Replaces web-based Kanban boards with a native 60fps desktop GUI (`taskcraft-app`), cycle-checked blocker DAGs, and automatic local `git log` commit linking without cloud webhook tokens.

## Users

- Sandra and a small team planning work per repo or project.
- Agents over MCP: create and triage issues, move them, link commits. Every agent change is in the history with actor `agent`.

## Jobs

1. Keep one honest list of work per project, with priorities, estimates and due dates.
2. Make blockers impossible to ignore.
3. Connect work to commits without a hosted service.
4. Collect work that other apps discover (support escalations, contract deadlines).

## Today (v0.1.3)

- egui desktop workstation (`taskcraft-app`): interactive drag-and-drop Kanban board with workflow rule enforcement, swimlanes (priority, assignee, none), filter toolbar, issues table, full interactive inspector drawer with in-place editing, relations and blockers management, milestone burn-up cards, git commit sync history, Ctrl+K command palette, and cross-app bus signals dock.
- Projects with keys, `KEY-n` issues, statuses, priorities, points, labels, assignee, due date, milestones with progress.
- Relations with a cycle check on `blocks`; moving a blocked issue to in_progress / in_review / done needs `force`.
- Full per-issue history (agent, user, `git:<author>`, `bus:<app>`).
- `git-sync`: read-only `git log`, mention linking, explicit closing keywords, idempotent.
- Inbox: `ticket.escalated` (deskcraft) and `contract.deadline` (contractcraft) become issues; `capacity.changed` (teamcraft) is shown in status. All three verified end to end.
- 17 MCP tools ([TOOLS.md](TOOLS.md), generated and test-checked), CLI with board view.

## Boundaries

- No hosted sync, no GitHub/GitLab issue sync in v1 (gitops-mcp covers GitHub).
- Git is read-only; taskcraft never writes to a repo.
- No time tracking (teamcraft) and no customer communication (deskcraft).

## Gaps (honest)

- No comments or attachments on issues.
- No sprints / cycles; capacity is shown, not planned against.
- Git sync on demand only (no hook or watcher).
- Synthetic data only.

## Next

Scope extended 2026-11-14 from issue tracker to project management for a startup or small company. Full tier specs in [TIERED_PLAN.md](TIERED_PLAN.md), Tiers 5-10:

1. Tier 5: comments, attachments, mentions/watchers, epic -> issue -> subtask hierarchy, templates, custom fields, saved views.
2. Tier 6: sprints planned against teamcraft capacity (minus leave and AT holidays), burndown, velocity.
3. Tier 7: timeline / Gantt with start dates, durations, FS/SS/FF dependencies, working-calendar scheduler, critical path, baselines, milestone forecast, `milestone.slipped`.
4. Tier 8: portfolio across projects, programs, workload heatmap with advisory levelling, actual hours from teamcraft (`time.logged`), optional cost, and billable hours to billcraft as `timesheet.approved`.

Schema per tier, code findings and test criteria: [PM_SPEC.md](PM_SPEC.md).
5. Tier 9: importers (Jira, Linear, Asana, Trello, ClickUp, GitHub) with dry-run diff, MS Project XML round-trip, .ics export, PDF status reports.
6. Tier 10: team sharing, decided 2026-11-14 as option A: one host runs `taskcraft serve` as the only DB writer. Clients run the egui app with `--server`, and every change goes through the command bus over HTTP with version checks and per-user tokens. Binds to LAN or Tailscale only.

Target replacement set after Tier 9: Jira Software, Asana, ClickUp, Linear, Trello, plus the planning half of MS Project / Smartsheet. Not targeted: enterprise resource pools, portfolio finance, hosted multi-tenant use.

## UI/UX Architecture & Menu Bar Standard

In accordance with [`CRAFT_MENU_BAR_STANDARD.md`](../../mcp-central-docs/standards/rust-artcraft/CRAFT_MENU_BAR_STANDARD.md):
- **Pattern 2 (Viewport-Centric Craft Workstation):** Persistent Kanban board / issue grid viewport with swimlanes and collapsible detail drawer. Top quick button row acts as accelerators into the `View` menu. To add a new issue view or backlog board:
  1. Register a visibility toggle in the `View` or `Window` menu.
  2. Add an entry in the sidebar Project / Status toolbox.
  3. Wire the action directly into the universal `Command::dispatch` bus.
- **Top Menu Bar Hierarchy:**
  - `File`: New Project (`Ctrl+N`), Open Project (`Ctrl+O`), Open Recent ▸, Save (`Ctrl+S`), Export Issues / CSV (`Ctrl+E`), Close Project (`Ctrl+W`), Quit (`Ctrl+Q`).
  - `Edit`: Undo (`Ctrl+Z`), Redo (`Ctrl+Y`), Cut, Copy, Paste, Select All, Preferences (`Ctrl+,`).
  - `View`: Kanban Board, Issues Table, Backlog List, Milestone Burn-up, Git Sync History, Theme (`Dark`/`Light`).
  - `Insert`: Add Issue (`C` / `Ctrl+Shift+I`), Add Milestone, Add Issue Dependency (`Blocks`), Link Git Commit.
  - `Modify`: Move Issue Status, Change Assignee, Update Priority / Points, Edit Labels.
  - `Commands`: Run Git Log Sync, Drain Escalations Inbox, Check Blocker Cycles, Audit Capacity.
  - `Window`: Reset Layout, Board Viewport, Issue Inspector Drawer, Cross-App Bus Dock.
  - `Help` (Mandatory 5-Dialog Suite): `Command Palette...` (`Ctrl+K`), `Keyboard Shortcuts...` (`F1`), `Project Files & Docs...`, `Licenses & Notices...`, `About TaskCraft`.
- **Behavioral Mechanics:** Smooth `switch_on_hover` between top-level menus, zero-unsafe immediate-mode rendering, and headless `--smoke` parity.
