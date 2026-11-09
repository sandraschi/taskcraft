//! taskcraft-core: the command bus shared by CLI, MCP and the desktop app.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

pub mod gitsync;

use chrono::NaiveDate;
use craft_biz::at::parse_date;
use craft_biz::bus::Bus;
pub use craft_biz::confine::Confinement;
use craft_biz::mcp::tool;
use serde_json::{json, Value};
use std::path::PathBuf;
use taskcraft_store::{
    CommitLink, IssueFilter, IssuePatch, NewIssue, Priority, RelationKind, Status, TaskError,
    Tracker,
};

pub const APP: &str = "taskcraft";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct Ctx {
    pub tracker: Tracker,
    pub confinement: Confinement,
    pub bus: Bus,
    /// Who is acting: "agent" over MCP, the OS user on the CLI, "desktop" in the app.
    pub actor: String,
}

#[derive(Debug, Clone)]
pub enum Command {
    Status,
    ProjectCreate {
        key: String,
        name: String,
        repo_path: Option<String>,
    },
    ProjectList,
    IssueCreate(NewIssue),
    IssueList(IssueFilter),
    IssueShow {
        issue: String,
    },
    IssueUpdate {
        issue: String,
        patch: IssuePatch,
    },
    IssueMove {
        issue: String,
        status: Status,
        force: bool,
    },
    RelationAdd {
        source: String,
        kind: RelationKind,
        target: String,
    },
    RelationRemove {
        source: String,
        kind: RelationKind,
        target: String,
    },
    MilestoneCreate {
        project: String,
        title: String,
        due: Option<NaiveDate>,
    },
    MilestoneList {
        project: Option<String>,
    },
    MilestoneClose {
        project: String,
        title: String,
    },
    Board {
        project: String,
    },
    GitSync {
        project: String,
        max: usize,
    },
    InboxDrain,
    DemoSeed,
}

// ---------- argument helpers ----------

fn s<'a>(a: &'a Value, k: &str) -> Result<&'a str, String> {
    a.get(k)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string argument {k:?}"))
}
fn opt_s(a: &Value, k: &str) -> Option<String> {
    a.get(k)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|v| !v.trim().is_empty())
}
fn opt_date(a: &Value, k: &str) -> Result<Option<NaiveDate>, String> {
    opt_s(a, k)
        .map(|d| parse_date(&d).map_err(|e| e.to_string()))
        .transpose()
}
fn e(err: TaskError) -> String {
    err.to_string()
}
/// Absent -> None; null -> Some(None); value -> Some(Some(v)).
fn tri<T>(
    a: &Value,
    k: &str,
    f: impl Fn(&Value) -> Result<T, String>,
) -> Result<Option<Option<T>>, String> {
    match a.get(k) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(v) => f(v).map(|x| Some(Some(x))),
    }
}
fn labels(a: &Value) -> Result<Option<Vec<String>>, String> {
    match a.get("labels") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(arr)) => arr
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "labels must be strings".to_string())
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(Value::String(s)) => Ok(Some(
            s.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect(),
        )),
        _ => Err("labels must be an array of strings".into()),
    }
}
fn priority(a: &Value) -> Result<Option<Priority>, String> {
    opt_s(a, "priority")
        .map(|p| Priority::parse(&p).map_err(e))
        .transpose()
}
fn estimate(v: &Value) -> Result<i64, String> {
    v.as_i64()
        .ok_or_else(|| "estimate must be an integer".to_string())
}

impl Command {
    pub fn from_tool(name: &str, a: &Value) -> Result<Command, String> {
        let name = craft_biz::mcp::normalize_tool_name(name);
        Ok(match name.as_str() {
            "task_status" => Command::Status,
            "task_project_create" => Command::ProjectCreate {
                key: s(a, "key")?.into(),
                name: s(a, "name")?.into(),
                repo_path: opt_s(a, "repo_path"),
            },
            "task_project_list" => Command::ProjectList,
            "task_issue_create" => Command::IssueCreate(NewIssue {
                project: s(a, "project")?.into(),
                title: s(a, "title")?.into(),
                description: opt_s(a, "description").unwrap_or_default(),
                status: opt_s(a, "status")
                    .map(|v| Status::parse(&v).map_err(e))
                    .transpose()?,
                priority: priority(a)?,
                estimate: a
                    .get("estimate")
                    .filter(|v| !v.is_null())
                    .map(estimate)
                    .transpose()?,
                assignee: opt_s(a, "assignee"),
                milestone: opt_s(a, "milestone"),
                due_date: opt_date(a, "due_date")?,
                labels: labels(a)?.unwrap_or_default(),
            }),
            "task_issue_list" => Command::IssueList(IssueFilter {
                project: opt_s(a, "project"),
                statuses: opt_s(a, "status")
                    .map(|v| {
                        v.split(',')
                            .map(|x| Status::parse(x).map_err(e))
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?
                    .unwrap_or_default(),
                priority: priority(a)?,
                label: opt_s(a, "label"),
                milestone: opt_s(a, "milestone"),
                text: opt_s(a, "text"),
                include_finished: a
                    .get("include_finished")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                limit: a
                    .get("limit")
                    .and_then(Value::as_u64)
                    .map_or(200, |v| v.min(5000) as usize),
            }),
            "task_issue_show" => Command::IssueShow {
                issue: s(a, "issue")?.into(),
            },
            "task_issue_update" => Command::IssueUpdate {
                issue: s(a, "issue")?.into(),
                patch: IssuePatch {
                    title: opt_s(a, "title"),
                    description: a
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    priority: priority(a)?,
                    estimate: tri(a, "estimate", estimate)?,
                    assignee: tri(a, "assignee", |v| {
                        v.as_str()
                            .map(str::to_string)
                            .ok_or_else(|| "assignee must be a string".into())
                    })?,
                    milestone: tri(a, "milestone", |v| {
                        v.as_str()
                            .map(str::to_string)
                            .ok_or_else(|| "milestone must be a string".into())
                    })?,
                    due_date: tri(a, "due_date", |v| {
                        v.as_str()
                            .ok_or_else(|| "due_date must be a string".to_string())
                            .and_then(|d| parse_date(d).map_err(|x| x.to_string()))
                    })?,
                    labels: labels(a)?,
                },
            },
            "task_issue_move" => Command::IssueMove {
                issue: s(a, "issue")?.into(),
                status: Status::parse(s(a, "status")?).map_err(e)?,
                force: a.get("force").and_then(Value::as_bool).unwrap_or(false),
            },
            "task_relation_add" | "task_relation_remove" => {
                let source = s(a, "source")?.to_string();
                let target = s(a, "target")?.to_string();
                let kind = RelationKind::parse(s(a, "kind")?).map_err(e)?;
                if name == "task_relation_add" {
                    Command::RelationAdd {
                        source,
                        kind,
                        target,
                    }
                } else {
                    Command::RelationRemove {
                        source,
                        kind,
                        target,
                    }
                }
            }
            "task_milestone_create" => Command::MilestoneCreate {
                project: s(a, "project")?.into(),
                title: s(a, "title")?.into(),
                due: opt_date(a, "due_date")?,
            },
            "task_milestone_list" => Command::MilestoneList {
                project: opt_s(a, "project"),
            },
            "task_milestone_close" => Command::MilestoneClose {
                project: s(a, "project")?.into(),
                title: s(a, "title")?.into(),
            },
            "task_board" => Command::Board {
                project: s(a, "project")?.into(),
            },
            "task_git_sync" => Command::GitSync {
                project: s(a, "project")?.into(),
                max: a
                    .get("max")
                    .and_then(Value::as_u64)
                    .map_or(500, |v| v.min(5000) as usize),
            },
            "task_inbox_drain" => Command::InboxDrain,
            "task_demo_seed" => Command::DemoSeed,
            other => return Err(format!("unknown tool {other}")),
        })
    }
}

fn to_json<T: serde::Serialize>(v: &T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|x| x.to_string())
}

pub fn dispatch(ctx: &mut Ctx, cmd: Command) -> Result<Value, String> {
    let actor = ctx.actor.clone();
    let t = &mut ctx.tracker;
    match cmd {
        Command::Status => Ok(
            json!({ "app": APP, "version": VERSION, "stats": t.stats().map_err(e)?, "projects": to_json(&t.projects().map_err(e)?)? }),
        ),
        Command::ProjectCreate {
            key,
            name,
            repo_path,
        } => {
            if let Some(p) = &repo_path {
                ctx.confinement.check(p, false)?;
            }
            to_json(
                &t.create_project(&key, &name, repo_path.as_deref())
                    .map_err(e)?,
            )
        }
        Command::ProjectList => Ok(json!({ "projects": to_json(&t.projects().map_err(e)?)? })),
        Command::IssueCreate(n) => to_json(&t.create_issue(&n, &actor).map_err(e)?),
        Command::IssueList(f) => Ok(json!({ "issues": to_json(&t.list(&f).map_err(e)?)? })),
        Command::IssueShow { issue } => to_json(&t.detail(&issue).map_err(e)?),
        Command::IssueUpdate { issue, patch } => {
            to_json(&t.update_issue(&issue, &patch, &actor).map_err(e)?)
        }
        Command::IssueMove {
            issue,
            status,
            force,
        } => to_json(&t.move_issue(&issue, status, force, &actor).map_err(e)?),
        Command::RelationAdd {
            source,
            kind,
            target,
        } => {
            t.add_relation(&source, kind, &target, &actor).map_err(e)?;
            Ok(json!({ "added": format!("{source} {} {target}", kind.as_str()) }))
        }
        Command::RelationRemove {
            source,
            kind,
            target,
        } => {
            t.remove_relation(&source, kind, &target, &actor)
                .map_err(e)?;
            Ok(json!({ "removed": format!("{source} {} {target}", kind.as_str()) }))
        }
        Command::MilestoneCreate {
            project,
            title,
            due,
        } => to_json(&t.create_milestone(&project, &title, due).map_err(e)?),
        Command::MilestoneList { project } => {
            Ok(json!({ "milestones": to_json(&t.milestones(project.as_deref()).map_err(e)?)? }))
        }
        Command::MilestoneClose { project, title } => {
            t.close_milestone(&project, &title).map_err(e)?;
            Ok(json!({ "closed": title }))
        }
        Command::Board { project } => board(t, &project),
        Command::GitSync { project, max } => git_sync(ctx, &project, max),
        Command::InboxDrain => inbox_drain(ctx),
        Command::DemoSeed => demo_seed(&mut ctx.tracker, &actor),
    }
}

/// Kanban lanes for a project.
pub fn board(t: &Tracker, project: &str) -> Result<Value, String> {
    let p = t.project(project).map_err(e)?;
    let issues = t
        .list(&IssueFilter {
            project: Some(p.key.clone()),
            include_finished: true,
            limit: 2000,
            ..Default::default()
        })
        .map_err(e)?;
    let mut lanes = Vec::new();
    for st in Status::ALL {
        let mut cards = Vec::new();
        let mut points = 0i64;
        for i in issues.iter().filter(|i| i.status == *st) {
            points += i.estimate.unwrap_or(0);
            let blocked = !t.open_blockers(i.id).map_err(e)?.is_empty();
            cards.push(json!({ "key": i.key, "title": i.title, "priority": i.priority, "estimate": i.estimate, "assignee": i.assignee, "labels": i.labels, "blocked": blocked, "due_date": i.due_date }));
        }
        lanes.push(json!({ "status": st, "count": cards.len(), "points": points, "cards": cards }));
    }
    Ok(json!({ "project": p.key, "name": p.name, "lanes": lanes }))
}

/// Link commits that mention `KEY-n`; close issues on explicit closing phrases.
pub fn git_sync(ctx: &mut Ctx, project: &str, max: usize) -> Result<Value, String> {
    let p = ctx.tracker.project(project).map_err(e)?;
    let repo = p
        .repo_path
        .clone()
        .ok_or_else(|| format!("project {} has no repo_path", p.key))?;
    let path: PathBuf = ctx.confinement.check(&repo, false)?;
    let commits = gitsync::read_log(&path, max)?;
    let (mut linked, mut closed, mut unknown) = (0usize, Vec::new(), Vec::new());
    let mut blocked = Vec::new();
    // Oldest first so state changes happen in history order.
    for c in commits.iter().rev() {
        let message = format!("{}\n{}", c.subject, c.body);
        for m in gitsync::mentions(&message, &p.key) {
            let key = format!("{}-{}", p.key, m.number);
            let Ok(issue) = ctx.tracker.issue(&key) else {
                if !unknown.contains(&key) {
                    unknown.push(key);
                }
                continue;
            };
            let link = CommitLink {
                hash: c.hash.clone(),
                author: c.author.clone(),
                committed_at: c.committed_at.clone(),
                subject: c.subject.clone(),
                closing: m.closing,
            };
            // Status only changes the first time a commit is seen for this issue.
            if !ctx.tracker.link_commit(issue.id, &link).map_err(e)? {
                continue;
            }
            linked += 1;
            if m.closing && !issue.status.is_finished() {
                match ctx.tracker.move_issue(
                    &key,
                    Status::Done,
                    false,
                    &format!("git:{}", c.author),
                ) {
                    Ok(_) => closed.push(key),
                    Err(TaskError::Blocked(msg)) => blocked.push(msg),
                    Err(other) => return Err(other.to_string()),
                }
            }
        }
    }
    if let Some(newest) = commits.first() {
        ctx.tracker.set_last_synced(p.id, &newest.hash).map_err(e)?;
    }
    Ok(
        json!({ "project": p.key, "commits_read": commits.len(), "links_added": linked, "closed": closed, "not_closed_blocked": blocked, "unknown_keys": unknown }),
    )
}

/// Turn events from other craft apps into issues.
pub fn inbox_drain(ctx: &mut Ctx) -> Result<Value, String> {
    let events = ctx.bus.pending(APP).map_err(|x| x.to_string())?;
    let mut created = Vec::new();
    let mut rejected = Vec::new();
    for ev in events {
        let result = issue_from_event(&mut ctx.tracker, &ev.kind, &ev.source, &ev.payload);
        match result {
            Ok(key) => {
                ctx.bus.ack(APP, &ev.id).map_err(|x| x.to_string())?;
                created.push(json!({ "event": ev.id, "issue": key }));
            }
            Err(err) => {
                ctx.bus
                    .reject(APP, &ev.id, &err)
                    .map_err(|x| x.to_string())?;
                rejected.push(json!({ "event": ev.id, "error": err }));
            }
        }
    }
    Ok(json!({ "created": created, "rejected": rejected }))
}

fn default_project(t: &Tracker, p: &Value) -> Result<String, String> {
    if let Some(k) = opt_s(p, "project") {
        return Ok(k);
    }
    t.projects()
        .map_err(e)?
        .first()
        .map(|p| p.key.clone())
        .ok_or_else(|| "no project exists to file the issue in".into())
}

fn issue_from_event(
    t: &mut Tracker,
    kind: &str,
    source: &str,
    p: &Value,
) -> Result<String, String> {
    let n = match kind {
        "ticket.escalated" => NewIssue {
            project: default_project(t, p)?,
            title: format!("[Support] {}", s(p, "subject")?),
            description: format!(
                "Escalated from {source} ticket {}.\n\n{}",
                p.get("ticket_id").map(Value::to_string).unwrap_or_default(),
                opt_s(p, "details").unwrap_or_default()
            ),
            status: Some(Status::Todo),
            priority: Some(
                opt_s(p, "priority")
                    .map(|v| Priority::parse(&v).map_err(e))
                    .transpose()?
                    .unwrap_or(Priority::High),
            ),
            labels: vec!["support".into(), "bug".into()],
            ..Default::default()
        },
        "contract.deadline" => NewIssue {
            project: default_project(t, p)?,
            title: format!("[Contract] {}", s(p, "title")?),
            description: format!(
                "Deadline from {source} contract {}.",
                p.get("contract_id")
                    .map(Value::to_string)
                    .unwrap_or_default()
            ),
            status: Some(Status::Todo),
            priority: Some(Priority::High),
            due_date: Some(parse_date(s(p, "due_date")?).map_err(|x| x.to_string())?),
            labels: vec!["contract".into()],
            ..Default::default()
        },
        other => return Err(format!("taskcraft does not handle event kind {other}")),
    };
    t.create_issue(&n, &format!("bus:{source}"))
        .map(|i| i.key)
        .map_err(e)
}

/// (title, status, priority, points, milestone, assignee, labels)
type DemoRow = (
    &'static str,
    Status,
    Priority,
    i64,
    &'static str,
    &'static str,
    &'static [&'static str],
);

/// Labelled demo project. Only on an empty tracker.
pub fn demo_seed(t: &mut Tracker, actor: &str) -> Result<Value, String> {
    if !t.projects().map_err(e)?.is_empty() {
        return Err("demo seed only runs on an empty tracker".into());
    }
    t.set_meta("demo", "1").map_err(e)?;
    t.create_project("BC", "BuchCraft roadmap (DEMO)", None)
        .map_err(e)?;
    t.create_milestone(
        "BC",
        "v0.3 GUI polish",
        NaiveDate::from_ymd_opt(2026, 11, 30),
    )
    .map_err(e)?;
    t.create_milestone("BC", "v1.0 Release", NaiveDate::from_ymd_opt(2027, 1, 31))
        .map_err(e)?;
    let rows: [DemoRow; 10] = [
        (
            "Verify bank CSV import against a real George export",
            Status::Todo,
            Priority::Urgent,
            2,
            "v0.3 GUI polish",
            "Joe Mocky",
            &["import", "bank"],
        ),
        (
            "Right-align amount columns in all tables",
            Status::InProgress,
            Priority::Medium,
            1,
            "v0.3 GUI polish",
            "Hannes Mockinger",
            &["ui"],
        ),
        (
            "egui_kittest golden snapshots for the 8 views",
            Status::Todo,
            Priority::High,
            3,
            "v0.3 GUI polish",
            "",
            &["ui", "tests"],
        ),
        (
            "NSIS current-user installer",
            Status::Backlog,
            Priority::Medium,
            3,
            "v1.0 Release",
            "",
            &["release"],
        ),
        (
            "Ship .exe release assets (no zip)",
            Status::Backlog,
            Priority::High,
            2,
            "v1.0 Release",
            "",
            &["release"],
        ),
        (
            "Steuerberater review of the EKR seed chart",
            Status::InReview,
            Priority::High,
            1,
            "v1.0 Release",
            "Joe Mocky",
            &["accounting"],
        ),
        (
            "Engine-generated VAT lines",
            Status::Done,
            Priority::Urgent,
            5,
            "v0.3 GUI polish",
            "Joe Mocky",
            &["accounting"],
        ),
        (
            "Storno with two-way links",
            Status::Done,
            Priority::High,
            3,
            "v0.3 GUI polish",
            "Hannes Mockinger",
            &["accounting"],
        ),
        (
            "CAMT.053 import",
            Status::Backlog,
            Priority::Low,
            5,
            "",
            "",
            &["import", "bank"],
        ),
        (
            "Dark-mode chart colours",
            Status::Canceled,
            Priority::Low,
            1,
            "",
            "",
            &["ui"],
        ),
    ];
    for (title, status, prio, est, ms, who, labels) in rows {
        t.create_issue(
            &NewIssue {
                project: "BC".into(),
                title: title.into(),
                description: String::new(),
                status: Some(status),
                priority: Some(prio),
                estimate: Some(est),
                assignee: (!who.is_empty()).then(|| who.to_string()),
                milestone: (!ms.is_empty()).then(|| ms.to_string()),
                due_date: None,
                labels: labels.iter().map(|l| l.to_string()).collect(),
            },
            actor,
        )
        .map_err(e)?;
    }
    t.add_relation("BC-6", RelationKind::Blocks, "BC-5", actor)
        .map_err(e)?;
    t.add_relation("BC-4", RelationKind::Blocks, "BC-5", actor)
        .map_err(e)?;
    t.add_relation("BC-1", RelationKind::Blocks, "BC-9", actor)
        .map_err(e)?;
    t.add_relation("BC-2", RelationKind::RelatesTo, "BC-3", actor)
        .map_err(e)?;
    Ok(json!({ "demo": true, "project": "BC", "stats": t.stats().map_err(e)? }))
}

/// MCP tool catalog.
pub fn tools() -> Vec<Value> {
    let issue = json!({ "type": "string", "description": "Issue key like BC-12 (or numeric id)" });
    let status = json!({ "type": "string", "enum": ["backlog", "todo", "in_progress", "in_review", "done", "canceled"] });
    let prio = json!({ "type": "string", "enum": ["low", "medium", "high", "urgent"] });
    let kind = json!({ "type": "string", "enum": ["blocks", "relates_to", "duplicates"] });
    let date = json!({ "type": "string", "description": "DD.MM.YYYY or YYYY-MM-DD" });
    let labels = json!({ "type": "array", "items": { "type": "string" } });
    vec![
        tool("task_status", "Tracker status: counts (open, in progress, urgent, overdue), projects, demo flag.", json!({}), &[]),
        tool("task_project_create", "Create a project with a 2-10 char key (issues become KEY-n). Optional repo_path enables git sync.", json!({ "key": {"type":"string"}, "name": {"type":"string"}, "repo_path": {"type":"string"} }), &["key", "name"]),
        tool("task_project_list", "List projects.", json!({}), &[]),
        tool("task_issue_create", "Create an issue.", json!({ "project": {"type":"string"}, "title": {"type":"string"}, "description": {"type":"string"}, "status": status, "priority": prio, "estimate": {"type":"integer","description":"story points 0-1000"}, "assignee": {"type":"string"}, "milestone": {"type":"string"}, "due_date": date, "labels": labels }), &["project", "title"]),
        tool("task_issue_list", "List issues, urgent first. Finished issues are hidden unless include_finished or an explicit status filter.", json!({ "project": {"type":"string"}, "status": {"type":"string","description":"comma list, e.g. todo,in_progress"}, "priority": prio, "label": {"type":"string"}, "milestone": {"type":"string"}, "text": {"type":"string"}, "include_finished": {"type":"boolean"}, "limit": {"type":"integer"} }), &[]),
        tool("task_issue_show", "One issue with relations, open blockers, linked commits and full history.", json!({ "issue": issue }), &["issue"]),
        tool("task_issue_update", "Change fields. Pass null to clear estimate/assignee/milestone/due_date. labels replaces the set.", json!({ "issue": issue, "title": {"type":"string"}, "description": {"type":"string"}, "priority": prio, "estimate": {"type":["integer","null"]}, "assignee": {"type":["string","null"]}, "milestone": {"type":["string","null"]}, "due_date": {"type":["string","null"]}, "labels": labels }), &["issue"]),
        tool("task_issue_move", "Change status. Moving to in_progress/in_review/done is refused while an open issue blocks it, unless force=true.", json!({ "issue": issue, "status": status, "force": {"type":"boolean"} }), &["issue", "status"]),
        tool("task_relation_add", "Relate two issues. 'blocks' edges may not form a cycle.", json!({ "source": issue, "kind": kind, "target": issue }), &["source", "kind", "target"]),
        tool("task_relation_remove", "Remove a relation.", json!({ "source": issue, "kind": kind, "target": issue }), &["source", "kind", "target"]),
        tool("task_milestone_create", "Create a milestone in a project.", json!({ "project": {"type":"string"}, "title": {"type":"string"}, "due_date": date }), &["project", "title"]),
        tool("task_milestone_list", "Milestones with progress (issues and points done/total).", json!({ "project": {"type":"string"} }), &[]),
        tool("task_milestone_close", "Close a milestone.", json!({ "project": {"type":"string"}, "title": {"type":"string"} }), &["project", "title"]),
        tool("task_board", "Kanban lanes for a project: cards per status, points, blocked flags.", json!({ "project": {"type":"string"} }), &["project"]),
        tool("task_git_sync", "Read the project repo's git log, link commits mentioning KEY-n, close issues on 'closes/fixes/resolves KEY-n'. Needs --allow-path covering the repo.", json!({ "project": {"type":"string"}, "max": {"type":"integer"} }), &["project"]),
        tool("task_inbox_drain", "Turn events from other craft apps (deskcraft ticket.escalated, contractcraft contract.deadline) into issues.", json!({}), &[]),
        tool("task_demo_seed", "Fill an EMPTY tracker with a labelled demo project.", json!({}), &[]),
    ]
}

#[cfg(test)]
mod tests;
