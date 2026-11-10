# taskcraft

Local-first issue tracker for a small team, with git commit linking. One SQLite file, one binary that is both CLI and MCP server. Part of the back-office craft suite (replaces Linear / Jira / Asana for the 1-10 person case).

Status: **v0.1.3, Tier 1 + L3 Desktop Workstation** (store + CLI + MCP + egui app, tested). Synthetic demo data available via `demo-seed` or desktop top bar.

- How to use it day to day: [HELP.md](HELP.md)
- What it is for and where it stops: [docs/PRD.md](docs/PRD.md)
- Every MCP tool: [docs/TOOLS.md](docs/TOOLS.md) (generated, checked by a test)
- Handover and roadmap: [HANDOVER.md](HANDOVER.md)
- Changes: [CHANGELOG.md](CHANGELOG.md)

## What this replaces

TaskCraft is a local-first, zero-cloud-lock-in replacement for hosted issue trackers and project management tools for small sovereign engineering teams (1-10 people):

- **Replaces**:
  - **Linear / Jira / Asana**: Cloud-hosted task tracking tools with vendor lock-in, seat fees, API rate limits, and latency.
  - **Trello / ClickUp**: Cloud Kanban boards lacking deep local git commit linkage and cycle-checked dependency DAGs.
  - **GitHub Projects**: Hosted issue trackers that require internet connectivity, webhook infrastructure, and personal access tokens.

- **Why it replaces them**:
  - **Single SQLite WAL file**: All data lives locally in `%LOCALAPPDATA%\taskcraft\taskcraft.db`. No monthly subscriptions, no downtime, and sub-millisecond queries.
  - **Triple-surface architecture**: Instant 60fps native desktop workstation (`taskcraft-app`), FastMCP stdio server (`taskcraft mcp`) for AI coding agents, and terminal CLI (`taskcraft`).
  - **Git-commit linking built in**: Subprocess git log scanner (`git -C <repo> log`) automatically and idempotently detects commit mentions and closes issues on explicit closing keywords (`closes KEY-12`, `fixes KEY-12`).
  - **Strict dependency DAG**: Cycle-checked `blocks` relations. Issues cannot move to in_progress or done while blocked unless explicitly forced and audited in the event history.
  - **Fleet bus integration**: Receives tickets escalated from `deskcraft`, contract deadlines from `contractcraft`, and team capacity changes from `teamcraft`.

## What it does

- Projects with a short key; issues numbered `KEY-n` per project.
- Status lanes: backlog, todo, in_progress, in_review, done, canceled. Priority low..urgent, story points, assignee, labels, due date, milestones with progress.
- Relations: `blocks` (cycle-checked), `relates_to`, `duplicates`. Starting or finishing a blocked issue is refused unless forced.
- Full per-issue history (who changed what, including agents, git and other craft apps).
- `git-sync`: reads the project repo's `git log`, links commits that mention `KEY-n`, closes issues on `closes/fixes/resolves KEY-n`. Idempotent.
- Inbox: `ticket.escalated` from deskcraft and `contract.deadline` from contractcraft become issues automatically.
- Inbox: `capacity.changed` from teamcraft is stored per ISO week and shown in `status` (available team hours).

## Quick start

### Desktop Workstation (egui)

```powershell
.\start.ps1                          # Launches native desktop app
.\start.ps1 -Smoke                   # Verifies startup and exits
.\start.ps1 -Mcp                     # Starts MCP server on stdio
```

### CLI

```powershell
taskcraft demo-seed                  # labelled demo project BC (empty DB only)
taskcraft board BC
taskcraft new BC "Fix CSV export" --priority high --labels bug,export --estimate 2
taskcraft link BC-4 blocks BC-5
taskcraft move BC-5 in_progress      # refused: blocked by BC-4, BC-6
taskcraft project create TC "TaskCraft" --repo D:\Dev\repos\taskcraft
taskcraft git-sync TC --allow-path D:\Dev\repos\taskcraft
taskcraft --json show BC-5
```

Database: `--db PATH`, else `$TASKCRAFT_DB`, else `%LOCALAPPDATA%\taskcraft\taskcraft.db`.

## MCP

```json
{ "mcpServers": { "taskcraft": { "command": "D:/Dev/cargo-target/release/taskcraft.exe", "args": ["mcp", "--allow-path", "D:/Dev/repos"] } } }
```

17 tools, all `task_*` (dotted aliases like `task.board` accepted); see [docs/TOOLS.md](docs/TOOLS.md). Agent changes are recorded with actor `agent`.

## Build

```powershell
just ci             # clippy -D warnings, tests, fmt check
just build-release
```

Needs `../craft-biz` checked out next to this repo. `git` on PATH for git sync (tests skip the git case if missing).

## Not done (honest list)

- No comments/attachments on issues; description and audit log only.
- No sprints/cycles, no time tracking (teamcraft owns capacity).
- Git sync is pull-on-demand; no hooks, no watcher. GitHub/GitLab issue import not implemented.
