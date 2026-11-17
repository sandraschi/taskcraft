# Changelog

## Unreleased

- Epics, stories and subtasks (Tier 5). Issues have a kind (epic, story, task, bug, subtask) and an optional parent in the same project. `task_issue_reparent` / `taskcraft reparent BC-7 BC-11` (or `--detach`); `--kind` and `--parent` on `new`, `--kind` on `update`. Cycles, epics with a parent, anything under a subtask and cross-project parents are refused.
- `task_issue_show` returns the children and a rollup: points and percent done over all descendants (canceled excluded; by issue count when nothing has points).
- Desktop inspector: kind pill and selector, parent link with Set Parent / Detach, clickable children, rollup progress bar.

- Comments, @mentions and watchers (Tier 5). Tools `task_comment_add`, `task_comment_list`, `task_comment_edit`, `task_comment_delete`, `task_watch`, `task_person_list`, `task_person_add`; CLI `comment`, `watch`, `people`. Mentioning `@handle` makes that person a watcher; unknown handles are reported and nothing is created. Only the author edits or deletes; deleting is soft. Passing `version` refuses an edit if the comment changed meanwhile.
- `task_issue_show` now includes comments and watchers, and every history entry has its `kind`.
- Desktop inspector: comment thread with watchers, a post box and Delete on your own comments.

- Database schema v2 (Tier 5 start). Opening an existing v1 database upgrades it automatically after saving a copy as `<db>.v1.bak`, and prints or shows where the copy is. Databases from a newer build are refused and left untouched.
- Assignees are people now: each distinct name becomes one person (spelling variants such as `Joe Mocky` and `joe mocky` merge). Assigning by name works as before.
- History events have a type (`issue.moved`, `commit.linked`, ...) and a project; events from before the upgrade are marked `legacy`.
- New tables for comments, mentions, watchers, attachments, custom fields, templates and saved views. No tools use them yet.
- Tests: migration of a real v1 database (every issue, relation, commit, label, milestone and event kept), backup, no-op reopen, refusal of newer schemas, handle rules.

- Menu bar (Tier 4): Insert, Modify and Commands menus; File gets New Project (`Ctrl+N`), Export Issues as CSV (`Ctrl+E`) and Refresh (`F5`). Issue items are disabled until an issue is selected. All items run through one `menu_action` that reuses the existing command paths.
- File > Export Issues as CSV: UTF-8 with BOM, semicolon-separated, RFC 4180 quoting, so it opens directly in Excel with German regional settings. Correction: the 0.1.3 entry below claimed a CSV export in the table; there was none until now.
- `--smoke` now checks taskcraft's real menu bar instead of the empty generic bookend.
- Tests: every menu item has a handler, CSV quoting, Tofu Guard over all menu text (with a negative check), and an egui_kittest test for `switch_on_hover` and a menu command running through dispatch. New dependency `rfd` (save dialog), dev-dependency `egui_kittest`.

- `taskcraft-core::schedule`: CPM scheduler (Tier 7 core). Forward and backward pass with finish-to-start, start-to-start and finish-to-finish links and working-day lag; total float and critical set; manual tasks with violation reports; cycle, duplicate, unknown and self-dependency errors; `duration_for`; `AtCalendar` and `DayMap` to map working-day indices to Austrian calendar dates. 10 tests matching PM_SPEC fixtures A to D. Not wired to the store yet.
- Docs: Tier 7 notes the new `egui_gantt` repo (v0.1: display, calendar, selection; dragging still to do).
- Docs: Tier 4b, skills and in-app chat (fleet pilot of `CRAFT_SKILLS_AND_CHAT_STANDARD.md`): four planned skills, `skill://` resources, `task_skill_read`, `read_only` flags per tool, Window > Chat with `task_status` and the open issue as context. PRD, HANDOVER updated.
- Docs: scope extended from issue tracker to project management. TIERED_PLAN.md gains Tiers 5-10 (collaboration, sprints, timeline/CPM, portfolio and actuals, migration and reports, team-sharing decision gate). PRD Next and README honest list updated. No code changes.
- Docs: Tier 10 decided as option A (single-writer `taskcraft serve` host).
- Docs: new docs/PM_SPEC.md with code findings, schema migrations v2-v7, and test criteria per tier (CPM fixtures with hand-computed results, burndown, capacity and billing fixtures). Bus plan corrected: `leave.approved` and `time.billable` dropped in favour of the existing `capacity.changed` and `timesheet.approved`. Tier 4 checklist updated to the v0.1.4 state.
- Docs: PM_SPEC section 5. No suitable ready-made Gantt or scheduler crate, so we build our own scheduler in `taskcraft-core::schedule` and a reusable Gantt widget in `craft-biz-ui::gantt`. `utf8proj-solver` is used only as an optional dev-time oracle. Includes the widget layout and data contract.
- Docs: the Gantt widget becomes a standalone `egui_gantt` crate, intended for crates.io; publishing procedure in PM_SPEC section 6. HANDOVER.md rewritten for the project-management scope. mcd craft-fleet page gains a "Planned improvements" section.

## 0.1.4 - 2026-11-13

- The window menu is File, Edit, View, Window, and Help, above the existing top bar. View switches the five screens. `--smoke` checks the menu. Ctrl+K still opens the tool palette.

## 0.1.3 - 2026-11-10

- Replaces Linear, Jira, Asana, and GitHub Projects: Local-first single SQLite WAL database, zero SaaS subscriptions or vendor lock-in, automatic read-only git log commit linking, and hard DAG blocker enforcement.
- Desktop App (taskcraft-app): Complete native egui workstation upgrade.
  - Interactive Issue Inspector side drawer: in-place title/description editing, status switcher with dependency blocker warnings, priority and assignee pills, due date helpers, milestone selector, interactive label chips with instant removal/addition.
  - Relations & dependencies DAG management: visual blocker warnings with clickable issue links, instant relation unlinking, and add-relation form with cycle detection.
  - Kanban Board enhancements: column '+' button opens pre-filled New Issue modal, swimlanes (None, Priority, Assignee), live filter toolbar (text search, priority filter, assignee filter), double-click to open in inspector.
  - Multiple Views: Kanban Board, sortable Issues Table with CSV export, Milestone burn-up with progress bars and closing actions, Git commit sync with clickable issue references, and Project Audit Log event stream.
  - Global Shell: Ctrl+K MCP Command Palette with schema-generated forms, cross-app fleet Signals Dock, project switcher, New Project and New Milestone modals, demo seeder, and dark/light theme toggle.
  - Unit tests for tones, project/issue creation, and status moves.

## 0.1.2 - 2026-11-09

- Docs: HELP.md (task-oriented guide), docs/PRD.md (purpose, boundaries, gaps), docs/TOOLS.md generated by `taskcraft tools --markdown` and checked by a test; `just docs` regenerates it.
- CLI: `--help` ends with a typical-workflow block; `tools --markdown`.

## 0.1.1 - 2026-11-09

- Inbox accepts teamcraft `capacity.changed`: stored per ISO week, shown in `task_status`.

## 0.1.0 - 2026-11-09

- Tier 1: taskcraft-store (projects, KEY-n issues, milestones, cycle-checked relations, commit links, history), taskcraft-core (command bus, git log sync, bus inbox, demo seed), `taskcraft` binary (CLI with `--json`, MCP stdio, 17 tools).
- Blocker gate on status moves with explicit `force` override.
- 17 tests, including git sync against a real temporary repository.
