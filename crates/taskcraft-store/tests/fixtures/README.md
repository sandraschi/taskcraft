# Test fixtures

## v1.db

A real schema-v1 database, made on 2026-11-15 with the v0.1.4 CLI at commit `ed729e3` (the last commit before the migration runner). Do not regenerate it with a newer build: a newer CLI writes the current schema, not v1. `tests/migrate.rs` checks that the file still says `schema_version = 1`.

Contents: 2 projects, 13 issues, milestones, labels, relations, 2 linked commits, history events.

Recipe (PowerShell, `$c` = v0.1.4 `taskcraft-cli.exe`, `$repo` = an empty folder):

```powershell
git -C $repo init -q -b main
& $c --db v1.db demo-seed
& $c --db v1.db project create FX 'Migration fixture' --repo $repo
& $c --db v1.db new FX 'Case variant of an existing assignee' --assignee 'joe mocky ' --estimate 3
& $c --db v1.db new FX 'One-letter assignee' --assignee 'A' --labels 'edge'
& $c --db v1.db new FX 'Unassigned, closed by commit' --priority high
& $c --db v1.db link FX-1 blocks FX-2
# two commits in $repo: "FX-1: start work", then "Closes FX-3"
& $c --db v1.db git-sync FX --allow-path $repo
```

What it covers for the v2 migration:
- `Joe Mocky` (demo) and `joe mocky ` (FX-1) must become one person.
- `A` is too short for a handle and becomes `user-a`.
- Unassigned issues stay unassigned.
- `FX-3` was closed by a commit, so commits and their history events must survive.

The project `FX` stores the repo path of the machine that made the fixture. Tests do not use it.
