# taskcraft project-management spec (Tiers 5-10)

Companion to [TIERED_PLAN.md](TIERED_PLAN.md). The tier plan says what each tier delivers; this file says how the data changes and how "done" is tested. Written 2026-11-14 against the code at v0.1.4 (`taskcraft-store` SCHEMA_VERSION 1).

## 0. Findings from the current code

These change the plan. Each one is fixed in the tier that first needs it.

1. **No migration path.** `Tracker::init` runs `CREATE TABLE IF NOT EXISTS` and rejects any `schema_version` other than 1. A v2 build would refuse every existing DB. A migration runner is the first item of Tier 5.
2. **Assignee is free text.** `issues.assignee TEXT`. teamcraft identifies people by employee `code`. Capacity per person, workload and Tier 10 users all need one identity, so Tier 5 adds a `people` table.
3. **Events must belong to an issue.** `events.issue_id NOT NULL`, `what` is free text. Sprint, baseline and import events have no issue. The table is rebuilt in Tier 5 with a nullable issue, a project, a typed `kind` and a JSON `payload`. `what` stays as the display line.
4. **Two units.** `issues.estimate` is story points (0-1000); teamcraft capacity is minutes. Planning against capacity needs minutes. Tier 6 adds `estimate_minutes` and a per-project `minutes_per_point`; with neither set, the commitment check runs on points only and says so.
5. **`leave.approved` is not needed.** teamcraft's `team_capacity` already subtracts AT holidays and approved leave per employee, and its `capacity.changed` payload carries the `employees` array, which taskcraft already stores (in `meta`, key `team_capacity:<week>`). What is missing is that teamcraft only emits on a manual `emit=true`. Fix in teamcraft: re-emit `capacity.changed` for every week a leave decision touches. The `leave.approved` event is dropped from the plan.
6. **`time.billable` is not needed.** billcraft's inbox already consumes `timesheet.approved` with `{ project, hours, rate_cents }`. taskcraft emits that kind instead of inventing a new one.
7. **Capacity lives in `meta`.** It moves to a real table in Tier 6 (migrated, not dropped).

## 1. Migration runner (Tier 5, first item)

- `MIGRATIONS: &[(i64, &str)]`, ordered, each applied in one transaction. `schema_version` in `meta` is updated in the same transaction.
- Before the first migration on a file DB, `VACUUM INTO '<db>.v<old>.bak'`. The backup path is printed and kept.
- A DB newer than the build is still refused, with the message as today.
- Table rebuilds (SQLite cannot change CHECK or NOT NULL in place) follow the 12-step pattern: `PRAGMA foreign_keys=OFF`, create `x_new`, copy, drop, rename, recreate indexes, `PRAGMA foreign_key_check`, `foreign_keys=ON`.
- Tests: open a v1 fixture DB (committed under `tests/fixtures/v1.db`, produced by `demo-seed` at v0.1.4) and migrate to latest; issue count, numbers, relations, commits and events are unchanged; a second open is a no-op; a DB stamped with version 99 is refused.

**As built (2026-11-15).**
- The fixture lives at `crates/taskcraft-store/tests/fixtures/v1.db`, with its recipe in the README next to it, and is committed despite the `*.db` ignore rule.
- Migrations are Rust functions, not `&str`, because v2 needs the handle rule while backfilling.
- `templates.kind` and `templates.priority` got the same CHECK constraints as `issues`; the spec had none.
- Handle rule (`person_handle`): lower-case ASCII, umlauts and ß spelled out, other characters become `-`, at most 32 characters, `user-<h>` when shorter than 2. A name matches an existing person by handle or case-insensitive display name.

## 2. Schema per tier

Types follow the existing schema: dates `TEXT 'YYYY-MM-DD'`, timestamps RFC 3339 `TEXT`, money in integer cents, durations in integer minutes or working days. JSON columns are allowed only where noted.

### v2 - Tier 5 (collaboration and structure)

```sql
CREATE TABLE people (
  id INTEGER PRIMARY KEY,
  handle TEXT NOT NULL UNIQUE,            -- lower-case, 2-32 chars
  display_name TEXT NOT NULL,
  teamcraft_code TEXT UNIQUE,             -- NULL for agents / externals
  active INTEGER NOT NULL DEFAULT 1
);
-- backfill: one person per DISTINCT issues.assignee, handle = lower(trim(assignee))
ALTER TABLE issues ADD COLUMN assignee_id INTEGER REFERENCES people(id);
-- backfill assignee_id, then DROP COLUMN assignee (bundled SQLite >= 3.35)
ALTER TABLE issues ADD COLUMN parent_id INTEGER REFERENCES issues(id);
ALTER TABLE issues ADD COLUMN kind TEXT NOT NULL DEFAULT 'task'
  CHECK (kind IN ('epic','story','task','bug','subtask'));
ALTER TABLE issues ADD COLUMN version INTEGER NOT NULL DEFAULT 1;  -- used by Tier 10, added now
ALTER TABLE milestones ADD COLUMN version INTEGER NOT NULL DEFAULT 1;

CREATE TABLE comments (
  id INTEGER PRIMARY KEY,
  issue_id INTEGER NOT NULL REFERENCES issues(id),
  author_id INTEGER NOT NULL REFERENCES people(id),
  body TEXT NOT NULL,                     -- markdown, max MAX_TEXT
  created_at TEXT NOT NULL,
  edited_at TEXT,
  deleted INTEGER NOT NULL DEFAULT 0,     -- soft delete; body kept for history
  version INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE mentions (comment_id INTEGER NOT NULL REFERENCES comments(id),
  person_id INTEGER NOT NULL REFERENCES people(id), PRIMARY KEY (comment_id, person_id));
CREATE TABLE watchers (issue_id INTEGER NOT NULL REFERENCES issues(id),
  person_id INTEGER NOT NULL REFERENCES people(id), PRIMARY KEY (issue_id, person_id));
CREATE TABLE attachments (
  id INTEGER PRIMARY KEY,
  issue_id INTEGER NOT NULL REFERENCES issues(id),
  path TEXT NOT NULL,                     -- must be under an --allow-path root
  sha256 TEXT NOT NULL,
  bytes INTEGER NOT NULL,
  added_by INTEGER REFERENCES people(id),
  added_at TEXT NOT NULL,
  UNIQUE (issue_id, sha256)
);
CREATE TABLE custom_fields (
  id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL REFERENCES projects(id),
  name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('text','number','date','select','multiselect')),
  UNIQUE (project_id, name)
);
CREATE TABLE custom_field_options (field_id INTEGER NOT NULL REFERENCES custom_fields(id),
  option TEXT NOT NULL, PRIMARY KEY (field_id, option));
CREATE TABLE issue_field_values (
  issue_id INTEGER NOT NULL REFERENCES issues(id),
  field_id INTEGER NOT NULL REFERENCES custom_fields(id),
  text_value TEXT, num_value REAL, date_value TEXT,
  PRIMARY KEY (issue_id, field_id)
);
CREATE TABLE issue_field_multi (issue_id INTEGER NOT NULL, field_id INTEGER NOT NULL,
  option TEXT NOT NULL, PRIMARY KEY (issue_id, field_id, option));
CREATE TABLE templates (
  id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL REFERENCES projects(id),
  name TEXT NOT NULL, kind TEXT NOT NULL, priority TEXT, title_prefix TEXT NOT NULL DEFAULT '',
  body TEXT NOT NULL DEFAULT '', UNIQUE (project_id, name)
);
CREATE TABLE template_labels (template_id INTEGER NOT NULL REFERENCES templates(id),
  label TEXT NOT NULL, PRIMARY KEY (template_id, label));
CREATE TABLE saved_views (
  id INTEGER PRIMARY KEY,
  owner_id INTEGER REFERENCES people(id), -- NULL = shared with everyone
  name TEXT NOT NULL,
  query_json TEXT NOT NULL,               -- JSON allowed: filter spec, validated against a schema
  UNIQUE (owner_id, name)
);
-- events rebuilt (12-step): issue_id nullable, plus project_id, kind, payload
CREATE TABLE events (
  id INTEGER PRIMARY KEY,
  project_id INTEGER REFERENCES projects(id),
  issue_id INTEGER REFERENCES issues(id),
  at TEXT NOT NULL, actor TEXT NOT NULL,
  kind TEXT NOT NULL,                     -- e.g. issue.moved, comment.added, sprint.started
  payload TEXT,                           -- JSON allowed
  what TEXT NOT NULL                      -- human line, as today
);
-- backfill: project_id from the issue, kind = 'legacy', payload NULL
```

Hierarchy rules (enforced in the store, not the UI): `parent_id` cannot form a cycle; an epic has no parent; a subtask's parent is not a subtask; parent and child share a project.

### v3 - Tier 6 (iterations)

```sql
CREATE TABLE sprints (
  id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL REFERENCES projects(id),
  name TEXT NOT NULL, goal TEXT NOT NULL DEFAULT '',
  start_date TEXT NOT NULL, end_date TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('planned','active','closed')),
  closed_at TEXT,
  version INTEGER NOT NULL DEFAULT 1,
  UNIQUE (project_id, name),
  CHECK (end_date >= start_date)
);
CREATE UNIQUE INDEX one_active_sprint ON sprints(project_id) WHERE state = 'active';
ALTER TABLE issues ADD COLUMN sprint_id INTEGER REFERENCES sprints(id);
ALTER TABLE issues ADD COLUMN estimate_minutes INTEGER
  CHECK (estimate_minutes IS NULL OR estimate_minutes BETWEEN 0 AND 600000);
ALTER TABLE projects ADD COLUMN minutes_per_point INTEGER;  -- NULL = points-only planning
CREATE TABLE sprint_scope (                 -- drives burndown and scope-change markers
  sprint_id INTEGER NOT NULL REFERENCES sprints(id),
  issue_id INTEGER NOT NULL REFERENCES issues(id),
  added_on TEXT NOT NULL,                   -- date
  removed_on TEXT,
  points_at_add INTEGER,
  PRIMARY KEY (sprint_id, issue_id, added_on)
);
CREATE TABLE capacity_weeks (
  week TEXT NOT NULL,                       -- 'YYYY-Www'
  teamcraft_code TEXT NOT NULL,
  available_minutes INTEGER NOT NULL,
  working_days INTEGER NOT NULL,
  received_at TEXT NOT NULL,
  PRIMARY KEY (week, teamcraft_code)
);
-- migrate every meta 'team_capacity:%' row into capacity_weeks, then delete those meta rows
```

Capacity joins to people through `people.teamcraft_code`. Codes with no person are listed in `task_sprint_report` as "unmapped capacity", never silently dropped. Daily capacity of a person in a week = `available_minutes / working_days`.

### v4 - Tier 7 (scheduling)

```sql
ALTER TABLE issues ADD COLUMN start_date TEXT;
ALTER TABLE issues ADD COLUMN duration_days INTEGER CHECK (duration_days IS NULL OR duration_days BETWEEN 0 AND 2000);
ALTER TABLE issues ADD COLUMN schedule_mode TEXT NOT NULL DEFAULT 'auto' CHECK (schedule_mode IN ('auto','manual'));
-- relations rebuilt (12-step) to widen the CHECK and add lag
CREATE TABLE relations (
  source_id INTEGER NOT NULL REFERENCES issues(id),
  target_id INTEGER NOT NULL REFERENCES issues(id),
  kind TEXT NOT NULL CHECK (kind IN ('blocks','starts_with','finishes_with','relates_to','duplicates')),
  lag_days INTEGER NOT NULL DEFAULT 0 CHECK (lag_days BETWEEN -365 AND 365),
  PRIMARY KEY (source_id, target_id, kind),
  CHECK (source_id <> target_id)
);
CREATE TABLE calendar_exceptions (          -- company closures / extra working days
  project_id INTEGER REFERENCES projects(id), -- NULL = all projects
  date TEXT NOT NULL,
  working INTEGER NOT NULL,                 -- 0 = closed, 1 = forced working day
  note TEXT NOT NULL DEFAULT '',
  PRIMARY KEY (project_id, date)
);
CREATE TABLE baselines (id INTEGER PRIMARY KEY, project_id INTEGER NOT NULL REFERENCES projects(id),
  name TEXT NOT NULL, taken_at TEXT NOT NULL, UNIQUE (project_id, name));
CREATE TABLE baseline_rows (baseline_id INTEGER NOT NULL REFERENCES baselines(id),
  issue_id INTEGER NOT NULL REFERENCES issues(id), start_date TEXT, finish_date TEXT,
  PRIMARY KEY (baseline_id, issue_id));
```

Relation meaning: `blocks` = finish-to-start, `starts_with` = start-to-start, `finishes_with` = finish-to-finish. All three are cycle-checked as one graph. Schedule results (ES, EF, LS, LF, float) are computed on demand and not stored. Only baselines are stored.

Duration used by the scheduler, first match wins: `duration_days`; else `ceil(estimate_minutes / daily capacity of the assignee)` for the start week; else 1 day with a `duration_defaulted` flag in the output.

### v5 - Tier 8 (portfolio and actuals)

```sql
CREATE TABLE programs (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE);
CREATE TABLE program_projects (program_id INTEGER NOT NULL REFERENCES programs(id),
  project_id INTEGER NOT NULL REFERENCES projects(id), PRIMARY KEY (program_id, project_id));
ALTER TABLE projects ADD COLUMN customer_account TEXT;   -- gaincraft account id
ALTER TABLE projects ADD COLUMN billable INTEGER NOT NULL DEFAULT 0;
ALTER TABLE projects ADD COLUMN rate_cents INTEGER;      -- blended hourly rate, v1
CREATE TABLE time_actuals (
  issue_id INTEGER NOT NULL REFERENCES issues(id),
  source TEXT NOT NULL CHECK (source IN ('teamcraft','import')),
  source_id TEXT NOT NULL,                  -- teamcraft entry id / import row id
  work_date TEXT NOT NULL,
  minutes INTEGER NOT NULL CHECK (minutes > 0),
  billed INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (source, source_id)
);
CREATE TABLE health_rules (
  project_id INTEGER REFERENCES projects(id),  -- NULL = default
  slip_days_amber INTEGER NOT NULL DEFAULT 3, slip_days_red INTEGER NOT NULL DEFAULT 10,
  blocked_amber INTEGER NOT NULL DEFAULT 3,   blocked_red INTEGER NOT NULL DEFAULT 8,
  overload_pct_amber INTEGER NOT NULL DEFAULT 110, overload_pct_red INTEGER NOT NULL DEFAULT 130,
  PRIMARY KEY (project_id)
);
```

`time_actuals` has no person column on purpose (ArbVG posture, as in gaincraft). The source of truth for who worked is teamcraft.

### v6 - Tier 9 (migration)

```sql
CREATE TABLE import_runs (id INTEGER PRIMARY KEY, source TEXT NOT NULL, file_sha256 TEXT NOT NULL,
  started_at TEXT NOT NULL, dry_run INTEGER NOT NULL, created INTEGER NOT NULL, updated INTEGER NOT NULL,
  dropped_json TEXT NOT NULL);              -- JSON allowed: field -> count + reason
CREATE TABLE import_map (source TEXT NOT NULL, external_id TEXT NOT NULL,
  issue_id INTEGER NOT NULL REFERENCES issues(id), PRIMARY KEY (source, external_id));
```

### v7 - Tier 10 (team sharing, option A)

```sql
CREATE TABLE users (
  id INTEGER PRIMARY KEY,
  person_id INTEGER NOT NULL UNIQUE REFERENCES people(id),
  role TEXT NOT NULL CHECK (role IN ('owner','member','viewer')),
  active INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE tokens (
  id INTEGER PRIMARY KEY,
  user_id INTEGER REFERENCES users(id),
  agent_name TEXT,
  hash TEXT NOT NULL UNIQUE,                -- argon2id of the raw token
  created_at TEXT NOT NULL, last_used_at TEXT, revoked_at TEXT,
  CHECK ((user_id IS NULL) <> (agent_name IS NULL))
);
```

## 3. Acceptance criteria (tests that close each tier)

Every criterion is a test through `call_tool` unless marked UI or CLI. "Rejected" means a typed error with a message, the DB unchanged and nothing written to history.

### Tier 5
1. Migration: the v1 fixture migrates to v2. All issues, relations, commits and events are kept (counts and issue keys compared before and after). Each distinct old assignee becomes one person.
2. `task_comment_add` on BC-5 with body `"ping @ana"` creates the comment, one mention, adds `ana` as a watcher, and writes a `comment.added` event with actor.
3. Mention of an unknown handle: the comment is saved and the result lists `unknown_mentions: ["bob"]`.
4. Editing a comment keeps `created_at`, sets `edited_at`, bumps `version`; deleting is soft and the body stays readable in history.
5. Attachment outside every `--allow-path` root is rejected. Inside a root: stored with the correct SHA-256 and size. The same file twice is rejected as a duplicate.
6. Hierarchy: epic E, story S under E, subtask T under S works. T as parent of E (cycle), subtask under subtask, and a cross-project parent are each rejected.
7. Rollup: E shows points and percent done from all descendants. Fixture: S=5 points done, T=3 points open, gives E 8 points and 62.5 % (rounded to 63 in UI).
8. Custom field `select` with options {A, B}: setting C is rejected; setting A then filtering by it returns only that issue.
9. Template "bug" creates an issue with its kind, priority, prefix, body and labels; explicit arguments override template values.
10. Saved view round-trip: a stored query returns exactly the issues `task_issue_list` returns for the same filters.
11. UI: comment thread in the inspector, epic swimlane, "My work" lists assigned + mentioned + watched without duplicates.

### Tier 6
1. Two sprints set to `active` in one project: the second start is rejected (unique index).
2. Burndown fixture: 5-day sprint, 20 points committed. Day 2 closes 5, day 3 adds a 3-point issue, day 4 closes 8, day 5 closes 10. Remaining at end of each day: `[20, 15, 18, 10, 0]`. One scope-change marker on day 3.
3. Velocity of that sprint = 23 (closed points, including the added issue; canceled issues excluded).
4. Commitment check: person with `available_minutes` 1920 in the week, committed `estimate_minutes` 2100 gives warning `over_by_minutes: 180`. The plan is still saved.
5. Points-only project (`minutes_per_point` NULL, no `estimate_minutes`): check reports `mode: "points_only"` and makes no capacity claim.
6. Close: unfinished issues go to the chosen next sprint or the backlog; each move is one history event; `sprint_scope.removed_on` is set.
7. Capacity migration: three `team_capacity:*` meta rows become `capacity_weeks` rows with identical minutes; meta rows gone. An unmapped teamcraft code appears under `unmapped_capacity`.
8. teamcraft side (separate repo, same tier): deciding a leave request re-emits `capacity.changed` for each week the leave touches; taskcraft drains it and the week's numbers change.

### Tier 7
Conventions for all fixtures: days are working-day indices; a task with ES 0 and duration 3 occupies days 0, 1, 2 and has EF 3 (exclusive). Finish-to-start lag 0 means the successor starts on day EF. Float = LS - ES.

**Fixture A, finish-to-start network.** Durations A 3, B 2, C 4, D 2, E 3. Links A->B, A->C, B->D, C->D, D->E (all `blocks`).

| Task | ES | EF | LS | LF | Float |
|---|---|---|---|---|---|
| A | 0 | 3 | 0 | 3 | 0 |
| B | 3 | 5 | 5 | 7 | 2 |
| C | 3 | 7 | 3 | 7 | 0 |
| D | 7 | 9 | 7 | 9 | 0 |
| E | 9 | 12 | 9 | 12 | 0 |

Project end 12. Critical path A, C, D, E.

**Fixture B, mixed links with lag.** Durations A 4, B 3, C 2, D 2. Links: A `starts_with` B lag 1; A `finishes_with` C lag 0; B `blocks` D lag 2; C `blocks` D lag 0.

| Task | ES | EF | LS | LF | Float |
|---|---|---|---|---|---|
| A | 0 | 4 | 0 | 4 | 0 |
| B | 1 | 4 | 1 | 4 | 0 |
| C | 2 | 4 | 4 | 6 | 2 |
| D | 6 | 8 | 6 | 8 | 0 |

Project end 8. Critical A, B, D. (C is pushed to ES 2 by the finish-to-finish link: EF must be at least A's EF of 4.)

**Fixture C, real calendar (Austria, Mon-Fri).** A starts Wed 2026-12-23, 3 days; B after A, 2 days; C after B, 1 day.
- No exceptions: A = Dec 23, Dec 24, Dec 28 (Dec 25 is a public holiday, Dec 26 is Saturday, also a holiday). B = Dec 29, Dec 30. C = Dec 31.
- With a company closure on Dec 24 (`calendar_exceptions`): A = Dec 23, Dec 28, Dec 29. B = Dec 30, Dec 31. C = Mon 2027-01-04 (Jan 1 is a public holiday, Jan 2-3 weekend).

**Fixture D, guards.**
- Adding C `blocks` A to fixture A is rejected as a cycle across all three dependency kinds.
- Setting B in fixture A to `manual` with start day 1 keeps B at day 1 and returns `violations: [{ issue: B, link: A->B, by_days: 2 }]`. The schedule is not moved.
- An issue with no duration and no estimate is scheduled as 1 day with `duration_defaulted: true`.

**Baselines and forecast.**
- Baseline on fixture A, then C's duration 4 -> 6: `task_baseline_diff` reports C, D, E slipped 2 days; milestone on E slips 2 days; one `milestone.slipped` event is emitted only if the project has `customer_account` set.
- Forecast returns both `by_schedule` and `by_velocity` dates, each with its inputs.

**UI.** Timeline renders fixture A with C red-highlighted path, today line, baseline ghost bars after the change above; dragging B to day 1 is refused with the dependency message; Ctrl+wheel zooms between day/week/month.

### Tier 8
1. `time.logged` with `{ entry_id, issue: "BC-5", work_date, minutes }` creates one `time_actuals` row; the same event delivered twice creates one row; an unknown issue key is rejected via `bus.reject`.
2. Estimate vs actual: BC-5 estimate 480 min, actuals 600 gives `variance_minutes: 120`, `variance_pct: 25`. Rolls up to sprint, milestone, project.
3. Health with default rules: 4 slip days and 2 blocked issues gives amber with reason `slip_days >= 3`; 11 slip days gives red. The rule text is returned with the status.
4. Workload: person with 1920 available minutes and 2496 assigned gives 130 % (red). `task_level_suggest` proposes moves but changes nothing until applied.
5. Billable project, rate 9000 cents: approving 6 h of actuals emits `timesheet.approved { project, hours: 6.0, rate_cents: 9000 }` to billcraft; rows are marked billed; a second approval of the same rows emits nothing.
6. No API returns minutes per person per issue (test asserts the field is absent from `task_actuals`).

### Tier 9 (criteria only; field mappings are written against real export files when the tier starts)
1. Dry run writes nothing (row counts before/after equal) and returns created/updated/dropped per field.
2. Re-running the same import updates in place through `import_map`; no duplicates.
3. Every source column is either mapped or listed in `dropped_json` with a reason.
4. MSPDI: export fixture A, import into an empty project, schedule equals fixture A.
5. `.ics` export of fixture C validates (RFC 5545 line folding, `DTSTART;VALUE=DATE`).
6. `task_report` with pdfcraft missing returns an error naming pdfcraft; the Markdown report is still written.

### Tier 10
1. Two clients load BC-5 at version 7; the first update succeeds (version 8); the second is rejected with `stale_version` and the current issue.
2. A viewer token can read and is rejected on every mutating command (one test per command family).
3. A revoked token is rejected; `last_used_at` updates on use.
4. `--bind 0.0.0.0` is refused unless `--allow-lan` is given. A public (non-RFC 1918, non-CGNAT, non-Tailscale) bind address is refused.
5. Events long-poll returns the changes after `since` in order and nothing older.
6. Host stopped: the client shows read-only within 5 s and all edit actions are disabled.
7. `taskcraft backup` produces a DB that opens and passes `PRAGMA integrity_check`.
8. Inbox drains are refused on a client and run on the host.

## 4. Bus events (corrected)

| From | To | Kind | Status |
|---|---|---|---|
| teamcraft | taskcraft | `capacity.changed` | Exists. Tier 6: teamcraft also emits it when leave is decided. |
| teamcraft | taskcraft | `time.logged` | New, Tier 8. teamcraft time entries gain an optional issue key. |
| taskcraft | gaincraft | `milestone.slipped` | New, Tier 7. Only for projects with `customer_account`. gaincraft drain needs a handler. |
| taskcraft | billcraft | `timesheet.approved` | Existing kind consumed by billcraft today. Tier 8 emits it. |

## 5. Build or reuse: scheduler and Gantt (checked 2026-11-14)

Searched crates.io / docs.rs for ready-made parts.

| Crate | What it is | Verdict |
|---|---|---|
| `gantt_chart` 3.1 | CLI that turns JSON5 into a static SVG chart; no dependency calculation (by its own README) | No. Output only, nothing interactive |
| `kuva` 0.2 (`GanttPlot`) | Scientific plotting lib with an SVG Gantt plot (bars, groups, milestones, now line, progress) | Maybe later for exported reports; not for the app |
| `egui_plot` 0.36 | General egui plotting | No. Bars could be faked, but no dependency arrows, drag or calendar axis |
| `cpm-rs` 0.1.7 | Basic CPM; last real work 2022; cycle check listed as future work | No |
| `cpm-planner` | CPM kernel for agent task planning, tokio + MCP server | No. Wrong shape, heavy |
| `utf8proj-solver` 0.9 / utf8proj 0.10 | CPM with FS/SS/FF/SF and lag, calendars, resource levelling; MIT OR Apache-2.0 | Closest match, but about two months old, one author, 2 stars, pulls `biodivine-lib-bdd` and `rust_decimal`. Not a production dependency |

No interactive Gantt widget exists for egui.

**Decision: build both, small and owned.**

- **Scheduler** in `taskcraft-core::schedule` (pure functions, no I/O): topological order over the one dependency graph, forward and backward pass on working-day indices, a calendar mapping index to date (`craft_biz::holidays` + `calendar_exceptions` + weekends), float and critical set. Estimated 400-600 lines plus tests. Fixtures A-D in section 3 are the spec.
- **Oracle check (optional, dev-only):** behind a `oracle-utf8proj` feature, a property test generates random DAGs (up to 40 tasks, mixed FS/SS/FF, lags -2..5) and compares ES/EF/LS/LF and the critical set with `utf8proj-solver`. Any disagreement is a bug in one of the two, investigated by hand against the fixtures. Never a runtime dependency. This follows the fleet oracle-loop pattern.
- **Gantt widget** as its own crate, `egui_gantt`, depending only on `egui` (plus an optional `chrono` feature for date axes). It is drawn with `egui::Painter`, and colours come from a `GanttStyle` struct, not from the fleet theme. `craft-biz-ui::gantt` is a thin wrapper that fills `GanttStyle` from the fleet high-contrast theme, so contractcraft (deadlines), hirecraft (pipelines) and teamcraft (leave calendar) get it the same way as `kanban` and `table`. Keeping fleet code out of the widget is what makes it publishable (section 6).

### Gantt widget layout

```
+--------------------------+-----------------------------------------------------------+
| [Day|Week|Month] [Today] [Baseline: v1 v] [Critical only] [Fit]          zoom 100 % |
+--------------------------+-----------------------------------------------------------+
| Task              Dur  % | Mo 21 Tu 22 We 23 Th 24 Fr 25 | Mo 28 Tu 29 We 30 Th 31 Fr 01|
|                          |                   |   (hatched holiday / closure)          |
| v Epic BC-1 Launch    12 | [=================================================]  rollup |
|   BC-2 Design          3 | [#####======]--+                                          |
|   BC-3 Build API       4 |               +-->[############]--+     (red = critical) |
|   BC-4 Docs            2 |               +-->[======]  . . float . .|                  |
|   BC-5 Release         2 |                                   +-->[=====]            |
|   <> Milestone M1        |                                              <> (diamond) |
+--------------------------+---------------------------|today|-------------------------+
```

- **Left pane:** tree with epic > issue > subtask, key, title, duration in working days, % done. Column widths are resizable and the pane can be collapsed. Selection is synced with the inspector drawer.
- **Right pane:** time axis in two header rows (month / day or week / ISO week). Weekends and holidays are hatched, company closures get a separate hatch, and the today line runs top to bottom.
- **Bars:** the filled part shows % done. Critical tasks use the theme's error colour plus a thicker outline, so they are not marked by colour alone. Milestones are diamonds. Epics are summary brackets. Baseline ghost bars sit under the live bar, and slip is shown as a thin hatched gap.
- **Arrows:** orthogonal routing. FS goes from the finish edge to the start edge, SS from start to start, FF from finish to finish. Lag is shown as a label on the arrow when it is not 0.
- **Interaction:**
  - Drag a bar body to move it and drag its right edge to change the duration, both snapping to working days. On drop, the change runs as a command. If it breaks a link, the result is shown as a red ghost and a toast names the link, and nothing is saved until confirmed. Manual-mode tasks may break links; that is recorded as a violation.
  - Drag from a bar's end handle to another bar to create an FS link (cycle-checked).
  - Double-click opens the inspector. Ctrl+wheel zooms the time axis, plain wheel scrolls rows, Shift+wheel scrolls time.
- **Accessibility and contrast:** fleet high-contrast rules hold in both themes (no light grey on white). Every bar has a hover tooltip with key, dates, float and why it is critical. A keyboard path covers everything: arrows move the selection, Alt+Left/Right shifts by one day, Alt+Shift+Left/Right changes the duration.
- **Performance target:** 2,000 rows and 5,000 links at 60 fps on Goliath. Only visible rows are drawn and arrows are culled by viewport.
- **Data contract:** the widget takes `GanttModel { rows, links, calendar, baseline, today }` and returns `GanttEvent` values (move, resize, link, select, open). It never touches the store, as with `kanban::Board`.
- **Tests:** the model and hit-testing get unit tests (bar geometry for fixture A at each zoom, arrow endpoints, snap to working day). `--smoke` renders fixture A off-screen once.

Effort for Tier 7 stays at 2-3 days: about 1 for the scheduler and 1.5-2 for the widget.

## 6. Publishing `egui_gantt` to crates.io

crates.io is Rust's equivalent of PyPI. No egui Gantt widget exists there today. The nearest are `egui_timeline` (media/DAW timeline) and `egui-timeline-widget` (GPL, egui 0.27). A web search on 2026-11-14 found no crate named `egui_gantt` or `egui-gantt`. That is not proof the name is free: run `cargo search egui_gantt` and check crates.io just before the first publish, because names are first come, first served.

**Repo layout.** Use a separate public repo, `sandraschi/egui_gantt`, not a crate inside craft-biz. Reasons: a published crate cannot have path-only dependencies, and CI plus trusted publishing are simpler for one small repo. Fleet repos use it as `egui_gantt = { path = "../egui_gantt", version = "0.1" }` while developing. Locally that resolves to the path; on crates.io it resolves to the published version.

**One-time setup**
1. Log in to crates.io with the `sandraschi` GitHub account and verify the email address.
2. Prefer Trusted Publishing (GitHub Actions OIDC, no stored token). Configure it in the crate's settings after the first publish, then switch on "Trusted Publishing only" so no API token can publish.
3. The first publish needs an API token: create it at crates.io/settings/tokens, scope it to publish-new for this crate name, run `cargo login`, publish, then revoke the token. `cargo login` stores it in `~/.cargo/credentials.toml`. It is a secret: never put it in a repo or script, and `cargo logout` removes it.

**Cargo.toml must have**: `name`, `version`, `edition`, `license = "MIT OR Apache-2.0"` (fleet default), `description`, `repository`, `readme`, `keywords` (e.g. `egui`, `gantt`, `timeline`, `scheduling`, `widget`; at most 5), `categories` (`gui`, `visualization`), `rust-version`. Use `include = ["src/**", "examples/**", "README.md", "LICENSE-*", "CHANGELOG.md"]` so screenshots and fixtures stay out of the package (limit 10 MB).

**Each release**
1. Tests, clippy `-D warnings`, fmt, `cargo doc` with no warnings, the example runs.
2. Bump `version` (SemVer; 0.x minor = breaking), add a CHANGELOG entry.
3. `cargo publish --dry-run`, then `cargo package --list` to check contents.
4. Tag `vX.Y.Z` on `main`. The GitHub Action publishes via trusted publishing (not using `pull_request_target` or `workflow_run` triggers, which trusted publishing refuses).
5. docs.rs builds the API docs automatically from the published crate.

**Rules that matter**
- Publishing is permanent: a version can never be overwritten or deleted, only yanked (yanking stops new users, existing lockfiles keep working). Never publish anything containing a secret.
- egui breaks its API in most minor releases. `egui_gantt` pins `egui = "0.37"` and releases a new minor version for each egui minor it follows. The README states which egui version each release targets, and CI tests against exactly that version.
- Publish only after taskcraft Tier 7 has used the widget for real. 0.1 then means "works in one real app", not a promise of a stable API.
- Fleet docs standard applies: README with a screenshot (hosted in the repo, excluded from the package), example `examples/basic.rs` rendering fixture A, CHANGELOG, LF, no em dashes.

**Effort:** about half a day on top of the widget (repo, CI workflow, docs, first publish).
