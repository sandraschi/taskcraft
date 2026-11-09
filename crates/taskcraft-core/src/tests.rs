use super::*;
use std::process::Command as Proc;

fn ctx(tag: &str) -> Ctx {
    let root = std::env::temp_dir().join(format!("taskcraft-bus-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    Ctx {
        tracker: Tracker::open_in_memory().unwrap(),
        confinement: Confinement(None),
        bus: Bus::new(root),
        actor: "test".into(),
    }
}

fn call(c: &mut Ctx, name: &str, a: Value) -> Result<Value, String> {
    let cmd = Command::from_tool(name, &a)?;
    dispatch(c, cmd)
}

#[test]
fn catalog_tools_are_routable() {
    for t in tools() {
        let name = t["name"].as_str().unwrap();
        if let Err(err) = Command::from_tool(name, &json!({})) {
            assert!(!err.starts_with("unknown tool"), "{name}");
        }
    }
}

#[test]
fn demo_board_and_blockers() {
    let mut c = ctx("demo");
    call(&mut c, "task_demo_seed", json!({})).unwrap();
    assert!(call(&mut c, "task_demo_seed", json!({})).is_err());
    let b = call(&mut c, "task_board", json!({ "project": "bc" })).unwrap();
    let lanes = b["lanes"].as_array().unwrap();
    assert_eq!(lanes.len(), 6);
    let counts: Vec<u64> = lanes.iter().map(|l| l["count"].as_u64().unwrap()).collect();
    assert_eq!(counts, vec![3, 2, 1, 1, 2, 1]);
    // BC-5 is blocked by BC-6 (in review) and BC-4 (backlog).
    let err = call(
        &mut c,
        "task_issue_move",
        json!({ "issue": "BC-5", "status": "in_progress" }),
    )
    .unwrap_err();
    assert!(err.contains("BC-4") && err.contains("BC-6"), "{err}");
    let shown = call(&mut c, "task.issue_show", json!({ "issue": "BC-5" })).unwrap();
    assert_eq!(shown["blocked_by_open"].as_array().map(Vec::len), Some(2));
    let ms = call(&mut c, "task_milestone_list", json!({ "project": "BC" })).unwrap();
    assert_eq!(ms["milestones"][0]["points_done"], 8);
}

#[test]
fn update_clears_with_null() {
    let mut c = ctx("upd");
    call(
        &mut c,
        "task_project_create",
        json!({ "key": "TC", "name": "TaskCraft" }),
    )
    .unwrap();
    call(&mut c, "task_issue_create", json!({ "project": "TC", "title": "x", "estimate": 3, "assignee": "Joe Mocky", "labels": "ui, egui" })).unwrap();
    let u = call(
        &mut c,
        "task_issue_update",
        json!({ "issue": "TC-1", "assignee": null, "estimate": null }),
    )
    .unwrap();
    assert!(u["assignee"].is_null() && u["estimate"].is_null());
    assert_eq!(u["labels"], json!(["egui", "ui"]));
    assert!(call(
        &mut c,
        "task_issue_update",
        json!({ "issue": "TC-1", "estimate": "five" })
    )
    .is_err());
}

#[test]
fn inbox_creates_issues_from_other_apps() {
    let mut c = ctx("inbox");
    call(
        &mut c,
        "task_project_create",
        json!({ "key": "OPS", "name": "Ops" }),
    )
    .unwrap();
    c.bus
        .emit(
            "deskcraft",
            APP,
            "ticket.escalated",
            json!({ "ticket_id": 42, "subject": "Export crashes", "details": "stack trace" }),
        )
        .unwrap();
    c.bus.emit("contractcraft", APP, "contract.deadline", json!({ "contract_id": "MSA-7", "title": "Kuendigung MSA Acme", "due_date": "30.09.2027" })).unwrap();
    c.bus
        .emit("teamcraft", APP, "capacity.changed", json!({}))
        .unwrap();
    c.bus
        .emit(
            "teamcraft",
            APP,
            "capacity.changed",
            json!({ "week": "2026-W46", "total": "32:00", "total_minutes": 1920, "employees": [] }),
        )
        .unwrap();
    c.bus
        .emit("gaincraft", APP, "signal.negative", json!({}))
        .unwrap();
    let r = call(&mut c, "task_inbox_drain", json!({})).unwrap();
    assert_eq!(r["created"].as_array().map(Vec::len), Some(3), "{r}");
    assert_eq!(
        r["rejected"].as_array().map(Vec::len),
        Some(2),
        "empty capacity + unknown kind"
    );
    let st = call(&mut c, "task_status", json!({})).unwrap();
    assert_eq!(st["team_capacity"][0]["total"], "32:00");
    let i = call(&mut c, "task_issue_show", json!({ "issue": "OPS-2" })).unwrap();
    assert_eq!(i["due_date"], "2027-09-30");
    assert_eq!(i["history"][0]["actor"], "bus:contractcraft");
    let _ = std::fs::remove_dir_all(c.bus.root());
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let ok = Proc::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(ok, "git {args:?}");
}

#[test]
fn git_sync_links_and_closes_against_a_real_repo() {
    if Proc::new("git").arg("--version").output().is_err() {
        eprintln!("git not installed; skipping");
        return;
    }
    let repo = std::env::temp_dir().join(format!("taskcraft-git-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&repo);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.name", "Joe Mocky"]);
    git(&repo, &["config", "user.email", "joe@mocky.invalid"]);
    for msg in [
        "feat: board work for TC-1",
        "fix typo (unrelated fix)",
        "Closes TC-1 and fixes TC-2",
        "wip TC-99",
    ] {
        std::fs::write(repo.join("f.txt"), msg).unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", msg]);
    }
    let mut c = ctx("git");
    c.confinement = Confinement::roots(std::slice::from_ref(&repo)).unwrap();
    call(
        &mut c,
        "task_project_create",
        json!({ "key": "TC", "name": "TaskCraft", "repo_path": repo.display().to_string() }),
    )
    .unwrap();
    call(
        &mut c,
        "task_issue_create",
        json!({ "project": "TC", "title": "board" }),
    )
    .unwrap();
    call(
        &mut c,
        "task_issue_create",
        json!({ "project": "TC", "title": "typo" }),
    )
    .unwrap();
    call(
        &mut c,
        "task_issue_create",
        json!({ "project": "TC", "title": "blocker" }),
    )
    .unwrap();
    call(
        &mut c,
        "task_relation_add",
        json!({ "source": "TC-3", "kind": "blocks", "target": "TC-2" }),
    )
    .unwrap();
    let r = call(&mut c, "task_git_sync", json!({ "project": "TC" })).unwrap();
    assert_eq!(r["commits_read"], 4);
    assert_eq!(r["closed"], json!(["TC-1"]));
    assert_eq!(
        r["not_closed_blocked"].as_array().map(Vec::len),
        Some(1),
        "{r}"
    );
    assert_eq!(r["unknown_keys"], json!(["TC-99"]));
    let one = call(&mut c, "task_issue_show", json!({ "issue": "TC-1" })).unwrap();
    assert_eq!(one["status"], "done");
    assert_eq!(one["commits"].as_array().map(Vec::len), Some(2));
    // Re-sync is idempotent: reopen TC-1, sync again, it stays open.
    call(
        &mut c,
        "task_issue_move",
        json!({ "issue": "TC-1", "status": "todo" }),
    )
    .unwrap();
    let again = call(&mut c, "task_git_sync", json!({ "project": "TC" })).unwrap();
    assert_eq!(again["links_added"], 0);
    assert_eq!(
        call(&mut c, "task_issue_show", json!({ "issue": "TC-1" })).unwrap()["status"],
        "todo"
    );
    // Confinement: a repo outside the allowed roots is refused.
    c.confinement = Confinement(Some(Vec::new()));
    assert!(call(&mut c, "task_git_sync", json!({ "project": "TC" }))
        .unwrap_err()
        .contains("--allow-path"));
    let _ = std::fs::remove_dir_all(&repo);
}
