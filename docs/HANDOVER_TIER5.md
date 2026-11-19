# Handover: finish Tier 5 (for Antigravity / OpenCode)

Written 2026-11-19 by Claude, who stopped here because of a weekly usage limit. Read this, then `HANDOVER.md`, `docs/TIERED_PLAN.md` (Tier 5) and `docs/PM_SPEC.md` (section 2 "v2", section 3 "Tier 5"). The spec's acceptance criteria are the definition of done.

## Repo rules (non-negotiable)

- Single trunk `main`. No feature branches, no leftover WIP. Commit to `main`, push when asked.
- LF line endings only. No em dash character anywhere (code, docs, commits): use a hyphen.
- No `unwrap`/`expect`/`panic` in non-test code (workspace lints deny them). Test files may allow them at the top, as `crates/taskcraft-store/tests/migrate.rs` does.
- Builds: `$env:CARGO_TARGET_DIR = 'M:\cargo-target\taskcraft'` (D: is short on space). Full gate before every commit:
  `cargo fmt --all --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace; cargo run -q -p taskcraft-app -- --smoke`
- A first app build takes over 80 s. If your shell tool times out, run the gate as a background PowerShell script that writes a log, and poll the log.
- After adding or changing a tool, regenerate `docs/TOOLS.md` (`just docs`, or `taskcraft-cli tools --markdown` written as UTF-8 without BOM, LF). The test `tools_doc_is_current` fails otherwise.
- Never edit `SCHEMA_V1` in `taskcraft-store/src/lib.rs` or `crates/taskcraft-store/tests/fixtures/v1.db`. Schema changes go into `taskcraft-store/src/migrate.rs` as a new version (v3, ...), and `SCHEMA_VERSION` goes up with it. Tier 5 needs NO schema change: every v2 table already exists.

## Check this first: possible gap from the last commit

`9e9db59` ("Follow Deutsch and English in the task menus", Cursor, 2026-11-18) made the menu labels bilingual via `bookend::t(de, "...", "...")`, with German whenever `settings.lang != "en"`. It also needs a newer `craft-biz` (`Spec.lang_de`, `Change.lang_de`). Nobody has run the gate since. Likely breakage:

- `crates/taskcraft-app/src/main.rs` tests look up English labels: `menu_bar_switches_on_hover_and_runs_commands` uses "New Project...", "Run Git Sync", "Seed Demo Data"; `every_menu_item_has_a_handler` looks for the item labelled "Seed Demo Data". If the test app defaults to German, these fail. Fix: set `app.settings.lang = "en".into()` in `memory_app()`, or match on action ids instead of labels. Do not weaken the assertions.
- `smoke_menus()` builds a `Spec` literal and may be missing `lang_de`.
- Only View and File items were translated in that diff. Check whether Insert, Modify and Commands (`middle_menus`) are still English-only, and translate them the same way if the intent is a fully bilingual bar.
- Tofu Guard test `menu_text_has_no_tofu` now also covers the German labels (umlauts must render). It should pass with the craft fonts; if not, that is a real bug.

Run the full gate before writing any Tier 5 code, and fix whatever is red first, in its own commit.

Also: `README.md` shows as modified with no content diff (line endings or stat only, from commit `f47885d`). Leave it, or normalise it in a separate commit.

## State of Tier 5

Done (all through `call_tool` tests in `crates/taskcraft-core/src/tests.rs`):

| Criterion | What | Where |
| --- | --- | --- |
| 1 | Migration runner, schema v2, people backfill, typed events | `taskcraft-store/src/migrate.rs`, `tests/migrate.rs` |
| 2-4 | Comments, @mentions, watchers, author-only edit and soft delete, version check | `taskcraft-store/src/collab.rs` |
| 6-7 | Kinds, parents, hierarchy rules, rollup | `taskcraft-store/src/hierarchy.rs` |
| 11 (part) | Inspector: comment thread, hierarchy section with rollup bar | `taskcraft-app/src/main.rs` (`render_inspector`) |

To do, in this order (each is one commit with tests and docs):

### 1. Attachments (criterion 5)

Table `attachments` exists (id, issue_id, path, sha256, bytes, added_by, added_at, UNIQUE(issue_id, sha256)).
- Store: new module `attachments.rs` (same pattern as `collab.rs`: `impl Tracker`, typed event `attachment.added` / `attachment.removed` with JSON payload via `crate::insert_event_with`).
- The file is referenced, never copied into the DB. Compute SHA-256 and size at attach time. `sha2` is probably not a dependency yet: add it to the workspace (or check `craft-biz` for an existing hash helper first).
- Path confinement happens in the core, like `ProjectCreate` does: `ctx.confinement.check(path, false)?` before calling the store. Criterion: a path outside every `--allow-path` root is rejected; inside a root it is stored with the correct SHA-256 and size; the same file twice on the same issue is rejected as a duplicate (UNIQUE constraint, but return a clear `Invalid` message, not a raw SQLite error).
- Tools: `task_attach {issue, path}`, `task_attachment_list {issue}`, `task_attachment_remove {attachment}`. `task_issue_show` gains `attachments`. CLI: `attach BC-5 <path>`, `attachments BC-5`. Note: in MCP mode the confinement is always on, on the CLI only with `--allow-path`.
- Test with a temp file and a `Confinement::roots(&[tempdir])` ctx (see how `Confinement` is built in `taskcraft-mcp/src/main.rs`).

### 2. Custom fields (criterion 8)

Tables: `custom_fields` (project_id, name, kind in text/number/date/select/multiselect), `custom_field_options`, `issue_field_values` (text_value, num_value, date_value), `issue_field_multi`.
- Store module `fields.rs`: define a field (with options for select kinds), set a value (validated: number parses, date is YYYY-MM-DD or DD.MM.YYYY via `craft_biz::at::parse_date` in the core, select value must be one of the options, multiselect each), clear a value, read values per issue.
- Filtering: extend `IssueFilter` with an optional `(field, value)` and the SQL in `Tracker::list` with an `EXISTS` on `issue_field_values` / `issue_field_multi`. Keep the current parameters positional order intact.
- Criterion: select field with options {A, B}; setting C is rejected; setting A then filtering by it returns only that issue.
- Tools (names from the plan): `task_field_define`, `task_field_list`, `task_field_set` (null clears), and a `field`/`field_value` filter on `task_issue_list`. `task_issue_show` gains `fields`.

### 3. Templates (criterion 9)

Tables `templates` (project_id, name, kind, priority, title_prefix, body) and `template_labels`. Note the CHECK constraints on kind and priority (they mirror `issues`).
- Store module `templates.rs`: create/list/delete a template.
- Creating an issue from a template: add `template: Option<String>` to `NewIssue`. Template values apply first; every explicit argument overrides (kind, priority, labels replace, description replaces body). Title = prefix + given title.
- Criterion: template "bug" creates an issue with its kind, priority, prefix, body and labels; explicit arguments override.
- Tools: `task_template_create`, `task_template_list`, `task_template_delete`; `template` argument on `task_issue_create` and `--template` on CLI `new`.

### 4. Saved views (criterion 10)

Table `saved_views` (owner_id NULL = shared, name, query_json, UNIQUE(owner_id, name)). JSON is allowed here, but it must be validated: parse it into the same arguments `task_issue_list` takes (reuse `Command::from_tool("task_issue_list", &query)` in the core to validate and to run it).
- Criterion: a stored query returns exactly the issues `task_issue_list` returns for the same filters (compare keys, in order).
- Tools: `task_view_save {name, query, shared}`, `task_view_list`, `task_view_run {name}`, `task_view_delete`. Owner = the acting user's person (`person_id_for` in the store, same as comment authors). Beware: SQLite UNIQUE treats NULL owner_id as distinct, so check duplicates of shared views in code.

### 5. UI (rest of criterion 11)

- "My work" screen: a new `ActiveView::MyWork`, a nav rail entry and a View menu item. It lists issues assigned to, mentioned to or watched by the acting user (person for `ctx.actor`) without duplicates, with the reason shown (assigned / mentioned / watched). Put the query in the store (`Tracker::my_work(person)`) and expose `task_my_work` so CLI/MCP get it too. Test the no-duplicate rule in core tests.
- Epic swimlane on the board: `SwimlaneMode` already has None / Priority / Assignee. Add `Epic`: lane per top-level epic (via `parent` up to the epic; issues without an epic in a "No epic" lane). Look at how `rebuild_cards` builds lanes.
- Every new menu label goes through `bookend::t(de, "Deutsch", "English")` (see gap section). Every new menu action must be handled in `menu_action` (the test `every_menu_item_has_a_handler` enforces this). Keep high contrast: no light grey text on white in light mode; use theme tokens (`theme::current(ctx)`), never raw greys.

## Patterns to copy

- Store feature module: `crates/taskcraft-store/src/collab.rs` (validation, transaction, typed event with payload, `Result` types, no unwrap).
- Core wiring for a tool, four places in `crates/taskcraft-core/src/lib.rs`: `Command` enum variant, `Command::from_tool` arm, `dispatch` arm, `tools()` catalog entry. Argument helpers: `s`, `opt_s`, `int`, `opt_int`, `tri` (absent / null / value).
- CLI: `crates/taskcraft-mcp/src/main.rs`, `Cmd` enum plus `to_tool` mapping. `--json` parity comes for free.
- Tests per criterion: `crates/taskcraft-core/src/tests.rs`, see `criterion_6_hierarchy_rules` and `assert_rejected` (rejected = typed error, DB unchanged, no history written).
- App actions: `execute_*` functions on `App`, deferred actions inside `render_inspector` (flags set in the UI closure, executed after it). App tests use `memory_app()`.
- The MCP probe test asserts `tools/list` returns `tools().len()`; no fixed count to update.

## Closing Tier 5

When all criteria pass: tick the boxes in `docs/TIERED_PLAN.md` Tier 5, update the Tier 5 row and "next" list in `HANDOVER.md`, add `CHANGELOG.md` (Unreleased) entries, add HELP.md sections for the new CLI verbs, regenerate `docs/TOOLS.md`. Tier 6 (sprints, capacity) is next; its schema is PM_SPEC "v3" and needs a new migration.

Effort guess: 1 to 1.5 days of agent work for all five steps.
