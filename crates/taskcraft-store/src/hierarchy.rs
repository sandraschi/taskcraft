//! Parent/child hierarchy and rollup (Tier 5, PM_SPEC criteria 6-7).
//!
//! Rules, enforced here and nowhere else:
//! - parent and child are in the same project;
//! - an epic has no parent;
//! - nothing goes under a subtask (so a subtask is never a parent);
//! - `parent_id` never forms a cycle.
//!
//! Rollup counts every descendant (not the issue itself), skips canceled
//! issues, and treats a missing estimate as 0 points. Percent done is by
//! points; with no points at all it is by issue count; with no descendants
//! there is no rollup.

use crate::{now, IssueKind, Result, TaskError, Tracker};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rollup {
    pub issues: i64,
    pub issues_done: i64,
    pub points: i64,
    pub points_done: i64,
    /// 0-100, unrounded (the UI rounds).
    pub percent_done: f64,
    /// "points" or "issues": what `percent_done` is measured in.
    pub basis: &'static str,
}

impl Tracker {
    /// (project_id, kind) of an issue id.
    fn kind_of(&self, id: i64) -> Result<(i64, IssueKind)> {
        let (project, kind): (i64, String) = self.conn.query_row(
            "SELECT project_id, kind FROM issues WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok((project, IssueKind::parse(&kind)?))
    }

    /// Ids of all descendants of `id` (cycle-safe).
    fn descendant_ids(&self, id: i64) -> Result<Vec<i64>> {
        let mut st = self.conn.prepare(
            "WITH RECURSIVE d(id) AS (
                SELECT id FROM issues WHERE parent_id = ?1
                UNION SELECT i.id FROM issues i JOIN d ON i.parent_id = d.id
             ) SELECT id FROM d",
        )?;
        let rows = st.query_map(params![id], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Check that an issue of `kind` (with id `child`, if it exists) may
    /// sit under `parent`, and return the parent's id.
    pub(crate) fn check_parent(
        &self,
        project_id: i64,
        child: Option<i64>,
        kind: IssueKind,
        parent: &str,
    ) -> Result<i64> {
        let pid = self.resolve(parent)?;
        let parent_issue = self.issue_by_id(pid)?;
        let (parent_project, parent_kind) = self.kind_of(pid)?;
        if parent_project != project_id {
            return Err(TaskError::Invalid(format!(
                "parent {} is in another project",
                parent_issue.key
            )));
        }
        if kind == IssueKind::Epic {
            return Err(TaskError::Invalid("an epic cannot have a parent".into()));
        }
        if parent_kind == IssueKind::Subtask {
            return Err(TaskError::Invalid(format!(
                "{} is a subtask; nothing can go under a subtask",
                parent_issue.key
            )));
        }
        if let Some(c) = child {
            if c == pid || self.descendant_ids(c)?.contains(&pid) {
                return Err(TaskError::Invalid(format!(
                    "{} under {} would create a cycle",
                    self.issue_by_id(c)?.key,
                    parent_issue.key
                )));
            }
        }
        Ok(pid)
    }

    /// Check a kind change against the issue's place in the tree.
    pub(crate) fn check_kind_change(&self, id: i64, to: IssueKind) -> Result<()> {
        let has_parent: Option<i64> = self
            .conn
            .query_row(
                "SELECT parent_id FROM issues WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        if to == IssueKind::Epic && has_parent.is_some() {
            return Err(TaskError::Invalid(
                "an epic cannot have a parent; detach it first".into(),
            ));
        }
        if to == IssueKind::Subtask && !self.children(id)?.is_empty() {
            return Err(TaskError::Invalid(
                "a subtask cannot have children; move them first".into(),
            ));
        }
        Ok(())
    }

    /// Direct children as keys, by number.
    pub fn children(&self, id: i64) -> Result<Vec<String>> {
        let mut st = self.conn.prepare(
            "SELECT p.key || '-' || i.number FROM issues i JOIN projects p ON p.id = i.project_id
             WHERE i.parent_id = ?1 ORDER BY i.number",
        )?;
        let rows = st.query_map(params![id], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Move an issue under `parent`, or detach it with `None`.
    pub fn set_parent(
        &mut self,
        issue: &str,
        parent: Option<&str>,
        actor: &str,
    ) -> Result<crate::Issue> {
        let cur = self.issue(issue)?;
        let (project_id, kind) = self.kind_of(cur.id)?;
        let pid = parent
            .map(|p| self.check_parent(project_id, Some(cur.id), kind, p))
            .transpose()?;
        let parent_key = pid
            .map(|p| self.issue_by_id(p).map(|i| i.key))
            .transpose()?;
        if parent_key == cur.parent {
            return Err(TaskError::Invalid("nothing to update".into()));
        }
        let ts = now();
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE issues SET parent_id = ?1, updated_at = ?2, version = version + 1 WHERE id = ?3",
            params![pid, ts, cur.id],
        )?;
        let what = match &parent_key {
            Some(k) => format!("parent -> {k}"),
            None => format!(
                "detached from {}",
                cur.parent.as_deref().unwrap_or("its parent")
            ),
        };
        crate::insert_event_with(
            &tx,
            cur.id,
            "issue.reparented",
            &ts,
            actor,
            &what,
            Some(&json!({ "parent": parent_key, "previous": cur.parent }).to_string()),
        )?;
        tx.commit()?;
        self.issue_by_id(cur.id)
    }

    /// Points and progress over all descendants; `None` without children.
    pub fn rollup(&self, id: i64) -> Result<Option<Rollup>> {
        let ids = self.descendant_ids(id)?;
        if ids.is_empty() {
            return Ok(None);
        }
        let (mut issues, mut done, mut points, mut points_done) = (0i64, 0i64, 0i64, 0i64);
        let mut st = self
            .conn
            .prepare("SELECT status, COALESCE(estimate, 0) FROM issues WHERE id = ?1")?;
        for d in ids {
            let (status, est): (String, i64) =
                st.query_row(params![d], |r| Ok((r.get(0)?, r.get(1)?)))?;
            if status == "canceled" {
                continue;
            }
            issues += 1;
            points += est;
            if status == "done" {
                done += 1;
                points_done += est;
            }
        }
        let (percent_done, basis) = if points > 0 {
            (points_done as f64 * 100.0 / points as f64, "points")
        } else if issues > 0 {
            (done as f64 * 100.0 / issues as f64, "issues")
        } else {
            (0.0, "issues")
        };
        Ok(Some(Rollup {
            issues,
            issues_done: done,
            points,
            points_done,
            percent_done,
            basis,
        }))
    }
}
