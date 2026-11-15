//! Schema migrations (`docs/PM_SPEC.md` section 1).
//!
//! - A new database gets the v1 base schema, then every migration, so new
//!   and upgraded databases go through the same code.
//! - Each migration runs in one transaction with foreign keys off (SQLite
//!   cannot toggle them inside a transaction), ends with
//!   `PRAGMA foreign_key_check`, and stamps `schema_version` in the same
//!   transaction. A failure rolls the whole step back.
//! - Before the first migration of an existing file database, the old file
//!   is copied with `VACUUM INTO '<db>.v<old>.bak'`.
//! - A database newer than the build is refused.

use crate::{person_id_for, Result, TaskError};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// One schema step.
pub(crate) struct Migration {
    pub version: i64,
    pub apply: fn(&Transaction<'_>) -> Result<()>,
}

/// Ordered; the last version must equal `SCHEMA_VERSION`.
pub(crate) const MIGRATIONS: &[Migration] = &[Migration {
    version: 2,
    apply: v2,
}];

/// What opening the database did to an existing file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Migrated {
    /// Version found on disk.
    pub from: i64,
    /// Version after migrating.
    pub to: i64,
    /// Copy of the database before the first step (file databases only).
    pub backup: Option<PathBuf>,
}

impl std::fmt::Display for Migrated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "database upgraded from schema v{} to v{}",
            self.from, self.to
        )?;
        if let Some(b) = &self.backup {
            write!(f, "; backup of the old file: {}", b.display())?;
        }
        Ok(())
    }
}

/// Bring the database to `latest`. Returns `Some` only when an existing
/// database was changed.
pub(crate) fn upgrade(
    conn: &mut Connection,
    path: Option<&Path>,
    base_schema: &str,
    latest: i64,
) -> Result<Option<Migrated>> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;
    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let (from, fresh) = match stored {
        None => {
            conn.execute_batch(base_schema)?;
            set_version(conn, 1)?;
            (1, true)
        }
        Some(s) => {
            let v = s.trim().parse::<i64>().map_err(|_| {
                TaskError::Db(format!("database schema version {s:?} is not a number"))
            })?;
            (v, false)
        }
    };
    if from > latest {
        return Err(TaskError::Db(format!(
            "database schema {from} is newer than this build ({latest})"
        )));
    }
    if from < 1 {
        return Err(TaskError::Db(format!(
            "database schema {from} is not valid"
        )));
    }
    if from == latest {
        return Ok(None);
    }
    let backup = match (fresh, path) {
        (false, Some(p)) => Some(backup(conn, p, from)?),
        _ => None,
    };
    for m in MIGRATIONS
        .iter()
        .filter(|m| m.version > from && m.version <= latest)
    {
        conn.execute_batch("PRAGMA foreign_keys = OFF;")?;
        let step = apply_one(conn, m);
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        step?;
    }
    Ok((!fresh).then_some(Migrated {
        from,
        to: latest,
        backup,
    }))
}

fn set_version(conn: &Connection, v: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![v.to_string()],
    )?;
    Ok(())
}

fn apply_one(conn: &mut Connection, m: &Migration) -> Result<()> {
    let tx = conn.transaction()?;
    (m.apply)(&tx)?;
    let broken: Vec<String> = {
        let mut st = tx.prepare("PRAGMA foreign_key_check")?;
        let rows = st.query_map([], |r| {
            Ok(format!(
                "{} row {} -> {}",
                r.get::<_, String>(0)?,
                r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                r.get::<_, String>(2)?
            ))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    if !broken.is_empty() {
        return Err(TaskError::Db(format!(
            "migration to schema v{} breaks foreign keys: {}",
            m.version,
            broken.join("; ")
        )));
    }
    set_version(&tx, m.version)?;
    tx.commit()?;
    Ok(())
}

/// `VACUUM INTO` next to the database. Never overwrites an earlier backup.
fn backup(conn: &Connection, db: &Path, from: i64) -> Result<PathBuf> {
    let name = db
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "taskcraft.db".into());
    let dir = db.parent().unwrap_or_else(|| Path::new(""));
    let mut target = dir.join(format!("{name}.v{from}.bak"));
    if target.exists() {
        let stamp = Utc::now().format("%Y%m%dT%H%M%S");
        target = dir.join(format!("{name}.v{from}.{stamp}.bak"));
    }
    conn.execute(
        "VACUUM INTO ?1",
        params![target.to_string_lossy().into_owned()],
    )?;
    Ok(target)
}

// ---------- v2: Tier 5 (PM_SPEC section 2) ----------

const V2_ADD: &str = r#"
CREATE TABLE people (
    id INTEGER PRIMARY KEY,
    handle TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    teamcraft_code TEXT UNIQUE,
    active INTEGER NOT NULL DEFAULT 1
);
ALTER TABLE issues ADD COLUMN assignee_id INTEGER REFERENCES people(id);
ALTER TABLE issues ADD COLUMN parent_id INTEGER REFERENCES issues(id);
ALTER TABLE issues ADD COLUMN kind TEXT NOT NULL DEFAULT 'task'
    CHECK (kind IN ('epic','story','task','bug','subtask'));
ALTER TABLE issues ADD COLUMN version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE milestones ADD COLUMN version INTEGER NOT NULL DEFAULT 1;

CREATE TABLE comments (
    id INTEGER PRIMARY KEY,
    issue_id INTEGER NOT NULL REFERENCES issues(id),
    author_id INTEGER NOT NULL REFERENCES people(id),
    body TEXT NOT NULL,
    created_at TEXT NOT NULL,
    edited_at TEXT,
    deleted INTEGER NOT NULL DEFAULT 0,
    version INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX idx_comments_issue ON comments(issue_id);
CREATE TABLE mentions (
    comment_id INTEGER NOT NULL REFERENCES comments(id),
    person_id INTEGER NOT NULL REFERENCES people(id),
    PRIMARY KEY (comment_id, person_id)
);
CREATE TABLE watchers (
    issue_id INTEGER NOT NULL REFERENCES issues(id),
    person_id INTEGER NOT NULL REFERENCES people(id),
    PRIMARY KEY (issue_id, person_id)
);
CREATE TABLE attachments (
    id INTEGER PRIMARY KEY,
    issue_id INTEGER NOT NULL REFERENCES issues(id),
    path TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    bytes INTEGER NOT NULL,
    added_by INTEGER REFERENCES people(id),
    added_at TEXT NOT NULL,
    UNIQUE (issue_id, sha256)
);
CREATE TABLE custom_fields (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id),
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('text','number','date','select','multiselect')),
    UNIQUE (project_id, name)
);
CREATE TABLE custom_field_options (
    field_id INTEGER NOT NULL REFERENCES custom_fields(id),
    option TEXT NOT NULL,
    PRIMARY KEY (field_id, option)
);
CREATE TABLE issue_field_values (
    issue_id INTEGER NOT NULL REFERENCES issues(id),
    field_id INTEGER NOT NULL REFERENCES custom_fields(id),
    text_value TEXT,
    num_value REAL,
    date_value TEXT,
    PRIMARY KEY (issue_id, field_id)
);
CREATE TABLE issue_field_multi (
    issue_id INTEGER NOT NULL REFERENCES issues(id),
    field_id INTEGER NOT NULL REFERENCES custom_fields(id),
    option TEXT NOT NULL,
    PRIMARY KEY (issue_id, field_id, option)
);
CREATE TABLE templates (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id),
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('epic','story','task','bug','subtask')),
    priority TEXT CHECK (priority IS NULL OR priority IN ('low','medium','high','urgent')),
    title_prefix TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL DEFAULT '',
    UNIQUE (project_id, name)
);
CREATE TABLE template_labels (
    template_id INTEGER NOT NULL REFERENCES templates(id),
    label TEXT NOT NULL,
    PRIMARY KEY (template_id, label)
);
CREATE TABLE saved_views (
    id INTEGER PRIMARY KEY,
    owner_id INTEGER REFERENCES people(id),
    name TEXT NOT NULL,
    query_json TEXT NOT NULL,
    UNIQUE (owner_id, name)
);
"#;

const V2_FINISH: &str = r#"
ALTER TABLE issues DROP COLUMN assignee;
CREATE INDEX idx_issues_assignee ON issues(assignee_id);
CREATE INDEX idx_issues_parent ON issues(parent_id);

CREATE TABLE events_new (
    id INTEGER PRIMARY KEY,
    project_id INTEGER REFERENCES projects(id),
    issue_id INTEGER REFERENCES issues(id),
    at TEXT NOT NULL,
    actor TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload TEXT,
    what TEXT NOT NULL
);
INSERT INTO events_new (id, project_id, issue_id, at, actor, kind, payload, what)
    SELECT e.id, i.project_id, e.issue_id, e.at, e.actor, 'legacy', NULL, e.what
    FROM events e LEFT JOIN issues i ON i.id = e.issue_id;
DROP TABLE events;
ALTER TABLE events_new RENAME TO events;
CREATE INDEX idx_events_issue ON events(issue_id);
CREATE INDEX idx_events_project ON events(project_id);
"#;

/// People, hierarchy columns, comments, attachments, custom fields,
/// templates, saved views; `assignee` text becomes `assignee_id`; events
/// get a nullable issue, a project, a kind and a payload.
fn v2(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(V2_ADD)?;
    // One person per distinct assignee, first spelling wins as display name.
    let rows: Vec<(i64, String)> = {
        let mut st = tx.prepare(
            "SELECT id, assignee FROM issues
             WHERE assignee IS NOT NULL AND trim(assignee) <> '' ORDER BY id",
        )?;
        let it = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        it.collect::<rusqlite::Result<_>>()?
    };
    for (issue, name) in rows {
        if let Some(person) = person_id_for(tx, &name)? {
            tx.execute(
                "UPDATE issues SET assignee_id = ?1 WHERE id = ?2",
                params![person, issue],
            )?;
        }
    }
    tx.execute_batch(V2_FINISH)?;
    Ok(())
}
