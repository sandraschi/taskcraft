//! People, comments, mentions and watchers (Tier 5, PM_SPEC criteria 2-4).
//!
//! Rules:
//! - A comment's author is the person for the acting user (found or created
//!   from the actor name, like an assignee).
//! - `@handle` in a body mentions an active person: one `mentions` row, and
//!   the person becomes a watcher. Unknown handles are reported, never
//!   created, and the comment is still saved.
//! - Only the author edits or deletes. Editing keeps `created_at`, sets
//!   `edited_at`, bumps `version`. Deleting is soft: the body stays.
//! - A caller may pass the version it saw; a different current version is
//!   refused, so two writers cannot overwrite each other silently.
//! - Every change writes a typed history event with a JSON payload.

use crate::{now, person_handle, person_id_for, Result, TaskError, Tracker, MAX_TEXT};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Person {
    pub id: i64,
    pub handle: String,
    pub display_name: String,
    pub teamcraft_code: Option<String>,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Comment {
    pub id: i64,
    pub issue: String,
    pub author: String,
    pub author_name: String,
    pub body: String,
    pub created_at: String,
    pub edited_at: Option<String>,
    pub deleted: bool,
    pub version: i64,
    pub mentions: Vec<String>,
}

/// A saved comment plus what its mentions did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommentResult {
    #[serde(flatten)]
    pub comment: Comment,
    /// `@handles` with no active person. The comment is saved anyway.
    pub unknown_mentions: Vec<String>,
    /// Mentioned people who were not watching before.
    pub new_watchers: Vec<String>,
}

/// `@handle` tokens in order, lower-case, without duplicates. An `@` right
/// after a letter, digit, `.`, `_` or `-` is not a mention (e-mail
/// addresses). Trailing `.` and `-` are punctuation, not part of the handle.
pub fn mention_handles(body: &str) -> Vec<String> {
    let word = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-');
    let chars: Vec<char> = body.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '@' && (i == 0 || !word(chars[i - 1])) {
            let mut j = i + 1;
            while j < chars.len() && word(chars[j]) {
                j += 1;
            }
            let h: String = chars[i + 1..j].iter().collect::<String>().to_lowercase();
            let h = h.trim_end_matches(['.', '-']).to_string();
            if h.len() >= 2 && h.len() <= 32 && !out.contains(&h) {
                out.push(h);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

fn check_body(body: &str) -> Result<String> {
    let b = body.trim();
    if b.is_empty() {
        return Err(TaskError::Invalid("comment is empty".into()));
    }
    if b.chars().count() > MAX_TEXT {
        return Err(TaskError::Invalid(format!(
            "comment longer than {MAX_TEXT} characters"
        )));
    }
    Ok(b.to_string())
}

fn row_to_person(r: &rusqlite::Row<'_>) -> rusqlite::Result<Person> {
    Ok(Person {
        id: r.get(0)?,
        handle: r.get(1)?,
        display_name: r.get(2)?,
        teamcraft_code: r.get(3)?,
        active: r.get::<_, i64>(4)? != 0,
    })
}

const PERSON_COLS: &str = "id, handle, display_name, teamcraft_code, active";

/// Existing person by handle or display name; never creates one.
fn find_person(conn: &Connection, who: &str) -> Result<Option<Person>> {
    let who = who.trim().trim_start_matches('@');
    if who.is_empty() {
        return Ok(None);
    }
    Ok(conn
        .query_row(
            &format!(
                "SELECT {PERSON_COLS} FROM people
                 WHERE handle = ?1 OR handle = ?2 OR lower(display_name) = lower(?3)
                 ORDER BY id LIMIT 1"
            ),
            params![who.to_lowercase(), person_handle(who), who],
            row_to_person,
        )
        .optional()?)
}

/// Insert mention rows and watchers for the handles in `body`.
/// Returns (mentioned handles, unknown handles, new watchers).
fn apply_mentions(
    conn: &Connection,
    comment_id: i64,
    issue_id: i64,
    body: &str,
) -> Result<(Vec<String>, Vec<String>, Vec<String>)> {
    let (mut known, mut unknown, mut watchers) = (Vec::new(), Vec::new(), Vec::new());
    for h in mention_handles(body) {
        let id: Option<i64> = conn
            .query_row(
                "SELECT id FROM people WHERE handle = ?1 AND active = 1",
                params![h],
                |r| r.get(0),
            )
            .optional()?;
        let Some(pid) = id else {
            unknown.push(h);
            continue;
        };
        conn.execute(
            "INSERT OR IGNORE INTO mentions (comment_id, person_id) VALUES (?1, ?2)",
            params![comment_id, pid],
        )?;
        let added = conn.execute(
            "INSERT OR IGNORE INTO watchers (issue_id, person_id) VALUES (?1, ?2)",
            params![issue_id, pid],
        )?;
        if added > 0 {
            watchers.push(h.clone());
        }
        known.push(h);
    }
    Ok((known, unknown, watchers))
}

fn excerpt(body: &str) -> String {
    let one_line = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut s: String = one_line.chars().take(80).collect();
    if one_line.chars().count() > 80 {
        s.push_str("...");
    }
    s
}

impl Tracker {
    // ---------- people ----------

    pub fn people(&self, include_inactive: bool) -> Result<Vec<Person>> {
        let sql = format!(
            "SELECT {PERSON_COLS} FROM people {} ORDER BY handle",
            if include_inactive {
                ""
            } else {
                "WHERE active = 1"
            }
        );
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map([], row_to_person)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Person by handle (with or without `@`) or display name.
    pub fn person(&self, who: &str) -> Result<Person> {
        find_person(&self.conn, who)?.ok_or_else(|| TaskError::NotFound(format!("person {who:?}")))
    }

    /// New person. Without a handle one is made from the name.
    pub fn add_person(
        &mut self,
        display_name: &str,
        handle: Option<&str>,
        teamcraft_code: Option<&str>,
    ) -> Result<Person> {
        let name = display_name.trim();
        if name.is_empty() || name.chars().count() > 100 {
            return Err(TaskError::Invalid(
                "display name must be 1-100 characters".into(),
            ));
        }
        let handle = match handle.map(str::trim).filter(|h| !h.is_empty()) {
            Some(h) => {
                let h = h.trim_start_matches('@').to_lowercase();
                if person_handle(&h) != h || h.len() < 2 {
                    return Err(TaskError::Invalid(format!(
                        "handle {h:?} must be 2-32 of a-z, 0-9, '.', '_', '-' (suggested: {})",
                        person_handle(&h)
                    )));
                }
                h
            }
            None => person_handle(name),
        };
        let code = teamcraft_code.map(str::trim).filter(|c| !c.is_empty());
        let taken: Option<String> = self
            .conn
            .query_row(
                "SELECT handle FROM people WHERE handle = ?1 OR (teamcraft_code IS NOT NULL AND teamcraft_code = ?2)",
                params![handle, code],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(t) = taken {
            return Err(TaskError::Invalid(format!(
                "handle or teamcraft code already used by @{t}"
            )));
        }
        self.conn.execute(
            "INSERT INTO people (handle, display_name, teamcraft_code) VALUES (?1, ?2, ?3)",
            params![handle, name, code],
        )?;
        self.person(&handle)
    }

    // ---------- comments ----------

    fn comment_row(&self, id: i64) -> Result<(Comment, i64, i64)> {
        let row = self
            .conn
            .query_row(
                "SELECT c.id, p.key || '-' || i.number, a.handle, a.display_name, c.body,
                        c.created_at, c.edited_at, c.deleted, c.version, c.issue_id, c.author_id
                 FROM comments c JOIN issues i ON i.id = c.issue_id
                 JOIN projects p ON p.id = i.project_id JOIN people a ON a.id = c.author_id
                 WHERE c.id = ?1",
                params![id],
                |r| {
                    Ok((
                        Comment {
                            id: r.get(0)?,
                            issue: r.get(1)?,
                            author: r.get(2)?,
                            author_name: r.get(3)?,
                            body: r.get(4)?,
                            created_at: r.get(5)?,
                            edited_at: r.get(6)?,
                            deleted: r.get::<_, i64>(7)? != 0,
                            version: r.get(8)?,
                            mentions: Vec::new(),
                        },
                        r.get::<_, i64>(9)?,
                        r.get::<_, i64>(10)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| TaskError::NotFound(format!("comment {id}")))?;
        let (mut c, issue_id, author_id) = row;
        c.mentions = self.mentions_of(id)?;
        Ok((c, issue_id, author_id))
    }

    fn mentions_of(&self, comment_id: i64) -> Result<Vec<String>> {
        let mut st = self.conn.prepare(
            "SELECT p.handle FROM mentions m JOIN people p ON p.id = m.person_id
             WHERE m.comment_id = ?1 ORDER BY p.handle",
        )?;
        let rows = st.query_map(params![comment_id], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn comment(&self, id: i64) -> Result<Comment> {
        Ok(self.comment_row(id)?.0)
    }

    /// Comments of an issue, oldest first, deleted ones included (flagged).
    pub fn comments(&self, issue: &str) -> Result<Vec<Comment>> {
        let issue_id = self.resolve(issue)?;
        let ids: Vec<i64> = {
            let mut st = self
                .conn
                .prepare("SELECT id FROM comments WHERE issue_id = ?1 ORDER BY id")?;
            let rows = st.query_map(params![issue_id], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        ids.into_iter().map(|id| self.comment(id)).collect()
    }

    pub fn add_comment(&mut self, issue: &str, body: &str, actor: &str) -> Result<CommentResult> {
        let issue_id = self.resolve(issue)?;
        let body = check_body(body)?;
        let ts = now();
        let tx = self.conn.transaction()?;
        let author = person_id_for(&tx, actor)?
            .ok_or_else(|| TaskError::Invalid("a comment needs an author".into()))?;
        tx.execute(
            "INSERT INTO comments (issue_id, author_id, body, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![issue_id, author, body, ts],
        )?;
        let id = tx.last_insert_rowid();
        let (known, unknown, watchers) = apply_mentions(&tx, id, issue_id, &body)?;
        crate::insert_event_with(
            &tx,
            issue_id,
            "comment.added",
            &ts,
            actor,
            &format!("comment #{id}: {}", excerpt(&body)),
            Some(&json!({ "comment": id, "mentions": known }).to_string()),
        )?;
        tx.commit()?;
        Ok(CommentResult {
            comment: self.comment(id)?,
            unknown_mentions: unknown,
            new_watchers: watchers,
        })
    }

    /// Check author, deleted flag and the version the caller saw.
    fn editable(&self, id: i64, expected: Option<i64>, actor: &str) -> Result<(Comment, i64)> {
        let (c, issue_id, author_id) = self.comment_row(id)?;
        let me = find_person(&self.conn, actor)?.map(|p| p.id);
        if me != Some(author_id) {
            return Err(TaskError::Invalid(format!(
                "only the author (@{}) can change comment {id}",
                c.author
            )));
        }
        if c.deleted {
            return Err(TaskError::Invalid(format!("comment {id} is deleted")));
        }
        if let Some(v) = expected {
            if v != c.version {
                return Err(TaskError::Invalid(format!(
                    "comment {id} changed meanwhile (now version {}, you had {v}); reload it",
                    c.version
                )));
            }
        }
        Ok((c, issue_id))
    }

    pub fn edit_comment(
        &mut self,
        id: i64,
        body: &str,
        expected_version: Option<i64>,
        actor: &str,
    ) -> Result<CommentResult> {
        let body = check_body(body)?;
        let (old, issue_id) = self.editable(id, expected_version, actor)?;
        if old.body == body {
            return Err(TaskError::Invalid("nothing to update".into()));
        }
        let ts = now();
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE comments SET body = ?1, edited_at = ?2, version = version + 1 WHERE id = ?3",
            params![body, ts, id],
        )?;
        tx.execute("DELETE FROM mentions WHERE comment_id = ?1", params![id])?;
        let (known, unknown, watchers) = apply_mentions(&tx, id, issue_id, &body)?;
        crate::insert_event_with(
            &tx,
            issue_id,
            "comment.edited",
            &ts,
            actor,
            &format!("edited comment #{id}: {}", excerpt(&body)),
            Some(
                &json!({ "comment": id, "version": old.version + 1, "mentions": known })
                    .to_string(),
            ),
        )?;
        tx.commit()?;
        Ok(CommentResult {
            comment: self.comment(id)?,
            unknown_mentions: unknown,
            new_watchers: watchers,
        })
    }

    /// Soft delete: flagged, body kept for history.
    pub fn delete_comment(
        &mut self,
        id: i64,
        expected_version: Option<i64>,
        actor: &str,
    ) -> Result<Comment> {
        let (old, issue_id) = self.editable(id, expected_version, actor)?;
        let ts = now();
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE comments SET deleted = 1, version = version + 1 WHERE id = ?1",
            params![id],
        )?;
        crate::insert_event_with(
            &tx,
            issue_id,
            "comment.deleted",
            &ts,
            actor,
            &format!("deleted comment #{id}"),
            Some(&json!({ "comment": id, "version": old.version + 1 }).to_string()),
        )?;
        tx.commit()?;
        self.comment(id)
    }

    // ---------- watchers ----------

    pub fn watchers(&self, issue: &str) -> Result<Vec<Person>> {
        let issue_id = self.resolve(issue)?;
        let mut st = self.conn.prepare(&format!(
            "SELECT {} FROM watchers w JOIN people p ON p.id = w.person_id
             WHERE w.issue_id = ?1 ORDER BY p.handle",
            PERSON_COLS
                .split(", ")
                .map(|c| format!("p.{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        ))?;
        let rows = st.query_map(params![issue_id], row_to_person)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Start or stop watching. Returns false when nothing changed.
    pub fn set_watch(&mut self, issue: &str, who: &str, on: bool, actor: &str) -> Result<bool> {
        let issue_id = self.resolve(issue)?;
        let p = self.person(who)?;
        let n = if on {
            self.conn.execute(
                "INSERT OR IGNORE INTO watchers (issue_id, person_id) VALUES (?1, ?2)",
                params![issue_id, p.id],
            )?
        } else {
            self.conn.execute(
                "DELETE FROM watchers WHERE issue_id = ?1 AND person_id = ?2",
                params![issue_id, p.id],
            )?
        };
        if n > 0 {
            let (kind, verb) = if on {
                ("watcher.added", "now watching")
            } else {
                ("watcher.removed", "stopped watching")
            };
            crate::insert_event_with(
                &self.conn,
                issue_id,
                kind,
                &now(),
                actor,
                &format!("@{} {verb}", p.handle),
                Some(&json!({ "person": p.handle }).to_string()),
            )?;
        }
        Ok(n > 0)
    }
}
