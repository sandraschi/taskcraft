# TaskCraft - Handover Document

**Last Updated**: 2026-11-10
**Current State**: v0.1.3 Tier 1 + L3 Desktop Workstation (Store + Core + CLI + MCP + egui 0.37 App operational)
**Primary Artifact**: [sandraschi/taskcraft](https://github.com/sandraschi/taskcraft) | `D:\Dev\repos\taskcraft`

---

## 1. What This Replaces

TaskCraft replaces hosted, multi-tenant cloud issue trackers for sovereign 1-10 person engineering teams:
- **Replaces**: Linear, Jira, Asana, Trello, ClickUp, and GitHub Projects.
- **Why**:
  - **Local-First & Zero Lock-in**: Single SQLite file with WAL journal (`taskcraft.db`), eliminating subscription fees, API rate limits, vendor downtime, and latency.
  - **Unified Surfaces**: Instant 60fps native desktop workstation (`taskcraft-app`), FastMCP stdio server (`taskcraft-mcp`), and CLI sharing one command bus.
  - **Native Git Integration**: Automatic, idempotent commit linking via read-only `git log` parsing without hosted webhooks or external tokens.
  - **Strict Workflow Invariants**: Cycle-checked DAG for `blocks` relations; hard gates preventing starting or closing blocked tasks without explicit audited override.
  - **Cross-App Fleet Signals**: Receives escalations from `deskcraft`, contract milestones from `contractcraft`, and capacity from `teamcraft`.

---

## 2. What Was Done

- Built `taskcraft-store` (L1): SQLite WAL storage for projects, `KEY-n` issues, milestones, DAG relations, commit links, and event audit history.
- Built `taskcraft-core` (L1): Command bus dispatching 17 verbs, git log sync (`gitsync.rs`), bus inbox, demo seeder.
- Built `taskcraft-mcp` (L2): Combined CLI and FastMCP server.
- Built `taskcraft-app` (L3): Complete native egui workstation:
  - **Kanban Board**: Drag-and-drop cards, live blocker rules, column `+` creation, priority/assignee swimlanes, and live filter toolbar.
  - **Interactive Inspector**: In-place title/description editing, status changer, priority/assignee pills, due date helpers, milestone selector, interactive label chips with instant removal/addition.
  - **Relations & Blockers**: DAG visualization with clickable issue links, instant relation unlinking, and add-relation form with cycle detection.
  - **Multiple Views**: Kanban Board, sortable Issues Table with CSV export, Milestone burn-up with progress bars, Git commit history with clickable issue references, and Project Audit Log event stream.
  - **Global Shell**: Ctrl+K Command Palette, cross-app Signals Dock, project switcher, New Project and New Milestone modals, and dark/light theme toggle.
- Added comprehensive unit tests in `taskcraft-app` and automated smoke check `--smoke`.

---

## 3. Deliberately Not Done (Boundaries & Honest Gaps)

- No hosted server or cloud synchronization (local-first SQLite).
- No GitHub/GitLab cloud issue import in v1 (gitops-mcp handles GitHub).
- Git sync is read-only pull-on-demand; taskcraft never writes to user git repositories.
- No comments or file attachments on issues yet (description and audit log only).
- No time-tracking or sprint capacity forecasting (owned by `teamcraft`).

---

## 4. Immediate Next Steps

1. Add issue comments thread in the inspector drawer.
2. Integrate with `devcraft.safe_commit_and_snapshot` to auto-link commits upon snapshot.
3. Add democraft recipes for automated blocker gate demonstrations.
