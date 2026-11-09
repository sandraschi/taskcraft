use super::*;

fn t() -> Tracker {
    let mut t = Tracker::open_in_memory().unwrap();
    t.create_project("TC", "TaskCraft", None).unwrap();
    t
}

fn new(t: &mut Tracker, title: &str) -> Issue {
    t.create_issue(
        &NewIssue {
            project: "TC".into(),
            title: title.into(),
            ..Default::default()
        },
        "test",
    )
    .unwrap()
}

#[test]
fn numbers_are_per_project_and_gapless() {
    let mut t = t();
    t.create_project("BC", "BuchCraft", None).unwrap();
    assert_eq!(new(&mut t, "a").key, "TC-1");
    assert_eq!(new(&mut t, "b").key, "TC-2");
    let bc = t
        .create_issue(
            &NewIssue {
                project: "bc".into(),
                title: "x".into(),
                ..Default::default()
            },
            "test",
        )
        .unwrap();
    assert_eq!(bc.key, "BC-1");
    assert!(t.create_project("TC", "dup", None).is_err());
    assert!(t.create_project("1X", "bad", None).is_err());
    assert!(t.create_project("T", "short", None).is_err());
}

#[test]
fn validation() {
    let mut t = t();
    assert!(t
        .create_issue(
            &NewIssue {
                project: "TC".into(),
                title: "  ".into(),
                ..Default::default()
            },
            "x"
        )
        .is_err());
    assert!(t
        .create_issue(
            &NewIssue {
                project: "NOPE".into(),
                title: "x".into(),
                ..Default::default()
            },
            "x"
        )
        .is_err());
    assert!(t
        .create_issue(
            &NewIssue {
                project: "TC".into(),
                title: "x".into(),
                estimate: Some(-1),
                ..Default::default()
            },
            "x"
        )
        .is_err());
    assert!(t
        .create_issue(
            &NewIssue {
                project: "TC".into(),
                title: "x".into(),
                milestone: Some("v9".into()),
                ..Default::default()
            },
            "x"
        )
        .is_err());
    assert!(t
        .create_issue(
            &NewIssue {
                project: "TC".into(),
                title: "x".into(),
                labels: vec!["a,b".into()],
                ..Default::default()
            },
            "x"
        )
        .is_err());
    assert!(parse_issue_key("TC-0").is_none());
    assert_eq!(parse_issue_key("#tc-12"), Some(("TC".into(), 12)));
}

#[test]
fn blockers_gate_status_moves() {
    let mut t = t();
    let a = new(&mut t, "schema");
    let b = new(&mut t, "api");
    t.add_relation(&a.key, RelationKind::Blocks, &b.key, "test")
        .unwrap();
    let err = t
        .move_issue(&b.key, Status::InProgress, false, "test")
        .unwrap_err();
    assert!(matches!(err, TaskError::Blocked(_)), "{err}");
    // Todo is allowed while blocked.
    t.move_issue(&b.key, Status::Todo, false, "test").unwrap();
    t.move_issue(&a.key, Status::Done, false, "test").unwrap();
    let moved = t
        .move_issue(&b.key, Status::InProgress, false, "test")
        .unwrap();
    assert_eq!(moved.status, Status::InProgress);
    // Force overrides.
    let c = new(&mut t, "docs");
    let d = new(&mut t, "release");
    t.add_relation(&c.key, RelationKind::Blocks, &d.key, "test")
        .unwrap();
    assert!(t.move_issue(&d.key, Status::Done, true, "test").is_ok());
    assert!(t.issue(&d.key).unwrap().closed_at.is_some());
    assert!(
        t.move_issue(&d.key, Status::Done, true, "test").is_err(),
        "same status refused"
    );
    let reopened = t.move_issue(&d.key, Status::Todo, false, "test").unwrap();
    assert!(reopened.closed_at.is_none());
}

#[test]
fn blocks_cycles_are_refused() {
    let mut t = t();
    let a = new(&mut t, "a");
    let b = new(&mut t, "b");
    let c = new(&mut t, "c");
    t.add_relation(&a.key, RelationKind::Blocks, &b.key, "x")
        .unwrap();
    t.add_relation(&b.key, RelationKind::Blocks, &c.key, "x")
        .unwrap();
    assert!(t
        .add_relation(&c.key, RelationKind::Blocks, &a.key, "x")
        .unwrap_err()
        .to_string()
        .contains("cycle"));
    assert!(t
        .add_relation(&a.key, RelationKind::Blocks, &a.key, "x")
        .is_err());
    assert!(
        t.add_relation(&a.key, RelationKind::Blocks, &b.key, "x")
            .is_err(),
        "duplicate"
    );
    // relates_to may go both ways.
    t.add_relation(&c.key, RelationKind::RelatesTo, &a.key, "x")
        .unwrap();
    t.remove_relation(&a.key, RelationKind::Blocks, &b.key, "x")
        .unwrap();
    t.add_relation(&c.key, RelationKind::Blocks, &a.key, "x")
        .unwrap();
}

#[test]
fn update_list_filter_and_history() {
    let mut t = t();
    t.create_milestone("TC", "v0.1", NaiveDate::from_ymd_opt(2026, 11, 30))
        .unwrap();
    let a = new(&mut t, "Kanban board");
    let b = new(&mut t, "Git sync");
    t.update_issue(
        &a.key,
        &IssuePatch {
            priority: Some(Priority::Urgent),
            labels: Some(vec!["UI".into(), "ui".into(), "egui".into()]),
            milestone: Some(Some("v0.1".into())),
            estimate: Some(Some(5)),
            ..Default::default()
        },
        "sandra",
    )
    .unwrap();
    t.update_issue(
        &b.key,
        &IssuePatch {
            estimate: Some(Some(3)),
            milestone: Some(Some("v0.1".into())),
            ..Default::default()
        },
        "sandra",
    )
    .unwrap();
    assert!(t.update_issue(&b.key, &IssuePatch::default(), "x").is_err());
    let got = t.issue(&a.key).unwrap();
    assert_eq!(got.labels, vec!["egui".to_string(), "ui".to_string()]);
    let urgent = t
        .list(&IssueFilter {
            priority: Some(Priority::Urgent),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(urgent.len(), 1);
    let ui = t
        .list(&IssueFilter {
            label: Some("UI".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(ui.len(), 1);
    let text = t
        .list(&IssueFilter {
            text: Some("git".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(text.first().map(|i| i.key.clone()), Some(b.key.clone()));
    // Priority ordering: urgent first.
    assert_eq!(
        t.list(&IssueFilter::default())
            .unwrap()
            .first()
            .map(|i| i.key.clone()),
        Some(a.key.clone())
    );
    t.move_issue(&b.key, Status::Done, false, "x").unwrap();
    assert_eq!(
        t.list(&IssueFilter::default()).unwrap().len(),
        1,
        "finished hidden by default"
    );
    assert_eq!(
        t.list(&IssueFilter {
            statuses: vec![Status::Done],
            ..Default::default()
        })
        .unwrap()
        .len(),
        1
    );
    let ms = t.milestones(Some("TC")).unwrap();
    let m = ms.first().unwrap();
    assert_eq!(
        (m.issues_total, m.issues_done, m.points_total, m.points_done),
        (2, 1, 8, 3)
    );
    let d = t.detail(&a.key).unwrap();
    assert!(d.history.len() >= 2);
    assert!(d
        .history
        .iter()
        .any(|e| e.actor == "sandra" && e.what.contains("priority -> urgent")));
}

#[test]
fn commit_links_are_idempotent() {
    let mut t = t();
    let a = new(&mut t, "a");
    let c = CommitLink {
        hash: "abc123".into(),
        author: "Joe Mocky".into(),
        committed_at: "2026-11-09T10:00:00Z".into(),
        subject: "fix: thing".into(),
        closing: true,
    };
    assert!(t.link_commit(a.id, &c).unwrap());
    assert!(!t.link_commit(a.id, &c).unwrap());
    assert_eq!(t.detail(&a.key).unwrap().commits.len(), 1);
}
