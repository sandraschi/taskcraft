# taskcraft PRD

## What it is

A local-first issue tracker for a 1-10 person team (Linear / Jira / Asana replacement for the small case), with git commit linking and inputs from the other back-office craft apps. Pure Rust, ArtCraft pattern. Suite plan: `BACKOFFICE_CRAFT_SUITE.md`, section 4.2.

## Users

- Sandra and a small team planning work per repo or project.
- Agents over MCP: create and triage issues, move them, link commits. Every agent change is in the history with actor `agent`.

## Jobs

1. Keep one honest list of work per project, with priorities, estimates and due dates.
2. Make blockers impossible to ignore.
3. Connect work to commits without a hosted service.
4. Collect work that other apps discover (support escalations, contract deadlines).

## Today (v0.1.1)

- Projects with keys, `KEY-n` issues, statuses, priorities, points, labels, assignee, due date, milestones with progress.
- Relations with a cycle check on `blocks`; moving a blocked issue to in_progress / in_review / done needs `force`.
- Full per-issue history (agent, user, `git:<author>`, `bus:<app>`).
- `git-sync`: read-only `git log`, mention linking, explicit closing keywords, idempotent.
- Inbox: `ticket.escalated` (deskcraft) and `contract.deadline` (contractcraft) become issues; `capacity.changed` (teamcraft) is shown in status. All three verified end to end.
- 17 MCP tools ([TOOLS.md](TOOLS.md), generated and test-checked), CLI with board view.

## Boundaries

- No hosted sync, no GitHub/GitLab issue sync in v1 (gitops-mcp covers GitHub).
- Git is read-only; taskcraft never writes to a repo.
- No time tracking (teamcraft) and no customer communication (deskcraft).

## Gaps (honest)

- No egui window; the board is CLI text and JSON.
- No comments or attachments on issues.
- No sprints / cycles; capacity is shown, not planned against.
- Git sync on demand only (no hook or watcher).
- Synthetic data only.

## Next

1. egui app: board lanes, issue detail with history, milestone burn-up.
2. Comments on issues.
3. Use teamcraft capacity in milestone planning.
