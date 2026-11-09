# AGENTS.md - taskcraft

Local-first issue tracker craft app (Linear/Jira replacement for a small team). Read first:
`mcp-central-docs/standards/RUST_ARTCRAFT_STANDARD.md`,
`mcp-central-docs/projects/craft-fleet-expansion/BACKOFFICE_CRAFT_SUITE.md` (section 4.2).

## Layout

- `crates/taskcraft-store` (L1): projects, issues (KEY-n numbering per project), milestones, relations, commit links, event history on SQLite WAL. All workflow invariants live here.
- `crates/taskcraft-core` (L1): `Command` bus, `dispatch`, MCP catalog, git log sync (`gitsync.rs`), bus inbox, demo seed.
- `crates/taskcraft-mcp` (L2): `taskcraft` binary. CLI verbs map onto the same tool calls as MCP.
- No L3 (egui) yet. Planned: board, issue detail, milestone burn-up.
- Shared kernel: `../craft-biz` (bus, MCP loop, confinement, date parsing).

## Workflow invariants (never weaken)

- Issue numbers are per project and never reused.
- `blocks` relations form a DAG; a cycle is refused at insert time.
- Moving to in_progress / in_review / done is refused while an unfinished issue blocks it, unless `force` is passed explicitly. Forced moves are recorded in history.
- Every mutation writes an event row with actor (`agent`, OS user, `git:<author>`, `bus:<app>`).
- Git sync links a commit to an issue at most once; status changes only on the first sighting, so re-syncing never reopens or re-closes anything.
- Only explicit closing phrases (closes/fixes/resolves KEY-n) close issues. A bare mention only links.

## Rules

- Zero-panic: unwrap/expect/panic/todo/unimplemented denied outside tests.
- MCP mode: stdout is JSON-RPC only; diagnostics to stderr.
- Repo paths go through `Confinement::check`; in `mcp` mode git sync needs `--allow-path`.
- Git is invoked as a subprocess (`git -C <repo> log`), read-only. Never write to a repo.
- Demo data only via `demo-seed`, only on an empty tracker, always flagged demo.
- Every bug fix ships with a regression test.

## Repo hygiene

- LF line endings only (`.gitattributes`). CRLF breaks commit hooks.
- Single trunk `main`. No agent branches or leftover WIP branches.
- No em dash characters in code, docs or commit messages.
- No `*.bak`, `*.db`, `target/`, binaries in git.

## Gates

1. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
2. `cargo test --workspace`
3. `cargo fmt --all --check`
4. MCP probe: `initialize` -> `tools/list` -> `tools/call`, stdout pure JSON
5. `Cargo.lock` committed, clean `git status`
6. `CHANGELOG.md` line
