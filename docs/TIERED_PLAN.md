# TaskCraft - Tiered Execution Plan

**Parent Standard:** [`RUST_ARTCRAFT_STANDARD.md`](../../mcp-central-docs/standards/RUST_ARTCRAFT_STANDARD.md)  
**Menu Bar Standard:** [`CRAFT_MENU_BAR_STANDARD.md`](../../mcp-central-docs/standards/rust-artcraft/CRAFT_MENU_BAR_STANDARD.md)  
**Architecture Model:** 4-Tier ArtCraft Incremental Delivery  
**Status:** Active  

---

## Architecture Flow

```
Tier 1: Core Engine ──> Tier 2: MCP / CLI ──> Tier 3: Desktop UI ──> Tier 4: Menu Bar, Help & Packaging
(taskcraft-core)       (taskcraft-mcp)       (taskcraft-app)       (Hardened Workstation)
```

---

## Tier Specifications

### Tier 1 - Core Issue & Blocker Engine (`taskcraft-core`)
- [x] L1 `Command::dispatch` universal issue tracking execution bus.
- [x] SQLite WAL database schema for projects, issues (`KEY-n`), dependencies, and milestones.
- [x] Blocker DAG cycle detection algorithm.
- [x] Per-issue history tracking (agent, user, git, bus).
- [x] Read-only `git log` synchronization and mention closing keywords.
- [x] Ingest cross-app bus events (`ticket.escalated`, `contract.deadline`, `capacity.changed`).
- [x] `#![forbid(unsafe_code)]` and zero-panic clippy lints.

### Tier 2 - Headless Driver & FastMCP (`taskcraft-mcp`)
- [x] Unified CLI `taskcraft-cli` with `--json` output.
- [x] FastMCP stdio server communicating over JSON-RPC 2.0 (17 tools).
- [x] Issue export and milestone progression telemetry.
- [x] Headless `--smoke` verification without window initialization.

### Tier 3 - Desktop Workstation UI (`taskcraft-app`)
- [x] Immediate-mode `egui` 0.37 / `wgpu` workstation (`craft-biz-ui`).
- [x] Drag-and-drop Kanban board with workflow rule enforcement.
- [x] Swimlanes (priority, assignee, none), quick filters, and issue table view.
- [x] Interactive inspector drawer with in-place editing and blocker management.

### Tier 4 - Craft Menu Bar Standard, Help Suite & Hardened Verification
State checked against the code on 2026-11-14 (v0.1.4, `craft_biz_ui::bookend`):
- [x] Bookend menu bar: File, Edit, View, Window, Help. View switches the five screens (Board, Table, Milestones, Git sync, History) with check marks.
- [x] Help suite: the five dialogs (Command Palette, Keyboard Shortcuts, Project Files & Docs, Licenses & Notices, About) come from the shared bookend.
- [x] Ctrl+wheel and Ctrl +/- zoom, dark/light theme, settings persisted.
- [x] `--smoke` checks the menu spec and exits.
- [x] Insert, Modify and Commands menus through `Spec.middle` (2026-11-15). Every item goes through `menu_action`, which calls the same `execute_*` functions and `dispatch` as the buttons. What was built and what was left out on purpose is in PRD "Top Menu Bar Hierarchy".
- [x] File menu: New Project, Export Issues as CSV (new: UTF-8 with BOM, semicolons, for Excel in German locales), Refresh, Quit. The 0.1.3 CHANGELOG claimed a CSV export that did not exist; it does now. Edit items stay inert (no undo stack yet).
- [x] `switch_on_hover` verified on the real bar with an egui_kittest test: File open, hover Commands, File closes, Commands opens, a Commands item runs through dispatch.
- [x] Tofu Guard: test renders every menu title, item and hint with the craft fonts; a negative check proves the guard reports a missing glyph.
- [x] `--smoke` checks taskcraft's real bar (with Insert, Modify and Commands), not the empty generic bookend.
- [x] Release packaging: `scripts/release.ps1` ships `taskcraft.exe`, `taskcraft-cli.exe`, the setup exe, the mcpb bundle and SHA256SUMS as plain assets; the portable zip is only an extra. Verified from the dry-run output in `dist/` (2026-11-15 08:01). Not yet published as a GitHub release.
- [ ] Edit menu: Undo and Redo need an undo stack (inert today, says so in the hint).
- Note: the em dash in the `craft-biz-ui/src/bookend.rs` palette title was replaced with a hyphen on 2026-11-14. It was the only one in craft-biz.

---

### Tier 4b - Skills and in-app chat (fleet pilot, added 2026-11-14)
taskcraft is the pilot for [`CRAFT_SKILLS_AND_CHAT_STANDARD.md`](../../mcp-central-docs/standards/rust-artcraft/CRAFT_SKILLS_AND_CHAT_STANDARD.md). It does not touch the schema, so it can go before or alongside Tier 5. It depends on the shared crates (`craft-skills`, the `craft-ai-core` fixes, `craft-ai-ui`) and on `craft-biz` changes; the repo-local plan is in `craft-ai/docs/CHAT_AND_SKILLS.md`.
- [ ] Skills in `skills/`: `taskcraft-workflow` (projects, `KEY-n`, statuses, priorities, triage), `taskcraft-blockers` (`blocks` DAG, cycle check, `force`), `taskcraft-git-sync` (closing keywords, `--allow-path`, idempotence), `taskcraft-bus` (inbox kinds and what each creates). Each skill names only real `task_*` tools; size limits per the standard.
- [ ] MCP: `skill://taskcraft-*/SKILL.md` resources, `task_skill_read`; `read_only` set on every tool (read-only: status, project list, issue list/show, milestone list, board, skill read; not read-only: project create, issue create/update/move, relation add/remove, milestone create/close, git sync, inbox drain, demo seed).
- [ ] CLI: `taskcraft skills list|show|export`.
- [ ] Window > Chat (`Ctrl+J`): `ChatHost` with `status()` = `task_status`, `selection()` = the issue open in the inspector (`task_issue_show`).
- [ ] Help > Project Files & Docs: Skills tab.
- [ ] Definition of done: standard section 8, including "Why is BC-5 blocked?" answered from a read-only tool call naming BC-4 and BC-6, and "Move BC-5 to done" producing a confirm card and actor `chat:<provider>/<model>` after Allow.
- Effort: about 0.25 day in this repo once the shared crates exist (shared work about 2-2.5 days, standard section 7).

---

## Scope extension: project management (added 2026-11-14)

Schema changes per tier, code findings that changed this plan, and the test criteria that close each tier are in [PM_SPEC.md](PM_SPEC.md).

Tiers 1-4 make taskcraft a good issue tracker. A startup or small company that wants to cancel Jira, Asana, Monday or MS Project also needs planning: sprints, timelines, workload across projects, actuals against estimates, and a way to get its existing data in. Tiers 5-10 add that, in dependency order. Each tier closes the 6-gate DoD (tests through `call_tool`, `--json` CLI parity, `--smoke`, docs regenerated) before the next one starts.

Target after Tier 9: replaces Jira Software, Asana, ClickUp, Linear and Trello for a 1-10 person team. It covers the planning half of MS Project and Smartsheet, but not their enterprise resource pools. Tier 10 decides whether more than one person can use it at a time.

```
T5 Collaboration ──> T6 Iterations ──> T7 Timeline & CPM ──> T8 Portfolio & actuals ──> T9 Migration & reports ──> T10 Team sharing (decision gate)
```

### Tier 5 - Collaboration and work structure
- [x] Migration runner (2026-11-15): `taskcraft-store/src/migrate.rs`. One transaction per step with a foreign-key check, `VACUUM INTO <db>.v<old>.bak` before an existing file is changed (never overwrites an older backup), newer databases refused. New databases are created at v1 and migrated, so both paths run the same code. CLI, MCP (stderr) and app (toast) report the upgrade and the backup path.
- [x] Schema v2 in full (PM_SPEC section 2): `people`, hierarchy columns (`parent_id`, `kind`), `version` columns, comments, mentions, watchers, attachments, custom fields, templates, saved views. Tables only; the features below use them.
- [x] `people` table: free-text assignees backfilled (one person per distinct name, spelling variants merged). Assigning by name still works and finds or creates the person; `Issue.assignee` still returns the display name, so CLI, MCP and app output are unchanged.
- [x] Events table rebuilt: nullable issue, `project_id`, typed `kind` (`issue.created`, `issue.updated`, `issue.moved`, `relation.added`, `relation.removed`, `commit.linked`; old rows `legacy`), `payload`.
- [x] Criterion 1 test: real v1 fixture (`crates/taskcraft-store/tests/fixtures/v1.db`, made with the v0.1.4 CLI) migrates with every issue, relation, commit, label, milestone and event unchanged; 8 tests in `tests/migrate.rs`.
- [ ] Comments on issues (markdown body, author, edit history, actor recorded as for every other change).
- [ ] Attachments as confined file references (`--allow-path`), stored by path + SHA-256, never copied into the DB.
- [ ] `@mentions` and watchers; a per-user "my work" inbox view (assigned, mentioned, watched).
- [ ] Parent/child hierarchy: epic -> issue -> subtask, with rollup of points and progress. Cycle-checked like `blocks`.
- [ ] Issue templates per project (bug, feature, chore) and custom fields (text, number, date, single/multi select) stored as typed columns in a side table, not JSON blobs.
- [ ] Saved filters / views, sharable as a named query.
- [ ] Tools: `task_comment_add`, `task_comment_list`, `task_attach`, `task_watch`, `task_my_work`, `task_issue_reparent`, `task_template_*`, `task_field_*`, `task_view_*`.
- [ ] UI: comment thread in the inspector, epic swimlane, "My work" screen.

### Tier 6 - Iteration planning (sprints / cycles)
- [ ] Sprints per project: name, start, end, goal, state (planned -> active -> closed). One active sprint per project.
- [ ] Capacity from teamcraft: `capacity.changed` already carries per-employee available minutes after AT holidays and approved leave. It moves from `meta` into a `capacity_weeks` table and joins to people by teamcraft code. Unmapped codes are listed, never dropped.
- [ ] teamcraft change: re-emit `capacity.changed` for every week a leave decision touches (today it only emits on a manual `emit=true`).
- [ ] Units: `estimate` stays story points; new `estimate_minutes` and per-project `minutes_per_point` allow planning in minutes. With neither, the check runs points-only and says so.
- [ ] Commitment check: points or estimated hours committed vs capacity, warned when over, never silently blocked.
- [ ] Sprint close: unfinished issues move to the next sprint or the backlog. Each move is recorded in the history.
- [ ] Burndown per sprint, velocity over the last N sprints, scope-change markers (issues added after start).
- [ ] Tools: `task_sprint_create`, `task_sprint_plan`, `task_sprint_start`, `task_sprint_close`, `task_sprint_report`, `task_velocity`.
- [ ] UI: Backlog with drag-into-sprint, sprint header bar with a capacity gauge, Burndown screen.

### Tier 7 - Scheduling and timeline (Gantt)
- [ ] Start date + duration (working days) on issues and milestones; duration defaults from estimate and assignee capacity.
- [ ] Dependency types on the existing DAG: finish-to-start (from `blocks`), plus start-to-start and finish-to-finish with lag in days.
- [x] Scheduler core (2026-11-15): `taskcraft-core::schedule`, pure functions.
  - Forward and backward pass over FS/SS/FF with working-day lag (negative allowed).
  - Manual tasks keep their start and report violations; cycles across all link kinds are refused.
  - `duration_for` rules; `AtCalendar` (Mon-Fri minus `craft_biz` AT holidays, plus closures and extra days) and `DayMap` (index to date).
  - 10 tests, including PM_SPEC fixtures A, B, C (both calendar variants) and D, and the baseline slip.
- [ ] Scheduler wiring: per-person daily capacity and teamcraft leave in durations (needs v3 `capacity_weeks`), and running it from the store (needs v4 columns).
- [x] Critical path: total float per task and critical set in dependency order (in `schedule`).
- [ ] Critical issues highlighted on board and timeline (needs the wiring above).
- [ ] Baselines: snapshot the planned dates; slip = current vs baseline, per task and per milestone.
- [ ] Milestone forecast: date from the schedule and, separately, from velocity, both shown with how they were computed.
- [ ] Emits `milestone.slipped` (payload: milestone, baseline date, forecast date, cause issues) for the bus.
- [ ] Tools: `task_schedule`, `task_critical_path`, `task_baseline_set`, `task_baseline_diff`, `task_milestone_forecast`, `task_timeline` (JSON rows for any renderer).
- [ ] Gantt widget as a standalone `egui_gantt` crate (egui only) with a `craft-biz-ui::gantt` theme wrapper; publish to crates.io after it has been used in taskcraft (PM_SPEC sections 5-6). **Started 2026-11-15:** [sandraschi/egui_gantt](https://github.com/sandraschi/egui_gantt) v0.1 has the calendar, layout, styles, display, drag to move, resize and link (snapped to working days, with link-violation checks), keyboard, collapsible tree and resizable pane, with fixtures A and C in its example (41 tests, 10 of them headless input tests). `craft-biz-ui::gantt` gives it the suite colours. Left for taskcraft: the scheduler in `taskcraft-core::schedule`, and wiring events to commands (a move goes through the scheduler and the store, and the result is drawn next frame).
- [ ] UI: Timeline / Gantt screen in egui (bars, dependency arrows, today line, critical path in red, baseline ghost bars, drag to reschedule with a dependency check on drop). Zoom day/week/month, Ctrl+wheel.

### Tier 8 - Portfolio, workload and actuals
- [ ] Portfolio view across all projects: status, % done, forecast vs target, red/amber/green status from explicit rules (slip days, blocked count, over-capacity), with the rule shown on hover.
- [ ] Programs / initiatives: group projects; rollup of progress, dates and hours.
- [ ] Workload heatmap per person per week across projects (assigned estimate vs capacity). Levelling is advisory: it proposes moves and a human applies them.
- [ ] Actuals: teamcraft time entries carry an optional issue key and emit `time.logged`. taskcraft stores hours per issue and shows estimate vs actual per issue, sprint, milestone and project. Aggregate per issue only. No per-person productivity ranking (same ArbVG posture as gaincraft).
- [ ] Optional cost view: hours x a blended project rate, read-only, labelled as an estimate. Approved billable hours go to billcraft as `timesheet.approved` `{ project, hours, rate_cents }`. billcraft's inbox already consumes that kind. Opt-in per project; rows are marked billed so nothing is invoiced twice.
- [ ] Tools: `task_portfolio`, `task_program_*`, `task_workload`, `task_level_suggest`, `task_actuals`, `task_project_health`.
- [ ] UI: Portfolio screen, Workload heatmap, actuals columns in Issues Table.

### Tier 9 - Migration, integration and reporting
- [ ] Importers with a dry-run diff before writing: Jira (CSV and JSON export), Linear CSV, Asana CSV, Trello JSON, ClickUp CSV, GitHub issues (via gitops-mcp export or `gh` JSON). Every imported field is mapped or reported as dropped. No silent loss.
- [ ] MS Project XML (MSPDI) import and export, so schedules can round-trip with people who still use MS Project or ProjectLibre.
- [ ] iCalendar (.ics) export of milestones, sprint dates and due dates.
- [ ] Status report generator: weekly project / portfolio report as Markdown, PDF via pdfcraft headless. If pdfcraft is missing, the command returns a clear error and no fake PDF.
- [ ] Git sync on a schedule or file watcher (opt-in), still read-only.
- [ ] Tools: `task_import` (`source`, `path`, `dry_run`), `task_export_mspdi`, `task_export_ics`, `task_report`.

### Tier 10 - Team sharing (decided 2026-11-14: option A, single-writer host)
A 10-person company needs several people on the same board at once. SQLite over an SMB share is not safe, so one process owns the DB and everyone else talks to it. Options B (per-seat SQLite + shared event log) and C (stay single-seat) were considered and rejected.

- [ ] `taskcraft serve --port <P> --bind <addr>`: one host (e.g. Goliath) owns the SQLite WAL DB and is its only writer. The port is registered in `mcp-central-docs/operations/WEBAPP_PORTS.md` before the first commit that uses it.
- [ ] Wire protocol: the existing `Command::dispatch` bus as JSON over HTTP (`POST /cmd`, same request/response shapes as the MCP `call_tool` payloads), plus `GET /events?since=<id>` long-poll on the history table so clients refresh without polling the whole board.
- [ ] Client mode: `taskcraft-app --server <url>` uses the same egui screens with a remote `CommandSink` in place of the local store. The local mode stays the default and is unchanged.
- [ ] Concurrency: every mutating command carries the issue's `version`; a stale version is rejected with the current state and the UI shows "changed by X, reload". No silent last-writer-wins.
- [ ] Users and roles: owner, member, viewer. Per-user tokens (hashed in the DB, raw token in Windows Credential Manager on the client via `craft-biz::secrets`). The history `actor` becomes the real user; agents get their own named tokens.
- [ ] Network posture: binds to localhost by default; LAN or Tailscale only by explicit `--bind`. No public exposure and no Tailscale Funnel. TLS is left to Tailscale; plain HTTP is refused on non-private addresses.
- [ ] Offline: a client that loses the host goes read-only with a banner. No local edits queue in v1.
- [ ] Ops: `taskcraft backup` (SQLite online backup API) on the host, audit export, `--smoke` for server mode.
- [ ] Bus: cross-app inbox drains run on the host only.
- [ ] Tools: `task_user_*`, `task_token_*`, `task_server_status`. MCP keeps working locally on the host and through `--server` on clients.

### Effort (AI-assisted, realistic)

| Tier | Days |
|---|---|
| 5 Collaboration | 1-1.5 |
| 6 Iterations | 1-1.5 |
| 7 Timeline & CPM | 2-3 (the Gantt widget is most of it) |
| 8 Portfolio & actuals | 1.5-2 (includes the teamcraft `time.logged` change) |
| 9 Migration & reports | 1.5-2 |
| 10 Team sharing (option A) | 2-3 |

About 9-14 days for Tiers 5-9. Tier 4 (menu bar and Help suite) should close first. Data stays synthetic until a pilot team uses it, so grades stay capped at B, as in the suite plan.

### New bus events

| From | To | Kind | Effect |
|---|---|---|---|
| teamcraft | taskcraft | `capacity.changed` (existing) | Also emitted when leave is decided (Tier 6) |
| teamcraft | taskcraft | `time.logged` | Actual hours on the issue |
| taskcraft | gaincraft | `milestone.slipped` | Only for projects with a customer account; it opens a CTA (gaincraft needs a handler) |
| taskcraft | billcraft | `timesheet.approved` (existing kind) | Draft T&M invoice lines (opt-in per project) |

`leave.approved` and `time.billable` from the first draft are dropped. Existing events already carry that information (PM_SPEC section 0).
