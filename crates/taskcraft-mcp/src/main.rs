//! `taskcraft`: CLI and MCP stdio server in one binary.
//!
//! Every CLI verb builds the same tool-argument JSON an MCP client would send
//! and goes through `Command::from_tool` + `dispatch`, so CLI and MCP share one
//! code path. In `mcp` mode stdout carries only JSON-RPC; logs go to stderr.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use clap::{Parser, Subcommand};
use craft_biz::bus::Bus;
use craft_biz::mcp::{serve_stdio, ToolHost};
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::process::ExitCode;
use taskcraft_core::{dispatch, tools, Command, Confinement, Ctx, APP, VERSION};
use taskcraft_store::Tracker;

const AFTER_HELP: &str = "Typical day:
  taskcraft board BC
  taskcraft new BC \"Fix CSV export\" --priority high --labels bug --estimate 2
  taskcraft link BC-4 blocks BC-5
  taskcraft move BC-5 in_progress              # refused while blocked; --force overrides
  taskcraft git-sync BC --allow-path D:\\Dev\\repos\\buchcraft
  taskcraft inbox-drain                        # escalations, contract deadlines, capacity
See HELP.md for workflows and docs/TOOLS.md for every MCP tool.";

#[derive(Parser)]
#[command(after_help = AFTER_HELP,
    name = "taskcraft",
    version,
    about = "Local-first issue tracker with git-commit linking. CLI + MCP."
)]
struct Cli {
    /// Tracker database. Default: $TASKCRAFT_DB or %LOCALAPPDATA%\taskcraft\taskcraft.db
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    /// Print machine-readable JSON only.
    #[arg(long, global = true)]
    json: bool,
    /// Restrict repo access to these directories (repeatable). Required for git sync in `mcp` mode.
    #[arg(long = "allow-path", global = true)]
    allow_path: Vec<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Tracker status and counts.
    Status,
    /// Projects.
    Project {
        #[command(subcommand)]
        op: ProjectOp,
    },
    /// Create an issue.
    New {
        project: String,
        title: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long)]
        estimate: Option<i64>,
        #[arg(long)]
        assignee: Option<String>,
        #[arg(long)]
        milestone: Option<String>,
        #[arg(long)]
        due: Option<String>,
        /// Comma list.
        #[arg(long)]
        labels: Option<String>,
    },
    /// List issues (finished hidden unless --all or --status).
    List {
        #[arg(long)]
        project: Option<String>,
        /// Comma list, e.g. todo,in_progress
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        milestone: Option<String>,
        #[arg(long)]
        text: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long, default_value_t = 200)]
        limit: u64,
    },
    /// Show an issue with relations, commits and history.
    Show { issue: String },
    /// Update fields. Use --clear-<field> to unset.
    Update {
        issue: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long)]
        estimate: Option<i64>,
        #[arg(long)]
        assignee: Option<String>,
        #[arg(long)]
        milestone: Option<String>,
        #[arg(long)]
        due: Option<String>,
        #[arg(long)]
        labels: Option<String>,
        #[arg(long)]
        clear_estimate: bool,
        #[arg(long)]
        clear_assignee: bool,
        #[arg(long)]
        clear_milestone: bool,
        #[arg(long)]
        clear_due: bool,
    },
    /// Change status (backlog, todo, in_progress, in_review, done, canceled).
    Move {
        issue: String,
        status: String,
        /// Override the open-blocker gate.
        #[arg(long)]
        force: bool,
    },
    /// Relate issues: `taskcraft link BC-4 blocks BC-5`.
    Link {
        source: String,
        kind: String,
        target: String,
    },
    /// Remove a relation.
    Unlink {
        source: String,
        kind: String,
        target: String,
    },
    /// Milestones.
    Milestone {
        #[command(subcommand)]
        op: MilestoneOp,
    },
    /// Kanban lanes for a project.
    Board { project: String },
    /// Link commits from the project's repo; close on "closes KEY-n".
    GitSync {
        project: String,
        #[arg(long, default_value_t = 500)]
        max: u64,
    },
    /// Turn events from other craft apps into issues.
    InboxDrain,
    /// Fill an empty tracker with the labelled demo project.
    DemoSeed,
    /// Print the MCP tool catalog (JSON, or markdown for docs/TOOLS.md).
    Tools {
        #[arg(long)]
        markdown: bool,
    },
    /// Run the MCP stdio server.
    Mcp,
}

#[derive(Subcommand)]
enum ProjectOp {
    List,
    Create {
        key: String,
        name: String,
        #[arg(long)]
        repo: Option<String>,
    },
}

#[derive(Subcommand)]
enum MilestoneOp {
    List {
        #[arg(long)]
        project: Option<String>,
    },
    Create {
        project: String,
        title: String,
        #[arg(long)]
        due: Option<String>,
    },
    Close {
        project: String,
        title: String,
    },
}

fn default_db() -> PathBuf {
    if let Ok(p) = std::env::var("TASKCRAFT_DB") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    let base = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("taskcraft").join("taskcraft.db")
}

/// Build a JSON object, dropping `None` values.
fn obj(pairs: Vec<(&str, Option<Value>)>) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        if let Some(v) = v {
            m.insert(k.to_string(), v);
        }
    }
    Value::Object(m)
}

fn sv(s: &str) -> Option<Value> {
    Some(Value::String(s.to_string()))
}
fn os(s: Option<String>) -> Option<Value> {
    s.map(Value::String)
}
/// `--clear-x` wins over a value: emits JSON null.
fn clearable(v: Option<Value>, clear: bool) -> Option<Value> {
    if clear {
        Some(Value::Null)
    } else {
        v
    }
}

/// Map a CLI verb onto (tool name, arguments).
fn to_tool(cmd: Cmd) -> Result<(&'static str, Value), String> {
    Ok(match cmd {
        Cmd::Status => ("task_status", json!({})),
        Cmd::Project {
            op: ProjectOp::List,
        } => ("task_project_list", json!({})),
        Cmd::Project {
            op: ProjectOp::Create { key, name, repo },
        } => (
            "task_project_create",
            obj(vec![
                ("key", sv(&key)),
                ("name", sv(&name)),
                ("repo_path", os(repo)),
            ]),
        ),
        Cmd::New {
            project,
            title,
            description,
            status,
            priority,
            estimate,
            assignee,
            milestone,
            due,
            labels,
        } => (
            "task_issue_create",
            obj(vec![
                ("project", sv(&project)),
                ("title", sv(&title)),
                ("description", os(description)),
                ("status", os(status)),
                ("priority", os(priority)),
                ("estimate", estimate.map(|e| json!(e))),
                ("assignee", os(assignee)),
                ("milestone", os(milestone)),
                ("due_date", os(due)),
                ("labels", os(labels)),
            ]),
        ),
        Cmd::List {
            project,
            status,
            priority,
            label,
            milestone,
            text,
            all,
            limit,
        } => (
            "task_issue_list",
            obj(vec![
                ("project", os(project)),
                ("status", os(status)),
                ("priority", os(priority)),
                ("label", os(label)),
                ("milestone", os(milestone)),
                ("text", os(text)),
                ("include_finished", Some(json!(all))),
                ("limit", Some(json!(limit))),
            ]),
        ),
        Cmd::Show { issue } => ("task_issue_show", json!({ "issue": issue })),
        Cmd::Update {
            issue,
            title,
            description,
            priority,
            estimate,
            assignee,
            milestone,
            due,
            labels,
            clear_estimate,
            clear_assignee,
            clear_milestone,
            clear_due,
        } => (
            "task_issue_update",
            obj(vec![
                ("issue", sv(&issue)),
                ("title", os(title)),
                ("description", os(description)),
                ("priority", os(priority)),
                (
                    "estimate",
                    clearable(estimate.map(|e| json!(e)), clear_estimate),
                ),
                ("assignee", clearable(os(assignee), clear_assignee)),
                ("milestone", clearable(os(milestone), clear_milestone)),
                ("due_date", clearable(os(due), clear_due)),
                ("labels", os(labels)),
            ]),
        ),
        Cmd::Move {
            issue,
            status,
            force,
        } => (
            "task_issue_move",
            json!({ "issue": issue, "status": status, "force": force }),
        ),
        Cmd::Link {
            source,
            kind,
            target,
        } => (
            "task_relation_add",
            json!({ "source": source, "kind": kind, "target": target }),
        ),
        Cmd::Unlink {
            source,
            kind,
            target,
        } => (
            "task_relation_remove",
            json!({ "source": source, "kind": kind, "target": target }),
        ),
        Cmd::Milestone {
            op: MilestoneOp::List { project },
        } => ("task_milestone_list", obj(vec![("project", os(project))])),
        Cmd::Milestone {
            op:
                MilestoneOp::Create {
                    project,
                    title,
                    due,
                },
        } => (
            "task_milestone_create",
            obj(vec![
                ("project", sv(&project)),
                ("title", sv(&title)),
                ("due_date", os(due)),
            ]),
        ),
        Cmd::Milestone {
            op: MilestoneOp::Close { project, title },
        } => (
            "task_milestone_close",
            json!({ "project": project, "title": title }),
        ),
        Cmd::Board { project } => ("task_board", json!({ "project": project })),
        Cmd::GitSync { project, max } => {
            ("task_git_sync", json!({ "project": project, "max": max }))
        }
        Cmd::InboxDrain => ("task_inbox_drain", json!({})),
        Cmd::DemoSeed => ("task_demo_seed", json!({})),
        Cmd::Tools { .. } | Cmd::Mcp => return Err("not a tool verb".into()),
    })
}

struct Host {
    ctx: Ctx,
}

impl ToolHost for Host {
    fn server_name(&self) -> &str {
        APP
    }
    fn server_version(&self) -> &str {
        VERSION
    }
    fn tools(&self) -> Vec<Value> {
        tools()
    }
    fn call(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        let cmd = Command::from_tool(name, args)?;
        dispatch(&mut self.ctx, cmd)
    }
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "-".into(),
        Value::Array(a) => a.iter().map(scalar).collect::<Vec<_>>().join(","),
        other => other.to_string(),
    }
}

/// Human rendering: compact lines for issue lists and boards, indented JSON otherwise.
fn render(v: &Value) -> String {
    let mut out = String::new();
    if let Some(issues) = v.get("issues").and_then(Value::as_array) {
        for i in issues {
            out.push_str(&format!(
                "{:<8} {:<11} {:<6} {:>3}  {}  [{}]\n",
                scalar(&i["key"]),
                scalar(&i["status"]),
                scalar(&i["priority"]),
                i["estimate"]
                    .as_i64()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "-".into()),
                scalar(&i["title"]),
                scalar(&i["labels"])
            ));
        }
        if issues.is_empty() {
            out.push_str("(none)\n");
        }
        return out;
    }
    if let Some(lanes) = v.get("lanes").and_then(Value::as_array) {
        out.push_str(&format!(
            "{} {}\n",
            scalar(&v["project"]),
            scalar(&v["name"])
        ));
        for l in lanes {
            out.push_str(&format!(
                "\n== {} ({} issues, {} pts)\n",
                scalar(&l["status"]),
                scalar(&l["count"]),
                scalar(&l["points"])
            ));
            for c in l["cards"].as_array().into_iter().flatten() {
                let flag = if c["blocked"].as_bool().unwrap_or(false) {
                    " BLOCKED"
                } else {
                    ""
                };
                out.push_str(&format!(
                    "  {:<8} {:<6} {}{flag}\n",
                    scalar(&c["key"]),
                    scalar(&c["priority"]),
                    scalar(&c["title"])
                ));
            }
        }
        return out;
    }
    for key in ["projects", "milestones"] {
        if let Some(rows) = v.get(key).and_then(Value::as_array) {
            for row in rows {
                let cells: Vec<String> = row
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter(|(_, x)| !x.is_object())
                    .map(|(_, x)| scalar(x))
                    .collect();
                out.push_str(&cells.join("  |  "));
                out.push('\n');
            }
            if rows.is_empty() {
                out.push_str("(none)\n");
            }
            return out;
        }
    }
    serde_json::to_string_pretty(v).unwrap_or_else(|e| e.to_string())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Cmd::Tools { markdown } = cli.cmd {
        if markdown {
            print!("{}", craft_biz::mcp::tools_markdown(APP, &tools()));
            return ExitCode::SUCCESS;
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&tools()).unwrap_or_default()
        );
        return ExitCode::SUCCESS;
    }
    let db = cli.db.clone().unwrap_or_else(default_db);
    let tracker = match Tracker::open(&db) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("taskcraft: cannot open {}: {e}", db.display());
            return ExitCode::FAILURE;
        }
    };
    let is_mcp = matches!(cli.cmd, Cmd::Mcp);
    let confinement = if is_mcp || !cli.allow_path.is_empty() {
        match Confinement::roots(&cli.allow_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("taskcraft: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        Confinement(None)
    };
    let actor = if is_mcp {
        "agent".to_string()
    } else {
        std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "cli".into())
    };
    let ctx = Ctx {
        tracker,
        confinement,
        bus: Bus::new(Bus::default_root()),
        actor,
    };
    if is_mcp {
        eprintln!("[taskcraft] db {}", db.display());
        let mut host = Host { ctx };
        return match serve_stdio(&mut host) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("taskcraft mcp: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let json_out = cli.json;
    let mut ctx = ctx;
    let result = to_tool(cli.cmd)
        .and_then(|(name, args)| Command::from_tool(name, &args))
        .and_then(|cmd| dispatch(&mut ctx, cmd));
    match result {
        Ok(v) => {
            if json_out {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            } else {
                print!("{}", render(&v));
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            if json_out {
                println!("{}", json!({ "error": e }));
            } else {
                eprintln!("taskcraft: {e}");
            }
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_biz::mcp::handle_line;

    #[test]
    fn tools_doc_is_current() {
        let committed = include_str!("../../../docs/TOOLS.md");
        assert!(
            craft_biz::mcp::docs_in_sync(committed, &craft_biz::mcp::tools_markdown(APP, &tools())),
            "docs/TOOLS.md is stale: run `just docs`"
        );
    }

    fn host() -> Host {
        let tracker = Tracker::open_in_memory().unwrap();
        let bus = Bus::new(
            std::env::temp_dir().join(format!("taskcraft-mcp-test-{}", std::process::id())),
        );
        Host {
            ctx: Ctx {
                tracker,
                confinement: Confinement(Some(Vec::new())),
                bus,
                actor: "agent".into(),
            },
        }
    }

    #[test]
    fn mcp_probe_initialize_list_call() {
        let mut h = host();
        let init = handle_line(
            &mut h,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        )
        .unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "taskcraft");
        let list =
            handle_line(&mut h, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap();
        assert_eq!(list["result"]["tools"].as_array().map(Vec::len), Some(17));
        let seed = handle_line(&mut h, r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"task_demo_seed","arguments":{}}}"#).unwrap();
        assert_eq!(seed["result"]["isError"], false);
        let board = handle_line(&mut h, r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"task.board","arguments":{"project":"BC"}}}"#).unwrap();
        assert!(board["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("in_review"));
        let blocked = handle_line(&mut h, r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"task_issue_move","arguments":{"issue":"BC-5","status":"done"}}}"#).unwrap();
        assert_eq!(blocked["result"]["isError"], true);
        let show = handle_line(&mut h, r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"task_issue_show","arguments":{"issue":"BC-1"}}}"#).unwrap();
        assert!(
            show["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("\"actor\": \"agent\"")
                || show["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("\"actor\":\"agent\"")
        );
    }

    #[test]
    fn cli_verbs_map_to_known_tools() {
        let verbs = vec![
            Cmd::Status,
            Cmd::Project {
                op: ProjectOp::List,
            },
            Cmd::Board {
                project: "BC".into(),
            },
            Cmd::Move {
                issue: "BC-1".into(),
                status: "done".into(),
                force: false,
            },
            Cmd::Link {
                source: "BC-1".into(),
                kind: "blocks".into(),
                target: "BC-2".into(),
            },
            Cmd::Milestone {
                op: MilestoneOp::List { project: None },
            },
            Cmd::GitSync {
                project: "BC".into(),
                max: 10,
            },
            Cmd::DemoSeed,
        ];
        let names: Vec<String> = tools()
            .iter()
            .filter_map(|t| t["name"].as_str().map(str::to_string))
            .collect();
        for v in verbs {
            let (name, _) = to_tool(v).unwrap();
            assert!(names.iter().any(|n| n == name), "{name}");
        }
    }

    #[test]
    fn clear_flags_emit_null() {
        let cmd = Cmd::Update {
            issue: "BC-1".into(),
            title: None,
            description: None,
            priority: None,
            estimate: Some(3),
            assignee: None,
            milestone: None,
            due: None,
            labels: None,
            clear_estimate: true,
            clear_assignee: true,
            clear_milestone: false,
            clear_due: false,
        };
        let (_, a) = to_tool(cmd).unwrap();
        assert!(a["estimate"].is_null() && a["assignee"].is_null());
        assert!(a.get("milestone").is_none());
    }

    #[test]
    fn render_lists_and_board() {
        let v = json!({ "issues": [ { "key": "BC-1", "status": "todo", "priority": "urgent", "estimate": 2, "title": "x", "labels": ["a", "b"] } ] });
        assert!(render(&v).contains("BC-1"));
        assert!(render(&json!({ "issues": [] })).contains("(none)"));
        let b = json!({ "project": "BC", "name": "n", "lanes": [ { "status": "todo", "count": 1, "points": 2, "cards": [ { "key": "BC-1", "priority": "low", "title": "t", "blocked": true } ] } ] });
        assert!(render(&b).contains("BLOCKED"));
    }
}
