//! taskcraft-store: the issue tracker.
//!
//! Invariants:
//! - Issue numbers are per project, gap-free, never reused (`KEY-n`).
//! - `blocks` relations form a DAG: an edge that would close a cycle is refused.
//! - An issue blocked by an unfinished issue cannot move to in_progress or done
//!   unless the caller passes `force`.
//! - Every change writes a history event (who, what, when).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use chrono::{NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::path::Path;

pub const SCHEMA_VERSION: i64 = 1;
const MAX_TITLE: usize = 300;
const MAX_TEXT: usize = 20_000;

#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    #[error("database: {0}")]
    Db(String),
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("blocked: {0}")]
    Blocked(String),
}

impl From<rusqlite::Error> for TaskError {
    fn from(e: rusqlite::Error) -> Self {
        TaskError::Db(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, TaskError>;

macro_rules! str_enum {
    ($name:ident { $($var:ident => $s:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($var),+ }
        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$var),+];
            pub fn as_str(self) -> &'static str { match self { $($name::$var => $s),+ } }
            pub fn parse(s: &str) -> Result<$name> {
                let t = s.trim().to_ascii_lowercase().replace([' ', '-'], "_");
                $name::ALL.iter().copied().find(|v| v.as_str() == t).ok_or_else(|| {
                    TaskError::Invalid(format!("unknown {} {s:?} (one of: {})", stringify!($name).to_lowercase(),
                        $name::ALL.iter().map(|v| v.as_str()).collect::<Vec<_>>().join(", ")))
                })
            }
        }
    };
}

str_enum!(Status { Backlog => "backlog", Todo => "todo", InProgress => "in_progress", InReview => "in_review", Done => "done", Canceled => "canceled" });
str_enum!(Priority { Low => "low", Medium => "medium", High => "high", Urgent => "urgent" });
str_enum!(RelationKind { Blocks => "blocks", RelatesTo => "relates_to", Duplicates => "duplicates" });

impl Status {
    pub fn is_finished(self) -> bool {
        matches!(self, Status::Done | Status::Canceled)
    }
}

impl Priority {
    fn rank(self) -> i64 {
        match self {
            Priority::Urgent => 0,
            Priority::High => 1,
            Priority::Medium => 2,
            Priority::Low => 3,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub id: i64,
    pub key: String,
    pub name: String,
    pub repo_path: Option<String>,
    pub last_synced_commit: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub id: i64,
    pub key: String,
    pub project: String,
    pub number: i64,
    pub title: String,
    pub description: String,
    pub status: Status,
    pub priority: Priority,
    pub estimate: Option<i64>,
    pub assignee: Option<String>,
    pub milestone: Option<String>,
    pub due_date: Option<NaiveDate>,
    pub labels: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Relation {
    pub kind: RelationKind,
    pub direction: &'static str,
    pub other: String,
    pub other_title: String,
    pub other_status: Status,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitLink {
    pub hash: String,
    pub author: String,
    pub committed_at: String,
    pub subject: String,
    pub closing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub at: String,
    pub actor: String,
    pub what: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IssueDetail {
    #[serde(flatten)]
    pub issue: Issue,
    pub relations: Vec<Relation>,
    pub blocked_by_open: Vec<String>,
    pub commits: Vec<CommitLink>,
    pub history: Vec<Event>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Milestone {
    pub id: i64,
    pub project: String,
    pub title: String,
    pub due_date: Option<NaiveDate>,
    pub open: bool,
    pub issues_total: i64,
    pub issues_done: i64,
    pub points_total: i64,
    pub points_done: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewIssue {
    pub project: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: Option<Status>,
    #[serde(default)]
    pub priority: Option<Priority>,
    #[serde(default)]
    pub estimate: Option<i64>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub milestone: Option<String>,
    #[serde(default)]
    pub due_date: Option<NaiveDate>,
    #[serde(default)]
    pub labels: Vec<String>,
}

/// Field changes; `None` = leave as is. Clearing an optional field uses `Some(None)`.
#[derive(Debug, Clone, Default)]
pub struct IssuePatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<Priority>,
    pub estimate: Option<Option<i64>>,
    pub assignee: Option<Option<String>>,
    pub milestone: Option<Option<String>>,
    pub due_date: Option<Option<NaiveDate>>,
    pub labels: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct IssueFilter {
    pub project: Option<String>,
    pub statuses: Vec<Status>,
    pub priority: Option<Priority>,
    pub label: Option<String>,
    pub milestone: Option<String>,
    pub text: Option<String>,
    pub include_finished: bool,
    pub limit: usize,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS projects (
    id INTEGER PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    repo_path TEXT,
    last_synced_commit TEXT,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS milestones (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id),
    title TEXT NOT NULL,
    due_date TEXT,
    open INTEGER NOT NULL DEFAULT 1,
    UNIQUE (project_id, title)
);
CREATE TABLE IF NOT EXISTS issues (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id),
    number INTEGER NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL CHECK (status IN ('backlog','todo','in_progress','in_review','done','canceled')),
    priority TEXT NOT NULL CHECK (priority IN ('low','medium','high','urgent')),
    estimate INTEGER CHECK (estimate IS NULL OR estimate BETWEEN 0 AND 1000),
    assignee TEXT,
    milestone_id INTEGER REFERENCES milestones(id),
    due_date TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    closed_at TEXT,
    UNIQUE (project_id, number)
);
CREATE INDEX IF NOT EXISTS idx_issues_status ON issues(project_id, status);
CREATE TABLE IF NOT EXISTS issue_labels (
    issue_id INTEGER NOT NULL REFERENCES issues(id),
    label TEXT NOT NULL,
    PRIMARY KEY (issue_id, label)
);
CREATE TABLE IF NOT EXISTS relations (
    source_id INTEGER NOT NULL REFERENCES issues(id),
    target_id INTEGER NOT NULL REFERENCES issues(id),
    kind TEXT NOT NULL CHECK (kind IN ('blocks','relates_to','duplicates')),
    PRIMARY KEY (source_id, target_id, kind),
    CHECK (source_id <> target_id)
);
CREATE TABLE IF NOT EXISTS commits (
    issue_id INTEGER NOT NULL REFERENCES issues(id),
    hash TEXT NOT NULL,
    author TEXT NOT NULL,
    committed_at TEXT NOT NULL,
    subject TEXT NOT NULL,
    closing INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (issue_id, hash)
);
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY,
    issue_id INTEGER NOT NULL REFERENCES issues(id),
    at TEXT NOT NULL,
    actor TEXT NOT NULL,
    what TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_events_issue ON events(issue_id);
"#;

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn d2s(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn opt_date(s: Option<String>) -> Option<NaiveDate> {
    s.and_then(|v| NaiveDate::parse_from_str(&v, "%Y-%m-%d").ok())
}

fn check_title(t: &str) -> Result<String> {
    let t = t.trim();
    if t.is_empty() {
        return Err(TaskError::Invalid("title is empty".into()));
    }
    if t.chars().count() > MAX_TITLE {
        return Err(TaskError::Invalid(format!(
            "title longer than {MAX_TITLE} characters"
        )));
    }
    Ok(t.to_string())
}

fn check_text(t: &str) -> Result<String> {
    if t.chars().count() > MAX_TEXT {
        return Err(TaskError::Invalid(format!(
            "description longer than {MAX_TEXT} characters"
        )));
    }
    Ok(t.to_string())
}

/// Project keys: 2-10 upper-case letters/digits, starting with a letter.
pub fn validate_key(k: &str) -> Result<String> {
    let k = k.trim().to_ascii_uppercase();
    let ok = (2..=10).contains(&k.len())
        && k.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && k.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    if ok {
        Ok(k)
    } else {
        Err(TaskError::Invalid(format!(
            "project key must be 2-10 letters/digits starting with a letter: {k:?}"
        )))
    }
}

fn normalize_labels(labels: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for l in labels {
        let t = l.trim().to_lowercase();
        if t.is_empty() {
            continue;
        }
        if t.chars().count() > 32 || t.contains(',') {
            return Err(TaskError::Invalid(format!(
                "bad label {l:?} (max 32 chars, no commas)"
            )));
        }
        if !out.contains(&t) {
            out.push(t);
        }
    }
    if out.len() > 20 {
        return Err(TaskError::Invalid("more than 20 labels".into()));
    }
    Ok(out)
}

/// "TC-12" -> ("TC", 12).
pub fn parse_issue_key(s: &str) -> Option<(String, i64)> {
    let s = s.trim().trim_start_matches('#');
    let (k, n) = s.rsplit_once('-')?;
    let key = validate_key(k).ok()?;
    let num = n.parse::<i64>().ok().filter(|n| *n > 0)?;
    Some((key, num))
}

pub struct Tracker {
    conn: Connection,
}

impl Tracker {
    pub fn open(path: &Path) -> Result<Tracker> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir).map_err(|e| TaskError::Db(e.to_string()))?;
            }
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Tracker> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Tracker> {
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;
        conn.execute_batch(SCHEMA)?;
        let v: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        match v.as_deref() {
            None => {
                conn.execute(
                    "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)",
                    params![SCHEMA_VERSION.to_string()],
                )?;
            }
            Some(x) if x == SCHEMA_VERSION.to_string() => {}
            Some(x) => {
                return Err(TaskError::Db(format!(
                    "database schema {x} is newer than this build ({SCHEMA_VERSION})"
                )))
            }
        }
        Ok(Tracker { conn })
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn is_demo(&self) -> Result<bool> {
        let v: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'demo'", [], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(v.as_deref() == Some("1"))
    }

    /// Meta entries whose key starts with `prefix`, newest key last.
    pub fn meta_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        let mut st = self.conn.prepare(
            "SELECT key, value FROM meta WHERE substr(key, 1, length(?1)) = ?1 ORDER BY key",
        )?;
        let rows = st
            .query_map(params![prefix], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---------- projects ----------

    pub fn create_project(
        &mut self,
        key: &str,
        name: &str,
        repo_path: Option<&str>,
    ) -> Result<Project> {
        let key = validate_key(key)?;
        let name = check_title(name)?;
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO projects (key, name, repo_path, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![key, name, repo_path, now()],
        )?;
        if n == 0 {
            return Err(TaskError::Invalid(format!("project {key} already exists")));
        }
        self.project(&key)
    }

    pub fn project(&self, key: &str) -> Result<Project> {
        let key = validate_key(key)?;
        self.conn
            .query_row(
                "SELECT id, key, name, repo_path, last_synced_commit FROM projects WHERE key = ?1",
                params![key],
                |r| {
                    Ok(Project {
                        id: r.get(0)?,
                        key: r.get(1)?,
                        name: r.get(2)?,
                        repo_path: r.get(3)?,
                        last_synced_commit: r.get(4)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| TaskError::NotFound(format!("project {key}")))
    }

    pub fn projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, key, name, repo_path, last_synced_commit FROM projects ORDER BY key",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Project {
                id: r.get(0)?,
                key: r.get(1)?,
                name: r.get(2)?,
                repo_path: r.get(3)?,
                last_synced_commit: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn set_last_synced(&self, project_id: i64, hash: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE projects SET last_synced_commit = ?1 WHERE id = ?2",
            params![hash, project_id],
        )?;
        Ok(())
    }

    // ---------- milestones ----------

    pub fn create_milestone(
        &mut self,
        project: &str,
        title: &str,
        due: Option<NaiveDate>,
    ) -> Result<Milestone> {
        let p = self.project(project)?;
        let title = check_title(title)?;
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO milestones (project_id, title, due_date) VALUES (?1, ?2, ?3)",
            params![p.id, title, due.map(d2s)],
        )?;
        if n == 0 {
            return Err(TaskError::Invalid(format!(
                "milestone {title:?} already exists in {}",
                p.key
            )));
        }
        self.milestones(Some(&p.key))?
            .into_iter()
            .find(|m| m.title == title)
            .ok_or_else(|| TaskError::NotFound(format!("milestone {title}")))
    }

    pub fn close_milestone(&mut self, project: &str, title: &str) -> Result<()> {
        let p = self.project(project)?;
        let n = self.conn.execute(
            "UPDATE milestones SET open = 0 WHERE project_id = ?1 AND title = ?2 AND open = 1",
            params![p.id, title],
        )?;
        if n == 0 {
            return Err(TaskError::NotFound(format!(
                "open milestone {title:?} in {}",
                p.key
            )));
        }
        Ok(())
    }

    pub fn milestones(&self, project: Option<&str>) -> Result<Vec<Milestone>> {
        let key = project.map(validate_key).transpose()?;
        let mut stmt = self.conn.prepare(
            "SELECT m.id, p.key, m.title, m.due_date, m.open,
                    COUNT(i.id),
                    COALESCE(SUM(CASE WHEN i.status IN ('done','canceled') THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(i.estimate), 0),
                    COALESCE(SUM(CASE WHEN i.status = 'done' THEN i.estimate ELSE 0 END), 0)
             FROM milestones m JOIN projects p ON p.id = m.project_id
             LEFT JOIN issues i ON i.milestone_id = m.id
             WHERE (?1 IS NULL OR p.key = ?1)
             GROUP BY m.id ORDER BY p.key, m.due_date IS NULL, m.due_date, m.title",
        )?;
        let rows = stmt.query_map(params![key], |r| {
            Ok(Milestone {
                id: r.get(0)?,
                project: r.get(1)?,
                title: r.get(2)?,
                due_date: opt_date(r.get(3)?),
                open: r.get::<_, i64>(4)? != 0,
                issues_total: r.get(5)?,
                issues_done: r.get(6)?,
                points_total: r.get(7)?,
                points_done: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    fn milestone_id(&self, project_id: i64, title: &str) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT id FROM milestones WHERE project_id = ?1 AND title = ?2",
                params![project_id, title],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| TaskError::NotFound(format!("milestone {title:?} (create it first)")))
    }

    // ---------- issues ----------

    fn event(&self, issue_id: i64, actor: &str, what: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO events (issue_id, at, actor, what) VALUES (?1, ?2, ?3, ?4)",
            params![
                issue_id,
                now(),
                actor,
                what.chars().take(1000).collect::<String>()
            ],
        )?;
        Ok(())
    }

    pub fn create_issue(&mut self, n: &NewIssue, actor: &str) -> Result<Issue> {
        let p = self.project(&n.project)?;
        let title = check_title(&n.title)?;
        let description = check_text(&n.description)?;
        let labels = normalize_labels(&n.labels)?;
        if let Some(e) = n.estimate {
            if !(0..=1000).contains(&e) {
                return Err(TaskError::Invalid("estimate must be 0-1000 points".into()));
            }
        }
        let milestone_id = n
            .milestone
            .as_deref()
            .map(|m| self.milestone_id(p.id, m))
            .transpose()?;
        let status = n.status.unwrap_or(Status::Backlog);
        let tx = self.conn.transaction()?;
        let number: i64 = tx.query_row(
            "SELECT COALESCE(MAX(number), 0) + 1 FROM issues WHERE project_id = ?1",
            params![p.id],
            |r| r.get(0),
        )?;
        let ts = now();
        tx.execute(
            "INSERT INTO issues (project_id, number, title, description, status, priority, estimate, assignee, milestone_id, due_date, created_at, updated_at, closed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, ?12)",
            params![
                p.id, number, title, description, status.as_str(), n.priority.unwrap_or(Priority::Medium).as_str(),
                n.estimate, n.assignee.as_deref().map(str::trim), milestone_id, n.due_date.map(d2s), ts,
                if status.is_finished() { Some(ts.clone()) } else { None }
            ],
        )?;
        let id = tx.last_insert_rowid();
        for l in &labels {
            tx.execute(
                "INSERT INTO issue_labels (issue_id, label) VALUES (?1, ?2)",
                params![id, l],
            )?;
        }
        tx.execute(
            "INSERT INTO events (issue_id, at, actor, what) VALUES (?1, ?2, ?3, ?4)",
            params![id, ts, actor, format!("created as {}", status.as_str())],
        )?;
        tx.commit()?;
        self.issue_by_id(id)
    }

    /// Resolve "TC-12" or a numeric id string.
    pub fn resolve(&self, key_or_id: &str) -> Result<i64> {
        if let Some((key, num)) = parse_issue_key(key_or_id) {
            return self
                .conn
                .query_row(
                    "SELECT i.id FROM issues i JOIN projects p ON p.id = i.project_id WHERE p.key = ?1 AND i.number = ?2",
                    params![key, num],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or_else(|| TaskError::NotFound(format!("issue {key}-{num}")));
        }
        let id: i64 = key_or_id
            .trim()
            .parse()
            .map_err(|_| TaskError::Invalid(format!("not an issue key or id: {key_or_id:?}")))?;
        self.issue_by_id(id).map(|i| i.id)
    }

    const ISSUE_SELECT: &'static str = "SELECT i.id, p.key, i.number, i.title, i.description, i.status, i.priority, i.estimate, i.assignee,
        m.title, i.due_date, i.created_at, i.updated_at, i.closed_at
        FROM issues i JOIN projects p ON p.id = i.project_id LEFT JOIN milestones m ON m.id = i.milestone_id";

    fn row_to_issue(r: &rusqlite::Row<'_>) -> rusqlite::Result<(Issue, String, String)> {
        let key: String = r.get(1)?;
        let number: i64 = r.get(2)?;
        Ok((
            Issue {
                id: r.get(0)?,
                key: format!("{key}-{number}"),
                project: key,
                number,
                title: r.get(3)?,
                description: r.get(4)?,
                status: Status::Backlog,
                priority: Priority::Medium,
                estimate: r.get(7)?,
                assignee: r.get(8)?,
                milestone: r.get(9)?,
                due_date: opt_date(r.get(10)?),
                labels: Vec::new(),
                created_at: r.get(11)?,
                updated_at: r.get(12)?,
                closed_at: r.get(13)?,
            },
            r.get(5)?,
            r.get(6)?,
        ))
    }

    fn finish_issue(&self, (mut i, status, priority): (Issue, String, String)) -> Result<Issue> {
        i.status = Status::parse(&status)?;
        i.priority = Priority::parse(&priority)?;
        let mut stmt = self
            .conn
            .prepare("SELECT label FROM issue_labels WHERE issue_id = ?1 ORDER BY label")?;
        i.labels = stmt
            .query_map(params![i.id], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(i)
    }

    pub fn issue_by_id(&self, id: i64) -> Result<Issue> {
        let sql = format!("{} WHERE i.id = ?1", Self::ISSUE_SELECT);
        let raw = self
            .conn
            .query_row(&sql, params![id], Self::row_to_issue)
            .optional()?
            .ok_or_else(|| TaskError::NotFound(format!("issue id {id}")))?;
        self.finish_issue(raw)
    }

    pub fn issue(&self, key_or_id: &str) -> Result<Issue> {
        self.issue_by_id(self.resolve(key_or_id)?)
    }

    pub fn list(&self, f: &IssueFilter) -> Result<Vec<Issue>> {
        let key = f.project.as_deref().map(validate_key).transpose()?;
        let statuses: Vec<&str> = f.statuses.iter().map(|s| s.as_str()).collect();
        let status_csv = statuses.join(",");
        let sql = format!(
            "{} WHERE (?1 IS NULL OR p.key = ?1)
               AND (?2 = '' OR instr(',' || ?2 || ',', ',' || i.status || ',') > 0)
               AND (?3 = 1 OR ?2 <> '' OR i.status NOT IN ('done','canceled'))
               AND (?4 IS NULL OR i.priority = ?4)
               AND (?5 IS NULL OR EXISTS (SELECT 1 FROM issue_labels l WHERE l.issue_id = i.id AND l.label = ?5))
               AND (?6 IS NULL OR m.title = ?6)
               AND (?7 IS NULL OR instr(lower(i.title || ' ' || i.description), lower(?7)) > 0)
             ORDER BY CASE i.priority WHEN 'urgent' THEN 0 WHEN 'high' THEN 1 WHEN 'medium' THEN 2 ELSE 3 END, p.key, i.number
             LIMIT ?8",
            Self::ISSUE_SELECT
        );
        let limit =
            i64::try_from(if f.limit == 0 { 500 } else { f.limit.min(5000) }).unwrap_or(500);
        let mut stmt = self.conn.prepare(&sql)?;
        let raws = stmt
            .query_map(
                params![
                    key,
                    status_csv,
                    i64::from(f.include_finished),
                    f.priority.map(Priority::as_str),
                    f.label.as_deref().map(str::to_lowercase),
                    f.milestone,
                    f.text,
                    limit
                ],
                Self::row_to_issue,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        raws.into_iter().map(|r| self.finish_issue(r)).collect()
    }

    pub fn update_issue(
        &mut self,
        key_or_id: &str,
        patch: &IssuePatch,
        actor: &str,
    ) -> Result<Issue> {
        let cur = self.issue(key_or_id)?;
        let p = self.project(&cur.project)?;
        let mut changes: Vec<String> = Vec::new();
        let tx_title = patch.title.as_deref().map(check_title).transpose()?;
        let tx_desc = patch.description.as_deref().map(check_text).transpose()?;
        let labels = patch.labels.as_deref().map(normalize_labels).transpose()?;
        if let Some(Some(e)) = patch.estimate {
            if !(0..=1000).contains(&e) {
                return Err(TaskError::Invalid("estimate must be 0-1000 points".into()));
            }
        }
        let milestone_id = match &patch.milestone {
            Some(Some(m)) => Some(Some(self.milestone_id(p.id, m)?)),
            Some(None) => Some(None),
            None => None,
        };
        let tx = self.conn.transaction()?;
        let ts = now();
        if let Some(t) = &tx_title {
            tx.execute(
                "UPDATE issues SET title = ?1 WHERE id = ?2",
                params![t, cur.id],
            )?;
            changes.push(format!("title -> {t:?}"));
        }
        if let Some(d) = &tx_desc {
            tx.execute(
                "UPDATE issues SET description = ?1 WHERE id = ?2",
                params![d, cur.id],
            )?;
            changes.push("description edited".into());
        }
        if let Some(pr) = patch.priority {
            tx.execute(
                "UPDATE issues SET priority = ?1 WHERE id = ?2",
                params![pr.as_str(), cur.id],
            )?;
            changes.push(format!("priority -> {}", pr.as_str()));
        }
        if let Some(e) = patch.estimate {
            tx.execute(
                "UPDATE issues SET estimate = ?1 WHERE id = ?2",
                params![e, cur.id],
            )?;
            changes.push(format!(
                "estimate -> {}",
                e.map_or("none".into(), |v| v.to_string())
            ));
        }
        if let Some(a) = &patch.assignee {
            tx.execute(
                "UPDATE issues SET assignee = ?1 WHERE id = ?2",
                params![a.as_deref().map(str::trim), cur.id],
            )?;
            changes.push(format!("assignee -> {}", a.as_deref().unwrap_or("none")));
        }
        if let Some(m) = milestone_id {
            tx.execute(
                "UPDATE issues SET milestone_id = ?1 WHERE id = ?2",
                params![m, cur.id],
            )?;
            changes.push(format!(
                "milestone -> {}",
                patch
                    .milestone
                    .clone()
                    .flatten()
                    .unwrap_or_else(|| "none".into())
            ));
        }
        if let Some(d) = patch.due_date {
            tx.execute(
                "UPDATE issues SET due_date = ?1 WHERE id = ?2",
                params![d.map(d2s), cur.id],
            )?;
            changes.push(format!("due -> {}", d.map_or("none".into(), d2s)));
        }
        if let Some(ls) = &labels {
            tx.execute(
                "DELETE FROM issue_labels WHERE issue_id = ?1",
                params![cur.id],
            )?;
            for l in ls {
                tx.execute(
                    "INSERT INTO issue_labels (issue_id, label) VALUES (?1, ?2)",
                    params![cur.id, l],
                )?;
            }
            changes.push(format!("labels -> [{}]", ls.join(", ")));
        }
        if changes.is_empty() {
            return Err(TaskError::Invalid("nothing to update".into()));
        }
        tx.execute(
            "UPDATE issues SET updated_at = ?1 WHERE id = ?2",
            params![ts, cur.id],
        )?;
        tx.execute(
            "INSERT INTO events (issue_id, at, actor, what) VALUES (?1, ?2, ?3, ?4)",
            params![cur.id, ts, actor, changes.join("; ")],
        )?;
        tx.commit()?;
        self.issue_by_id(cur.id)
    }

    /// Open issues that block `id`.
    pub fn open_blockers(&self, id: i64) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.key || '-' || i.number FROM relations r JOIN issues i ON i.id = r.source_id JOIN projects p ON p.id = i.project_id
             WHERE r.target_id = ?1 AND r.kind = 'blocks' AND i.status NOT IN ('done','canceled') ORDER BY 1",
        )?;
        let rows = stmt.query_map(params![id], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn move_issue(
        &mut self,
        key_or_id: &str,
        to: Status,
        force: bool,
        actor: &str,
    ) -> Result<Issue> {
        let cur = self.issue(key_or_id)?;
        if cur.status == to {
            return Err(TaskError::Invalid(format!(
                "{} is already {}",
                cur.key,
                to.as_str()
            )));
        }
        if matches!(to, Status::InProgress | Status::InReview | Status::Done) && !force {
            let blockers = self.open_blockers(cur.id)?;
            if !blockers.is_empty() {
                return Err(TaskError::Blocked(format!(
                    "{} is blocked by {} (pass force to override)",
                    cur.key,
                    blockers.join(", ")
                )));
            }
        }
        let ts = now();
        let closed = if to.is_finished() {
            Some(ts.clone())
        } else {
            None
        };
        self.conn.execute(
            "UPDATE issues SET status = ?1, updated_at = ?2, closed_at = ?3 WHERE id = ?4",
            params![to.as_str(), ts, closed, cur.id],
        )?;
        let note = if force { " (forced)" } else { "" };
        self.event(
            cur.id,
            actor,
            &format!("{} -> {}{note}", cur.status.as_str(), to.as_str()),
        )?;
        self.issue_by_id(cur.id)
    }

    // ---------- relations ----------

    /// Does `from` reach `to` along `blocks` edges?
    fn blocks_path(&self, from: i64, to: i64) -> Result<bool> {
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([from]);
        let mut stmt = self
            .conn
            .prepare("SELECT target_id FROM relations WHERE source_id = ?1 AND kind = 'blocks'")?;
        while let Some(n) = queue.pop_front() {
            if n == to {
                return Ok(true);
            }
            if !seen.insert(n) || seen.len() > 100_000 {
                continue;
            }
            for next in stmt.query_map(params![n], |r| r.get::<_, i64>(0))? {
                queue.push_back(next?);
            }
        }
        Ok(false)
    }

    pub fn add_relation(
        &mut self,
        source: &str,
        kind: RelationKind,
        target: &str,
        actor: &str,
    ) -> Result<()> {
        let s = self.issue(source)?;
        let t = self.issue(target)?;
        if s.id == t.id {
            return Err(TaskError::Invalid(
                "an issue cannot relate to itself".into(),
            ));
        }
        if kind == RelationKind::Blocks && self.blocks_path(t.id, s.id)? {
            return Err(TaskError::Invalid(format!(
                "{} blocks {} would create a dependency cycle",
                s.key, t.key
            )));
        }
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO relations (source_id, target_id, kind) VALUES (?1, ?2, ?3)",
            params![s.id, t.id, kind.as_str()],
        )?;
        if n == 0 {
            return Err(TaskError::Invalid("relation already exists".into()));
        }
        self.event(s.id, actor, &format!("{} {}", kind.as_str(), t.key))?;
        self.event(t.id, actor, &format!("{} by {}", kind.as_str(), s.key))?;
        Ok(())
    }

    pub fn remove_relation(
        &mut self,
        source: &str,
        kind: RelationKind,
        target: &str,
        actor: &str,
    ) -> Result<()> {
        let s = self.issue(source)?;
        let t = self.issue(target)?;
        let n = self.conn.execute(
            "DELETE FROM relations WHERE source_id = ?1 AND target_id = ?2 AND kind = ?3",
            params![s.id, t.id, kind.as_str()],
        )?;
        if n == 0 {
            return Err(TaskError::NotFound("relation".into()));
        }
        self.event(s.id, actor, &format!("removed {} {}", kind.as_str(), t.key))?;
        Ok(())
    }

    pub fn detail(&self, key_or_id: &str) -> Result<IssueDetail> {
        let issue = self.issue(key_or_id)?;
        let mut relations = Vec::new();
        for (sql, dir) in [
            ("SELECT r.kind, i.id FROM relations r JOIN issues i ON i.id = r.target_id WHERE r.source_id = ?1", "outgoing"),
            ("SELECT r.kind, i.id FROM relations r JOIN issues i ON i.id = r.source_id WHERE r.target_id = ?1", "incoming"),
        ] {
            let mut stmt = self.conn.prepare(sql)?;
            let rows = stmt.query_map(params![issue.id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?.collect::<std::result::Result<Vec<_>, _>>()?;
            for (kind, other) in rows {
                let o = self.issue_by_id(other)?;
                relations.push(Relation { kind: RelationKind::parse(&kind)?, direction: dir, other: o.key, other_title: o.title, other_status: o.status });
            }
        }
        let mut stmt = self.conn.prepare("SELECT hash, author, committed_at, subject, closing FROM commits WHERE issue_id = ?1 ORDER BY committed_at")?;
        let commits = stmt
            .query_map(params![issue.id], |r| {
                Ok(CommitLink {
                    hash: r.get(0)?,
                    author: r.get(1)?,
                    committed_at: r.get(2)?,
                    subject: r.get(3)?,
                    closing: r.get::<_, i64>(4)? != 0,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut stmt = self
            .conn
            .prepare("SELECT at, actor, what FROM events WHERE issue_id = ?1 ORDER BY id")?;
        let history = stmt
            .query_map(params![issue.id], |r| {
                Ok(Event {
                    at: r.get(0)?,
                    actor: r.get(1)?,
                    what: r.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let blocked_by_open = self.open_blockers(issue.id)?;
        Ok(IssueDetail {
            issue,
            relations,
            blocked_by_open,
            commits,
            history,
        })
    }

    /// Link a commit; returns false if it was already linked.
    pub fn link_commit(&mut self, issue_id: i64, c: &CommitLink) -> Result<bool> {
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO commits (issue_id, hash, author, committed_at, subject, closing) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![issue_id, c.hash, c.author, c.committed_at, c.subject.chars().take(500).collect::<String>(), i64::from(c.closing)],
        )?;
        if n > 0 {
            let short: String = c.hash.chars().take(8).collect();
            self.event(
                issue_id,
                &c.author,
                &format!("commit {short}: {}", c.subject),
            )?;
        }
        Ok(n > 0)
    }

    pub fn stats(&self) -> Result<serde_json::Value> {
        let q = |sql: &str| -> Result<i64> { Ok(self.conn.query_row(sql, [], |r| r.get(0))?) };
        Ok(serde_json::json!({
            "projects": q("SELECT COUNT(*) FROM projects")?,
            "issues": q("SELECT COUNT(*) FROM issues")?,
            "open": q("SELECT COUNT(*) FROM issues WHERE status NOT IN ('done','canceled')")?,
            "in_progress": q("SELECT COUNT(*) FROM issues WHERE status = 'in_progress'")?,
            "urgent_open": q("SELECT COUNT(*) FROM issues WHERE priority = 'urgent' AND status NOT IN ('done','canceled')")?,
            "overdue": q(&format!("SELECT COUNT(*) FROM issues WHERE due_date < '{}' AND status NOT IN ('done','canceled')", d2s(Utc::now().date_naive())))?,
            "demo": self.is_demo()?,
        }))
    }

    /// Sort helper for callers that rank by priority.
    pub fn priority_rank(p: Priority) -> i64 {
        p.rank()
    }
}

#[cfg(test)]
mod tests;
