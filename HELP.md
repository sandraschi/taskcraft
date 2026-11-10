# taskcraft help

Local-first replacement for Linear, Jira, Asana, and GitHub Projects for small teams (1-10 people). Combines a native egui 0.37 desktop workstation, FastMCP stdio server, and CLI on top of an embedded SQLite database.

Task-oriented guide. Every command also exists as an MCP tool ([docs/TOOLS.md](docs/TOOLS.md)); add `--json` for machine-readable output; `taskcraft <command> --help` lists all options.

## Projects and issues

```powershell
taskcraft project create BC "BuchCraft roadmap" --repo D:\Dev\repos\buchcraft
taskcraft new BC "Fix CSV export" --priority high --labels bug,export --estimate 2 --due 30.11.2026
taskcraft list --project BC                 # unfinished, urgent first
taskcraft list --status done,canceled --project BC
taskcraft show BC-7                         # relations, blockers, commits, full history
taskcraft update BC-7 --assignee "Joe Mocky" --milestone "v1.0 Release"
taskcraft update BC-7 --clear-assignee
```

Keys are `PROJECT-n`, numbered per project and never reused.

## Status and blockers

Statuses: backlog, todo, in_progress, in_review, done, canceled.

```powershell
taskcraft link BC-4 blocks BC-5
taskcraft move BC-5 in_progress             # refused while BC-4 is unfinished
taskcraft move BC-5 in_progress --force     # override; recorded in the history
taskcraft unlink BC-4 blocks BC-5
```

`blocks` links may not form a cycle. `relates_to` and `duplicates` are informational.

## Board and milestones

```powershell
taskcraft board BC
taskcraft milestone create BC "v1.0 Release" --due 31.01.2027
taskcraft milestone list --project BC       # issues and points done / total
taskcraft milestone close BC "v1.0 Release"
```

## Git

```powershell
taskcraft git-sync BC --allow-path D:\Dev\repos\buchcraft
```

Reads the repo's `git log` (read-only). Commits that mention `BC-12` are linked; `closes BC-12`, `fixes BC-12` or `resolves BC-12` also move the issue to done (unless it is blocked). Re-running links nothing twice and never reopens or re-closes.

## From other apps

```powershell
taskcraft inbox-drain
```

- deskcraft `ticket.escalated` becomes a `[Support]` issue (labels support, bug).
- contractcraft `contract.deadline` becomes a `[Contract]` issue with the due date.
- teamcraft `capacity.changed` is stored per week and shown in `taskcraft status`.

Issues go into the project named in the event, else the first project.

## Desktop workstation (egui)

Launch via `start.ps1` or `start.bat`:

```powershell
.\start.ps1                     # Launches native desktop app
.\start.ps1 -Smoke              # Verifies startup and exits
.\start.ps1 -Mcp                # Starts TaskCraft MCP server on stdio
```

Features:
- **Kanban Board**: Drag and drop cards, live blocker rules, column `+` button to create issues, priority/assignee swimlanes, and live filter toolbar.
- **Interactive Inspector**: Click or double-click any card to view and edit title, description, status, priority, assignee, estimate, due date, milestones, labels, blockers/relations, and git commits in place.
- **Views**: Quick navigation between Kanban Board, Issues Table (sortable with CSV export), Milestones & Burn-up, Git Commits & Sync, and Activity Log.
- **Palette**: Press `Ctrl+K` to open the command palette and run any tool via auto-generated forms.

## Troubleshooting

- **"blocked: ..."**: finish or unlink the blocker, or pass `--force`.
- **git-sync "outside the allowed paths"**: pass `--allow-path` covering the repo (always required in MCP mode).
- **Inbox rejected an event**: see `taskcraft --json inbox-drain`; the reason is written next to the event in the bus `failed` folder.
