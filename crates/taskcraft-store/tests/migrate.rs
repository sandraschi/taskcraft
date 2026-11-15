//! Migration tests against `tests/fixtures/v1.db` (PM_SPEC section 1 and
//! Tier 5 criterion 1). How the fixture was made: `tests/fixtures/README.md`.
// Test helpers may panic, like `#[test]` bodies (clippy.toml allows those).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use taskcraft_store::{person_handle, NewIssue, Status, Tracker, SCHEMA_VERSION};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/v1.db");

/// Fresh copy of the fixture in its own temp folder.
fn copy_fixture(tag: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("taskcraft-migrate-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("taskcraft.db");
    std::fs::copy(FIXTURE, &db).unwrap();
    (dir, db)
}

fn version(db: &Path) -> String {
    let c = Connection::open(db).unwrap();
    c.query_row(
        "SELECT value FROM meta WHERE key = 'schema_version'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn strings(c: &Connection, sql: &str) -> Vec<String> {
    let mut st = c.prepare(sql).unwrap();
    st.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Everything that must survive the migration, as comparable text.
struct Snapshot {
    keys: Vec<String>,
    relations: Vec<String>,
    commits: Vec<String>,
    events: Vec<String>,
    labels: Vec<String>,
    milestones: Vec<String>,
}

fn snapshot(c: &Connection) -> Snapshot {
    Snapshot {
        keys: strings(
            c,
            "SELECT p.key || '-' || i.number || ' ' || i.status || ' ' || i.title
             FROM issues i JOIN projects p ON p.id = i.project_id ORDER BY i.id",
        ),
        relations: strings(
            c,
            "SELECT source_id || ' ' || kind || ' ' || target_id FROM relations ORDER BY 1",
        ),
        commits: strings(
            c,
            "SELECT issue_id || ' ' || hash || ' ' || closing FROM commits ORDER BY 1",
        ),
        events: strings(
            c,
            "SELECT id || ' ' || issue_id || ' ' || actor || ' ' || what FROM events ORDER BY id",
        ),
        labels: strings(
            c,
            "SELECT issue_id || ' ' || label FROM issue_labels ORDER BY 1",
        ),
        milestones: strings(
            c,
            "SELECT project_id || ' ' || title FROM milestones ORDER BY 1",
        ),
    }
}

#[test]
fn fixture_is_a_real_v1_database() {
    assert_eq!(version(Path::new(FIXTURE)), "1");
    let c = Connection::open(FIXTURE).unwrap();
    let s = snapshot(&c);
    assert_eq!(s.keys.len(), 13);
    assert!(!s.relations.is_empty() && s.commits.len() == 2 && !s.events.is_empty());
}

#[test]
fn v1_fixture_migrates_and_keeps_every_row() {
    let (dir, db) = copy_fixture("keep");
    let before = {
        let c = Connection::open(&db).unwrap();
        let s = snapshot(&c);
        let assignees = strings(
            &c,
            "SELECT p.key || '-' || i.number || '=' || lower(trim(i.assignee))
             FROM issues i JOIN projects p ON p.id = i.project_id
             WHERE trim(COALESCE(i.assignee, '')) <> '' ORDER BY i.id",
        );
        (s, assignees)
    };

    let t = Tracker::open(&db).unwrap();
    let m = t
        .migrated()
        .cloned()
        .expect("an existing v1 file is migrated");
    assert_eq!((m.from, m.to), (1, SCHEMA_VERSION));
    let backup = m.backup.clone().expect("file databases get a backup");
    assert_eq!(backup, dir.join("taskcraft.db.v1.bak"));
    assert_eq!(version(&backup), "1", "the backup is the old database");

    // Assignees read through the store keep their name (first spelling wins).
    for line in &before.1 {
        let (key, old) = line.split_once('=').unwrap();
        let now = t.issue(key).unwrap().assignee.unwrap_or_default();
        assert_eq!(now.to_lowercase(), old, "{key}");
    }
    drop(t);

    let after = snapshot(&Connection::open(&db).unwrap());
    assert_eq!(after.keys, before.0.keys);
    assert_eq!(after.relations, before.0.relations);
    assert_eq!(after.commits, before.0.commits);
    assert_eq!(after.labels, before.0.labels);
    assert_eq!(after.milestones, before.0.milestones);
    assert_eq!(after.events, before.0.events);

    // A new event on the migrated database is typed and carries its project.
    let mut t = Tracker::open(&db).unwrap();
    t.move_issue("FX-1", Status::InProgress, false, "tester")
        .unwrap();
    drop(t);
    let c = Connection::open(&db).unwrap();

    // Each distinct old assignee is one person; spellings of the same name merge.
    let people = strings(
        &c,
        "SELECT handle || '|' || display_name FROM people ORDER BY id",
    );
    assert_eq!(
        people,
        vec![
            "joe-mocky|Joe Mocky",
            "hannes-mockinger|Hannes Mockinger",
            "user-a|A"
        ]
    );
    let joe = strings(
        &c,
        "SELECT DISTINCT p.key || ':' || i.assignee_id FROM issues i
         JOIN projects p ON p.id = i.project_id JOIN people x ON x.id = i.assignee_id
         WHERE x.handle = 'joe-mocky' ORDER BY 1",
    );
    assert_eq!(joe, vec!["BC:1", "FX:1"], "BC and FX share one Joe");

    // Old events: kind legacy, project from the issue. The new one is typed.
    let kinds = strings(
        &c,
        "SELECT DISTINCT e.kind || ' ' || (e.project_id = i.project_id)
         FROM events e JOIN issues i ON i.id = e.issue_id ORDER BY 1",
    );
    assert_eq!(kinds, vec!["issue.moved 1", "legacy 1"]);

    // The text column is gone; constraints hold.
    assert!(c.prepare("SELECT assignee FROM issues").is_err());
    assert!(strings(&c, "PRAGMA foreign_key_check").is_empty());
    assert_eq!(strings(&c, "PRAGMA integrity_check"), vec!["ok"]);
    assert_eq!(version(&db), SCHEMA_VERSION.to_string());
}

#[test]
fn second_open_is_a_no_op() {
    let (dir, db) = copy_fixture("noop");
    drop(Tracker::open(&db).unwrap());
    let t = Tracker::open(&db).unwrap();
    assert!(t.migrated().is_none());
    let backups = std::fs::read_dir(&dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .map(|e| e.file_name().to_string_lossy().ends_with(".bak"))
                .unwrap_or(false)
        })
        .count();
    assert_eq!(backups, 1);
}

#[test]
fn a_second_migration_never_overwrites_an_old_backup() {
    let (dir, db) = copy_fixture("twobak");
    std::fs::write(dir.join("taskcraft.db.v1.bak"), b"older backup").unwrap();
    let t = Tracker::open(&db).unwrap();
    let b = t.migrated().and_then(|m| m.backup.clone()).unwrap();
    assert_ne!(b, dir.join("taskcraft.db.v1.bak"));
    assert_eq!(
        std::fs::read(dir.join("taskcraft.db.v1.bak")).unwrap(),
        b"older backup"
    );
    assert_eq!(version(&b), "1");
}

#[test]
fn newer_database_is_refused_and_left_alone() {
    let (_dir, db) = copy_fixture("newer");
    {
        let c = Connection::open(&db).unwrap();
        c.execute(
            "UPDATE meta SET value = '99' WHERE key = 'schema_version'",
            [],
        )
        .unwrap();
    }
    let err = Tracker::open(&db).err().expect("refused");
    assert!(err.to_string().contains("newer than this build"), "{err}");
    assert_eq!(version(&db), "99");
    let c = Connection::open(&db).unwrap();
    assert!(
        c.prepare("SELECT assignee FROM issues").is_ok(),
        "not migrated"
    );
}

#[test]
fn new_database_starts_at_latest_without_backup() {
    let dir = std::env::temp_dir().join(format!("taskcraft-migrate-fresh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let db = dir.join("new.db");
    let mut t = Tracker::open(&db).unwrap();
    assert!(t.migrated().is_none());
    assert_eq!(version(&db), SCHEMA_VERSION.to_string());
    t.create_project("TC", "Test", None).unwrap();
    for name in ["Jürgen Öhler", "  jürgen öhler ", "Jürgen-Öhler"] {
        let n = NewIssue {
            project: "TC".into(),
            title: format!("for {name}"),
            assignee: Some(name.into()),
            ..NewIssue::default()
        };
        let i = t.create_issue(&n, "tester").unwrap();
        assert_eq!(i.assignee.as_deref(), Some("Jürgen Öhler"));
    }
    drop(t);
    let c = Connection::open(&db).unwrap();
    assert_eq!(
        strings(&c, "SELECT handle FROM people"),
        vec!["juergen-oehler"]
    );
    assert!(std::fs::read_dir(&dir).unwrap().all(|e| !e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".bak")));
}

#[test]
fn in_memory_database_is_current() {
    let t = Tracker::open_in_memory().unwrap();
    assert!(t.migrated().is_none());
}

#[test]
fn handle_rules() {
    assert_eq!(person_handle("Joe Mocky"), "joe-mocky");
    assert_eq!(person_handle("  Hannes   Mockinger "), "hannes-mockinger");
    assert_eq!(person_handle("Straße"), "strasse");
    assert_eq!(person_handle("A"), "user-a");
    assert_eq!(person_handle(""), "user");
    assert_eq!(person_handle("日本"), "user");
    assert_eq!(person_handle("ana.b_c"), "ana.b_c");
    let long = person_handle(&"x".repeat(50));
    assert_eq!(long.len(), 32);
    assert_eq!(person_handle("ab- -cd!"), "ab-cd");
}
