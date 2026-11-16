# TaskCraft - Handover Document

**Last Updated**: 2026-11-14
**Current State**: v0.1.4 code (Tiers 1-3 done, Tier 4 partly done). Docs-only session on 2026-11-14 widened the scope from issue tracker to project management (Tiers 5-10 planned, nothing built yet).
**Primary Artifact**: [sandraschi/taskcraft](https://github.com/sandraschi/taskcraft) | `D:\Dev\repos\taskcraft` (local commits on `main`, not pushed)

---

## 1. What this replaces

Today: Linear, Jira, Asana, Trello, ClickUp and GitHub Projects for the issue-tracking part of a 1-10 person team.
After Tier 9: also sprints, timeline / Gantt with critical path, portfolio and workload, i.e. the planning half of MS Project / Smartsheet.
After Tier 10: several people on one board through a single-writer host.

Not targeted: enterprise resource pools, portfolio finance, hosted multi-tenant use.

---

## 2. Where things stand

| Tier | Content | State |
|---|---|---|
| 1 | Store + core: projects, `KEY-n` issues, milestones, cycle-checked blockers, history, git sync, bus inbox | Done |
| 2 | CLI `--json` + MCP stdio, 17 tools | Done |
| 3 | egui workstation: Kanban, table, inspector, milestones, git, history, palette, signals dock | Done |
| 4 | Menu bar, Help, packaging | Done 2026-11-15 except Undo/Redo (no undo stack) and publishing a GitHub release. Full bar with Insert/Modify/Commands, CSV export, hover and tofu tests, smoke on the real bar, release script verified by dry run |
| 4b | Skills (bundled, served over MCP) + in-app chat with skills as preprompt; fleet pilot of `CRAFT_SKILLS_AND_CHAT_STANDARD.md` | Planned (needs shared crates first, about 2-2.5 days fleet-wide + 0.25 day here) |
| 5 | Collaboration: migration runner, people, comments, attachments, epics, templates, custom fields, saved views | Started 2026-11-15: migration runner and full schema v2 (people, typed events, all new tables), criterion 1 tested on a real v1 fixture. 2026-11-16: comments, mentions, watchers, people tools and inspector thread (criteria 2-4). Still to do: attachments, hierarchy and rollup, custom fields, templates, saved views, My work, epic swimlane |
| 6 | Sprints against teamcraft capacity, burndown, velocity | Planned |
| 7 | Scheduler (CPM, FS/SS/FF + lag, AT calendar), Gantt widget, baselines, forecast | Started. Scheduler core in `taskcraft-core::schedule` (10 tests, fixtures A-D), and Gantt widget in `egui_gantt` with the `craft-biz-ui::gantt` style (41 tests). Not wired: needs the v2-v4 schema (Tier 5 migration runner first) |
| 8 | Portfolio, programs, workload, actual hours, health, billing to billcraft | Planned |
| 9 | Importers (Jira, Linear, Asana, Trello, ClickUp, GitHub), MS Project XML, .ics, PDF reports | Planned |
| 10 | Team sharing, option A: `taskcraft serve` single-writer host, egui clients, tokens, version checks | Planned (decided 2026-11-14) |

Estimates (AI-assisted): Tiers 5-9 about 9-14 days, Tier 10 about 2-3 days.

---

## 3. Read these first

1. `docs/PRD.md`: what it is, boundaries, honest gaps, the Next list.
2. `docs/TIERED_PLAN.md`: tier checklists 1-10, effort, bus events.
3. `docs/PM_SPEC.md`: **the build spec for Tiers 5-10.** Section 0 lists code findings, section 2 the schema migrations v2-v7, section 3 the test criteria (hand-computed CPM fixtures), section 4 the corrected bus events, section 5 the scheduler and Gantt build decision, section 6 publishing the Gantt widget.
4. `mcp-central-docs/projects/craft-fleet-expansion/BACKOFFICE_CRAFT_SUITE.md`: suite context, section 4.2 and decision 7.5.

---

## 4. Traps found in the code (2026-11-14)

- `taskcraft-store` has `SCHEMA_VERSION = 1` and **refuses any other version**. The first Tier 5 commit must be the migration runner (PM_SPEC section 1), with a committed v1 fixture DB and a `VACUUM INTO` backup before migrating.
- `issues.assignee` is free text; teamcraft uses employee codes. Do not build capacity or workload before the `people` table exists.
- `events.issue_id` is NOT NULL. Sprint, baseline and import events need the rebuilt events table (v2).
- `estimate` is story points, capacity is minutes. Use `estimate_minutes` / `minutes_per_point`; never convert silently.
- teamcraft capacity (in `meta`, key `team_capacity:<week>`) already subtracts holidays and leave per employee. Do not add a `leave.approved` event. Instead, teamcraft must re-emit `capacity.changed` when leave is decided.
- billcraft already consumes `timesheet.approved { project, hours, rate_cents }`. Reuse that kind, do not invent `time.billable`.
- No usable Gantt or CPM crate exists (checked 2026-11-14). Build both; `utf8proj-solver` only as an optional dev-time oracle.

---

## 5. Immediate next steps (in order)

1. Tier 5 features on the v2 tables. Done: migration runner and schema v2 (2026-11-15), comments with mentions and watchers (criteria 2-4, 2026-11-16). Next: hierarchy and rollup (6-7), attachments (5), custom fields (8), templates (9), saved views (10), rest of the UI (11). New schema changes go into `crates/taskcraft-store/src/migrate.rs` as v3, v4, ...; never edit `SCHEMA_V1` or `tests/fixtures/v1.db`.
2. Tier 4b can go in parallel once craft-ai has `craft-skills`, the `complete()` / `chat()` fixes and `craft-ai-ui` (plan: `craft-ai/docs/CHAT_AND_SKILLS.md`). Write the four skills early: they help agents even before the chat exists.
3. Tier 5 item 1: migration runner + v1 fixture DB + tests (PM_SPEC section 1).
4. Tier 5 rest, test by test from PM_SPEC section 3.
5. Carried over from 2026-11-10: democraft recipes (`task/blocker-gate`, `task/git-close`); link commits on `devcraft` snapshot.

---

## 6. Rules for whoever builds this

- Single trunk (`main`), no agent branches left behind. LF line endings only. No em dashes anywhere.
- Every new MCP tool gets a test through `call_tool`. `docs/TOOLS.md` is regenerated with `just docs` and checked by a test.
- README, HELP.md, docs/PRD.md and CHANGELOG are updated in the same commit as the feature.
- egui: high contrast in both themes, Ctrl+wheel zoom, no light grey on white.
- Data stays synthetic until a pilot team uses it; grades capped at B until then.

---

## 7. Changed on 2026-11-14 (docs only, no code)

- `docs/TIERED_PLAN.md`: Tiers 5-10 added, Tier 4 checklist corrected to v0.1.4.
- `docs/PM_SPEC.md`: new.
- `docs/PRD.md`, `README.md`, `CHANGELOG.md` (Unreleased): updated.
- craft-biz: em dash in `craft-biz-ui/src/bookend.rs` palette title replaced with a hyphen (not rebuilt; string literal only).
- mcd: suite plan section 4.2, section 5 exception, decision 7.5, bus tables; `projects/craft-fleet/taskcraft.md`.
