//! `taskcraft-app`: Native eframe/egui desktop workstation for TaskCraft.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use chrono::{Local, NaiveDate};
use clap::Parser;
use craft_biz::bus::Bus;
use craft_biz_ui::bookend::{self, Bar, Spec};
use craft_biz_ui::kanban::{Board, Card, Column, Event, Lane};
use craft_biz_ui::menu::{Item, Menu};
use craft_biz_ui::palette::Palette;
use craft_biz_ui::shell::{self, settings_path, Toasts, UiSettings};
use craft_biz_ui::signals::{SignalAction, SignalsDock};
use craft_biz_ui::table::{Cell, Col, Table, TableEvent};
use craft_biz_ui::theme::{self, semibold, Tone};
use craft_biz_ui::widgets;
use eframe::egui::{self, CornerRadius, ProgressBar, RichText, ScrollArea, Stroke};
use std::collections::HashSet;
use std::path::PathBuf;
use taskcraft_core::{dispatch, tools, Command, Confinement, Ctx};
use taskcraft_store::{
    Issue, IssueDetail, IssueFilter, IssueKind, IssuePatch, Milestone, NewIssue, Priority, Project,
    RelationKind, Status, Tracker,
};

const APP_NAME: &str = "TaskCraft";

#[derive(Parser, Debug)]
#[command(
    name = "taskcraft-app",
    version,
    about = "Desktop UI for TaskCraft issue tracker"
)]
struct Cli {
    /// Database file path.
    #[arg(long)]
    db: Option<PathBuf>,
    /// Smoke test: verify startup, tool catalogue, and exit immediately.
    #[arg(long)]
    smoke: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveView {
    Board,
    Table,
    Milestones,
    GitSync,
    History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwimlaneMode {
    None,
    Priority,
    Assignee,
}

fn status_tone(s: Status) -> Tone {
    match s {
        Status::Backlog => Tone::Neutral,
        Status::Todo => Tone::Info,
        Status::InProgress => Tone::Accent,
        Status::InReview => Tone::Warn,
        Status::Done => Tone::Ok,
        Status::Canceled => Tone::Neutral,
    }
}

fn kind_tone(k: IssueKind) -> Tone {
    match k {
        IssueKind::Epic => Tone::Accent,
        IssueKind::Story => Tone::Info,
        IssueKind::Bug => Tone::Danger,
        IssueKind::Task | IssueKind::Subtask => Tone::Neutral,
    }
}

fn priority_tone(p: Priority) -> Tone {
    match p {
        Priority::Urgent => Tone::Danger,
        Priority::High => Tone::Warn,
        Priority::Medium => Tone::Info,
        Priority::Low => Tone::Neutral,
    }
}

struct App {
    ctx: Ctx,
    db_path: PathBuf,
    settings: UiSettings,
    settings_file: PathBuf,
    toasts: Toasts,
    palette: Palette,
    dock: SignalsDock,
    current_view: ActiveView,

    // Data cache
    projects: Vec<Project>,
    selected_project: Option<String>,
    all_issues: Vec<Issue>,
    milestones: Vec<Milestone>,
    cards: Vec<Card>,

    // Selected issue & inspector drawer
    selected_issue_key: Option<String>,
    selected_detail: Option<IssueDetail>,
    inspector_open: bool,

    // Draft edit state for currently inspected issue
    edit_title: String,
    edit_description: String,
    edit_assignee: String,
    edit_estimate: String,
    edit_due_date: String,
    edit_milestone: String,
    new_label_input: String,
    new_rel_kind: RelationKind,
    new_rel_target: String,
    new_comment: String,
    new_parent: String,

    // Board filter and swimlanes
    filter_text: String,
    filter_priority: Option<Priority>,
    filter_assignee: Option<String>,
    filter_status: Option<Status>,
    swimlane_mode: SwimlaneMode,

    // Modals
    show_new_issue: bool,
    new_issue_title: String,
    new_issue_desc: String,
    new_issue_status: Status,
    new_issue_priority: Priority,
    new_issue_assignee: String,
    new_issue_estimate: String,
    new_issue_due_date: String,
    new_issue_milestone: String,
    new_issue_labels: String,

    show_new_project: bool,
    new_project_key: String,
    new_project_name: String,
    new_project_repo: String,

    show_new_milestone: bool,
    new_milestone_title: String,
    new_milestone_due: String,

    show_force_move: bool,
    force_move_target: Option<Status>,
    bar: Bar,
}

impl App {
    fn new(ctx: Ctx, db_path: PathBuf) -> Self {
        let settings_file = settings_path(&db_path);
        let settings = UiSettings::load(&settings_file);
        let palette = Palette::default();

        let mut app = Self {
            ctx,
            db_path,
            settings,
            settings_file,
            toasts: Toasts::default(),
            palette,
            dock: SignalsDock::default(),
            current_view: ActiveView::Board,

            projects: Vec::new(),
            selected_project: None,
            all_issues: Vec::new(),
            milestones: Vec::new(),
            cards: Vec::new(),

            selected_issue_key: None,
            selected_detail: None,
            inspector_open: false,

            edit_title: String::new(),
            edit_description: String::new(),
            edit_assignee: String::new(),
            edit_estimate: String::new(),
            edit_due_date: String::new(),
            edit_milestone: String::new(),
            new_label_input: String::new(),
            new_rel_kind: RelationKind::Blocks,
            new_rel_target: String::new(),
            new_comment: String::new(),
            new_parent: String::new(),

            filter_text: String::new(),
            filter_priority: None,
            filter_assignee: None,
            filter_status: None,
            swimlane_mode: SwimlaneMode::None,

            show_new_issue: false,
            new_issue_title: String::new(),
            new_issue_desc: String::new(),
            new_issue_status: Status::Todo,
            new_issue_priority: Priority::Medium,
            new_issue_assignee: String::new(),
            new_issue_estimate: String::new(),
            new_issue_due_date: String::new(),
            new_issue_milestone: String::new(),
            new_issue_labels: String::new(),

            show_new_project: false,
            new_project_key: String::new(),
            new_project_name: String::new(),
            new_project_repo: String::new(),

            show_new_milestone: false,
            new_milestone_title: String::new(),
            new_milestone_due: String::new(),

            show_force_move: false,
            force_move_target: None,
            bar: Bar::new(),
        };

        app.reload_projects();
        app
    }

    fn reload_projects(&mut self) {
        if let Ok(projs) = self.ctx.tracker.projects() {
            self.projects = projs;
            if self.selected_project.is_none() && !self.projects.is_empty() {
                self.selected_project = Some(self.projects[0].key.clone());
            }
        }
        self.reload_active_data();
    }

    fn reload_active_data(&mut self) {
        let Some(ref p_key) = self.selected_project else {
            self.all_issues.clear();
            self.cards.clear();
            self.milestones.clear();
            return;
        };

        if let Ok(ms) = self.ctx.tracker.milestones(Some(p_key)) {
            self.milestones = ms;
        }

        if let Ok(issues) = self.ctx.tracker.list(&IssueFilter {
            project: Some(p_key.clone()),
            include_finished: true,
            limit: 2000,
            ..Default::default()
        }) {
            self.all_issues = issues;
        }

        self.rebuild_cards();

        if let Some(ref key) = self.selected_issue_key {
            let k = key.clone();
            self.load_issue_detail(&k);
        }
    }

    fn rebuild_cards(&mut self) {
        let mut cards = Vec::new();

        for i in &self.all_issues {
            // Apply filtering
            if !self.filter_text.is_empty() {
                let q = self.filter_text.to_lowercase();
                let matches_key = i.key.to_lowercase().contains(&q);
                let matches_title = i.title.to_lowercase().contains(&q);
                let matches_desc = i.description.to_lowercase().contains(&q);
                let matches_labels = i.labels.iter().any(|l| l.to_lowercase().contains(&q));
                if !matches_key && !matches_title && !matches_desc && !matches_labels {
                    continue;
                }
            }
            if let Some(p) = self.filter_priority {
                if i.priority != p {
                    continue;
                }
            }
            if let Some(ref a) = self.filter_assignee {
                if a == "Unassigned" {
                    if i.assignee.is_some() {
                        continue;
                    }
                } else if i.assignee.as_deref() != Some(a.as_str()) {
                    continue;
                }
            }
            if let Some(s) = self.filter_status {
                if i.status != s {
                    continue;
                }
            }

            let col = i.status.as_str().to_string();
            let accent = priority_tone(i.priority);
            let badge = Some((i.priority.as_str().to_uppercase(), accent));

            let lane = match self.swimlane_mode {
                SwimlaneMode::None => String::new(),
                SwimlaneMode::Priority => i.priority.as_str().to_string(),
                SwimlaneMode::Assignee => i
                    .assignee
                    .clone()
                    .unwrap_or_else(|| "Unassigned".to_string()),
            };

            let mut meta = Vec::new();
            if let Some(est) = i.estimate {
                meta.push(format!("{est} pts"));
            }
            if let Some(due) = i.due_date {
                meta.push(format!("Due {due}"));
            }
            if let Some(ref m) = i.milestone {
                meta.push(format!("🎯 {m}"));
            }

            let blockers = self.ctx.tracker.open_blockers(i.id).unwrap_or_default();
            let blocked = if !blockers.is_empty() {
                Some(format!("Blocked by {} issue(s)", blockers.len()))
            } else {
                None
            };

            let tags = i.labels.iter().map(|l| (l.clone(), Tone::Info)).collect();

            cards.push(Card {
                id: i.key.clone(),
                column: col,
                lane,
                key: i.key.clone(),
                title: i.title.clone(),
                tags,
                accent,
                badge,
                assignee: i.assignee.clone(),
                meta,
                blocked,
                done: i.status.is_finished(),
            });
        }

        self.cards = cards;
    }

    fn select_issue(&mut self, key: &str) {
        self.selected_issue_key = Some(key.to_string());
        self.load_issue_detail(key);
        self.inspector_open = true;
    }

    fn load_issue_detail(&mut self, key: &str) {
        if let Ok(detail) = self.ctx.tracker.detail(key) {
            self.edit_title = detail.issue.title.clone();
            self.edit_description = detail.issue.description.clone();
            self.edit_assignee = detail.issue.assignee.clone().unwrap_or_default();
            self.edit_estimate = detail
                .issue
                .estimate
                .map(|e| e.to_string())
                .unwrap_or_default();
            self.edit_due_date = detail
                .issue
                .due_date
                .map(|d| d.to_string())
                .unwrap_or_default();
            self.edit_milestone = detail.issue.milestone.clone().unwrap_or_default();
            self.selected_detail = Some(detail);
        } else {
            self.selected_detail = None;
        }
    }

    fn open_new_issue_modal(&mut self, status: Option<Status>, priority: Option<Priority>) {
        self.new_issue_title.clear();
        self.new_issue_desc.clear();
        self.new_issue_status = status.unwrap_or(Status::Todo);
        self.new_issue_priority = priority.unwrap_or(Priority::Medium);
        self.new_issue_assignee.clear();
        self.new_issue_estimate.clear();
        self.new_issue_due_date.clear();
        self.new_issue_milestone.clear();
        self.new_issue_labels.clear();
        self.show_new_issue = true;
    }

    fn execute_create_issue(&mut self) {
        let Some(ref p_key) = self.selected_project else {
            self.toasts.err("Select a project first");
            return;
        };

        let title = self.new_issue_title.trim();
        if title.is_empty() {
            self.toasts.err("Title cannot be empty");
            return;
        }

        let estimate = self.new_issue_estimate.trim().parse::<i64>().ok();
        let due_date = if !self.new_issue_due_date.trim().is_empty() {
            NaiveDate::parse_from_str(self.new_issue_due_date.trim(), "%Y-%m-%d").ok()
        } else {
            None
        };

        let assignee = if !self.new_issue_assignee.trim().is_empty() {
            Some(self.new_issue_assignee.trim().to_string())
        } else {
            None
        };

        let milestone = if !self.new_issue_milestone.trim().is_empty() {
            Some(self.new_issue_milestone.trim().to_string())
        } else {
            None
        };

        let labels = self
            .new_issue_labels
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let new_iss = NewIssue {
            project: p_key.clone(),
            title: title.to_string(),
            description: self.new_issue_desc.trim().to_string(),
            status: Some(self.new_issue_status),
            priority: Some(self.new_issue_priority),
            estimate,
            assignee,
            milestone,
            due_date,
            labels,
            ..NewIssue::default()
        };

        match dispatch(&mut self.ctx, Command::IssueCreate(new_iss)) {
            Ok(res) => {
                let key = res
                    .get("key")
                    .and_then(|k| k.as_str())
                    .unwrap_or("issue")
                    .to_string();
                self.toasts.ok(format!("Created issue {key}"));
                self.show_new_issue = false;
                self.reload_active_data();
                self.select_issue(&key);
            }
            Err(e) => {
                self.toasts.err(format!("Create issue failed: {e}"));
            }
        }
    }

    fn execute_create_project(&mut self) {
        let key = self.new_project_key.trim().to_uppercase();
        let name = self.new_project_name.trim();
        if key.is_empty() || name.is_empty() {
            self.toasts.err("Project key and name are required");
            return;
        }

        let repo_path = if !self.new_project_repo.trim().is_empty() {
            Some(self.new_project_repo.trim().to_string())
        } else {
            None
        };

        match dispatch(
            &mut self.ctx,
            Command::ProjectCreate {
                key: key.clone(),
                name: name.to_string(),
                repo_path,
            },
        ) {
            Ok(_) => {
                self.toasts.ok(format!("Project {key} created"));
                self.show_new_project = false;
                self.new_project_key.clear();
                self.new_project_name.clear();
                self.new_project_repo.clear();
                self.selected_project = Some(key);
                self.reload_projects();
            }
            Err(e) => {
                self.toasts.err(format!("Create project failed: {e}"));
            }
        }
    }

    fn execute_create_milestone(&mut self) {
        let Some(ref p_key) = self.selected_project else {
            self.toasts.err("Select a project first");
            return;
        };

        let title = self.new_milestone_title.trim();
        if title.is_empty() {
            self.toasts.err("Milestone title is required");
            return;
        }

        let due = if !self.new_milestone_due.trim().is_empty() {
            NaiveDate::parse_from_str(self.new_milestone_due.trim(), "%Y-%m-%d").ok()
        } else {
            None
        };

        match dispatch(
            &mut self.ctx,
            Command::MilestoneCreate {
                project: p_key.clone(),
                title: title.to_string(),
                due,
            },
        ) {
            Ok(_) => {
                self.toasts.ok(format!("Milestone '{title}' created"));
                self.show_new_milestone = false;
                self.new_milestone_title.clear();
                self.new_milestone_due.clear();
                self.reload_active_data();
            }
            Err(e) => {
                self.toasts.err(format!("Create milestone failed: {e}"));
            }
        }
    }

    fn execute_move_issue(&mut self, issue_key: &str, status: Status, force: bool) {
        match dispatch(
            &mut self.ctx,
            Command::IssueMove {
                issue: issue_key.to_string(),
                status,
                force,
            },
        ) {
            Ok(_) => {
                let verb = if force { "Force moved" } else { "Moved" };
                self.toasts
                    .ok(format!("{verb} {issue_key} -> {}", status.as_str()));
                self.reload_active_data();
            }
            Err(e) => {
                self.toasts.err(format!("Move failed: {e}"));
                if e.contains("blocked") {
                    self.force_move_target = Some(status);
                    self.show_force_move = true;
                }
            }
        }
    }

    fn execute_update_issue_patch(&mut self, issue_key: &str, patch: IssuePatch) {
        match dispatch(
            &mut self.ctx,
            Command::IssueUpdate {
                issue: issue_key.to_string(),
                patch,
            },
        ) {
            Ok(_) => {
                self.toasts.ok(format!("Updated {issue_key}"));
                self.reload_active_data();
            }
            Err(e) => {
                self.toasts.err(format!("Update failed: {e}"));
            }
        }
    }

    fn execute_add_relation(&mut self, source: &str, kind: RelationKind, target: &str) {
        let tgt = target.trim();
        if tgt.is_empty() {
            self.toasts.err("Target issue is required");
            return;
        }

        match dispatch(
            &mut self.ctx,
            Command::RelationAdd {
                source: source.to_string(),
                kind,
                target: tgt.to_string(),
            },
        ) {
            Ok(_) => {
                self.toasts.ok(format!(
                    "Linked {source} {} {tgt}",
                    kind.as_str().replace('_', " ")
                ));
                self.new_rel_target.clear();
                self.reload_active_data();
            }
            Err(e) => {
                self.toasts.err(format!("Relation failed: {e}"));
            }
        }
    }

    /// Put `issue` under `parent`, or detach it with `None`.
    fn execute_reparent(&mut self, issue: &str, parent: Option<String>) {
        let label = parent.clone();
        match dispatch(
            &mut self.ctx,
            Command::IssueReparent {
                issue: issue.to_string(),
                parent,
            },
        ) {
            Ok(_) => {
                self.new_parent.clear();
                self.toasts.ok(match label {
                    Some(p) => format!("{issue} is now under {p}"),
                    None => format!("{issue} detached from its parent"),
                });
                self.reload_active_data();
                self.load_issue_detail(issue);
            }
            Err(e) => self.toasts.err(format!("Reparent failed: {e}")),
        }
    }

    /// Post the comment box on `issue` as the acting user.
    fn execute_add_comment(&mut self, issue: &str) {
        let body = self.new_comment.trim().to_string();
        if body.is_empty() {
            return;
        }
        match dispatch(
            &mut self.ctx,
            Command::CommentAdd {
                issue: issue.to_string(),
                body,
            },
        ) {
            Ok(r) => {
                self.new_comment.clear();
                let unknown: Vec<String> = r["unknown_mentions"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str())
                            .map(|h| format!("@{h}"))
                            .collect()
                    })
                    .unwrap_or_default();
                if unknown.is_empty() {
                    self.toasts.ok(format!("Commented on {issue}"));
                } else {
                    self.toasts.push(
                        format!(
                            "Commented on {issue}; nobody is called {}",
                            unknown.join(", ")
                        ),
                        Tone::Warn,
                    );
                }
                self.load_issue_detail(issue);
            }
            Err(e) => self.toasts.err(format!("Comment failed: {e}")),
        }
    }

    fn execute_delete_comment(&mut self, issue: &str, comment: i64, version: i64) {
        match dispatch(
            &mut self.ctx,
            Command::CommentDelete {
                comment,
                version: Some(version),
            },
        ) {
            Ok(_) => {
                self.toasts.ok(format!("Deleted comment #{comment}"));
                self.load_issue_detail(issue);
            }
            Err(e) => self.toasts.err(format!("Delete failed: {e}")),
        }
    }

    fn execute_remove_relation(&mut self, source: &str, kind: RelationKind, target: &str) {
        match dispatch(
            &mut self.ctx,
            Command::RelationRemove {
                source: source.to_string(),
                kind,
                target: target.to_string(),
            },
        ) {
            Ok(_) => {
                self.toasts.ok(format!("Removed relation with {target}"));
                self.reload_active_data();
            }
            Err(e) => {
                self.toasts.err(format!("Remove relation failed: {e}"));
            }
        }
    }

    fn execute_git_sync(&mut self) {
        let Some(ref p_key) = self.selected_project else {
            self.toasts.err("Select a project first");
            return;
        };

        match dispatch(
            &mut self.ctx,
            Command::GitSync {
                project: p_key.clone(),
                max: 100,
            },
        ) {
            Ok(res) => {
                let commits = res
                    .get("commits_scanned")
                    .and_then(|c| c.as_i64())
                    .unwrap_or(0);
                let linked = res.get("linked").and_then(|c| c.as_i64()).unwrap_or(0);
                let closed = res.get("closed").and_then(|c| c.as_i64()).unwrap_or(0);
                self.toasts.ok(format!(
                    "Git sync complete: {commits} scanned, {linked} linked, {closed} closed"
                ));
                self.reload_active_data();
            }
            Err(e) => {
                self.toasts.err(format!("Git sync failed: {e}"));
            }
        }
    }

    fn execute_demo_seed(&mut self) {
        match dispatch(&mut self.ctx, Command::DemoSeed) {
            Ok(_) => {
                self.toasts
                    .ok("Database seeded with demo issues and projects");
                self.reload_projects();
            }
            Err(e) => {
                self.toasts.err(format!("Demo seed failed: {e}"));
            }
        }
    }

    // =========================================================================
    // RENDERING SECTIONS
    // =========================================================================

    fn render_top_bar(&mut self, ui: &mut egui::Ui) {
        let t = theme::current(ui.ctx());
        let bar = egui::Frame::new()
            .fill(t.surface)
            .inner_margin(egui::Margin::symmetric(14, 8));

        let mut open_palette = false;
        let mut toggle_dock = false;
        let mut trigger_new_issue = false;
        let mut trigger_new_project = false;
        let mut trigger_refresh = false;
        let mut trigger_demo_seed = false;
        let mut switch_project = None;

        let selected_project_key = self.selected_project.clone();
        let projects = self.projects.clone();
        let is_empty = self.projects.is_empty() || self.all_issues.is_empty();
        let (pending, failed) = self.dock.counts();

        egui::Panel::top("top_panel").frame(bar).show(ui, |ui| {
            shell::top_bar(
                ui,
                APP_NAME,
                Tone::Accent,
                |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Project:").family(semibold()));
                        let current_name = selected_project_key
                            .as_ref()
                            .and_then(|k| projects.iter().find(|p| &p.key == k))
                            .map(|p| format!("{} ({})", p.name, p.key))
                            .unwrap_or_else(|| "Select Project".to_string());

                        egui::ComboBox::from_id_salt("project_selector")
                            .selected_text(current_name)
                            .width(180.0)
                            .show_ui(ui, |ui| {
                                for p in &projects {
                                    let is_sel = selected_project_key.as_deref() == Some(&p.key);
                                    let label = format!("{} ({})", p.name, p.key);
                                    if ui.selectable_label(is_sel, label).clicked() {
                                        switch_project = Some(p.key.clone());
                                    }
                                }
                            });

                        if ui
                            .button("+ New")
                            .on_hover_text("Create a new project")
                            .clicked()
                        {
                            trigger_new_project = true;
                        }

                        ui.separator();

                        if ui
                            .add(
                                egui::Button::new("🔍 Search or command...")
                                    .min_size(egui::vec2(220.0, 28.0)),
                            )
                            .on_hover_text("Open command palette (Ctrl+K)")
                            .clicked()
                        {
                            open_palette = true;
                        }
                        widgets::key_hint(ui, "Ctrl+K");
                    });
                },
                |ui| {
                    if widgets::primary(ui, "+ New Issue").clicked() {
                        trigger_new_issue = true;
                    }

                    if ui
                        .button("⟳ Refresh")
                        .on_hover_text("Reload board & data")
                        .clicked()
                    {
                        trigger_refresh = true;
                    }

                    let sig_label = if pending + failed > 0 {
                        format!("Signals {}", pending + failed)
                    } else {
                        "Signals".to_string()
                    };
                    if ui
                        .button(sig_label)
                        .on_hover_text("Fleet cross-app bus events")
                        .clicked()
                    {
                        toggle_dock = true;
                    }

                    if is_empty
                        && ui
                            .button("🌱 Demo Seed")
                            .on_hover_text("Seed sample projects and cards")
                            .clicked()
                    {
                        trigger_demo_seed = true;
                    }
                },
            );
        });

        if let Some(p) = switch_project {
            self.selected_project = Some(p);
            self.reload_active_data();
        }
        if trigger_new_project {
            self.show_new_project = true;
        }
        if trigger_new_issue {
            self.open_new_issue_modal(None, None);
        }
        if trigger_refresh {
            self.reload_active_data();
            self.toasts.ok("Data refreshed");
        }
        if trigger_demo_seed {
            self.execute_demo_seed();
        }
        if open_palette {
            self.palette.open_list();
        }
        if toggle_dock {
            self.dock.open = !self.dock.open;
        }
    }

    fn render_nav_rail(&mut self, ui: &mut egui::Ui) {
        let t = theme::current(ui.ctx());
        let mut switch_project = None;
        let projects = self.projects.clone();
        let selected_project_key = self.selected_project.clone();
        let open_issues = self
            .all_issues
            .iter()
            .filter(|i| !i.status.is_finished())
            .count();
        let total_issues = self.all_issues.len();
        let milestone_count = self.milestones.len();

        egui::Panel::left("nav_rail")
            .exact_size(210.0)
            .frame(
                egui::Frame::new()
                    .fill(t.surface)
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(ui, |ui| {
                let proj_title = selected_project_key
                    .as_ref()
                    .map(|k| format!("Project {k}"))
                    .unwrap_or_else(|| "Workspace".to_string());
                shell::nav_caption(ui, &proj_title);

                if shell::nav_item(
                    ui,
                    "▦",
                    "Kanban Board",
                    self.current_view == ActiveView::Board,
                    Some((open_issues, Tone::Accent)),
                )
                .clicked()
                {
                    self.current_view = ActiveView::Board;
                }

                if shell::nav_item(
                    ui,
                    "☰",
                    "Issues Table",
                    self.current_view == ActiveView::Table,
                    Some((total_issues, Tone::Neutral)),
                )
                .clicked()
                {
                    self.current_view = ActiveView::Table;
                }

                if shell::nav_item(
                    ui,
                    "⚑",
                    "Milestones",
                    self.current_view == ActiveView::Milestones,
                    Some((milestone_count, Tone::Info)),
                )
                .clicked()
                {
                    self.current_view = ActiveView::Milestones;
                }

                if shell::nav_item(
                    ui,
                    "⎇",
                    "Git Sync",
                    self.current_view == ActiveView::GitSync,
                    None,
                )
                .clicked()
                {
                    self.current_view = ActiveView::GitSync;
                }

                if shell::nav_item(
                    ui,
                    "⏱",
                    "Activity Log",
                    self.current_view == ActiveView::History,
                    None,
                )
                .clicked()
                {
                    self.current_view = ActiveView::History;
                }

                ui.separator();
                shell::nav_caption(ui, "Quick Switch");

                ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    for p in &projects {
                        let is_active = selected_project_key.as_deref() == Some(&p.key);
                        if ui
                            .selectable_label(is_active, format!("{}: {}", p.key, p.name))
                            .clicked()
                        {
                            switch_project = Some(p.key.clone());
                        }
                    }
                });

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new(format!("Actor: {}", self.ctx.actor))
                            .color(t.text_weak)
                            .size(11.5),
                    );
                    ui.label(
                        RichText::new(format!("DB: {}", self.db_path.display()))
                            .color(t.text_weak)
                            .size(10.5),
                    );
                });
            });

        if let Some(p) = switch_project {
            self.selected_project = Some(p);
            self.reload_active_data();
        }
    }

    fn render_status_bar(&mut self, ui: &mut egui::Ui) {
        let t = theme::current(ui.ctx());
        let ctx = ui.ctx().clone();
        let mut click = shell::StatusClick::default();

        egui::Panel::bottom("status_bar")
            .frame(
                egui::Frame::new()
                    .fill(t.surface)
                    .inner_margin(egui::Margin::symmetric(14, 5)),
            )
            .show(ui, |ui| {
                let info = format!(
                    "Project: {} | {} issues ({} cards) | DB: OK",
                    self.selected_project.as_deref().unwrap_or("-"),
                    self.all_issues.len(),
                    self.cards.len()
                );
                click = shell::status_bar(
                    ui,
                    self.toasts.latest(),
                    ctx.zoom_factor(),
                    self.settings.dark,
                    &info,
                );
            });

        if click.toggle_theme {
            let dark = !self.settings.dark;
            shell::set_dark(&ctx, &mut self.settings, dark);
            let _ = self.settings.save(&self.settings_file);
        }
        if click.reset_zoom {
            ctx.set_zoom_factor(1.0);
            self.settings.zoom = 1.0;
            let _ = self.settings.save(&self.settings_file);
        }
    }

    fn render_board_view(&mut self, ui: &mut egui::Ui) {
        // Filter toolbar
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.filter_text)
                    .hint_text("🔍 Filter board...")
                    .desired_width(180.0),
            );

            // Priority filter dropdown
            let prio_text = self
                .filter_priority
                .map(|p| p.as_str().to_uppercase())
                .unwrap_or_else(|| "All Priorities".to_string());
            egui::ComboBox::from_id_salt("prio_filter")
                .selected_text(prio_text)
                .width(130.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.filter_priority.is_none(), "All Priorities")
                        .clicked()
                    {
                        self.filter_priority = None;
                    }
                    for p in Priority::ALL {
                        if ui
                            .selectable_label(
                                self.filter_priority == Some(*p),
                                p.as_str().to_uppercase(),
                            )
                            .clicked()
                        {
                            self.filter_priority = Some(*p);
                        }
                    }
                });

            // Assignee filter dropdown
            let mut assignees: HashSet<String> = HashSet::new();
            for i in &self.all_issues {
                if let Some(ref a) = i.assignee {
                    assignees.insert(a.clone());
                }
            }
            let assignee_text = self
                .filter_assignee
                .clone()
                .unwrap_or_else(|| "All Assignees".to_string());
            egui::ComboBox::from_id_salt("assignee_filter")
                .selected_text(assignee_text)
                .width(140.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.filter_assignee.is_none(), "All Assignees")
                        .clicked()
                    {
                        self.filter_assignee = None;
                    }
                    if ui
                        .selectable_label(
                            self.filter_assignee.as_deref() == Some("Unassigned"),
                            "Unassigned",
                        )
                        .clicked()
                    {
                        self.filter_assignee = Some("Unassigned".to_string());
                    }
                    for a in &assignees {
                        if ui
                            .selectable_label(self.filter_assignee.as_deref() == Some(a), a)
                            .clicked()
                        {
                            self.filter_assignee = Some(a.clone());
                        }
                    }
                });

            // Swimlane mode selector
            let lane_text = match self.swimlane_mode {
                SwimlaneMode::None => "Lanes: None",
                SwimlaneMode::Priority => "Lanes: Priority",
                SwimlaneMode::Assignee => "Lanes: Assignee",
            };
            egui::ComboBox::from_id_salt("lane_mode")
                .selected_text(lane_text)
                .width(130.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.swimlane_mode == SwimlaneMode::None, "Lanes: None")
                        .clicked()
                    {
                        self.swimlane_mode = SwimlaneMode::None;
                    }
                    if ui
                        .selectable_label(
                            self.swimlane_mode == SwimlaneMode::Priority,
                            "Lanes: Priority",
                        )
                        .clicked()
                    {
                        self.swimlane_mode = SwimlaneMode::Priority;
                    }
                    if ui
                        .selectable_label(
                            self.swimlane_mode == SwimlaneMode::Assignee,
                            "Lanes: Assignee",
                        )
                        .clicked()
                    {
                        self.swimlane_mode = SwimlaneMode::Assignee;
                    }
                });

            let has_filter = !self.filter_text.is_empty()
                || self.filter_priority.is_some()
                || self.filter_assignee.is_some()
                || self.filter_status.is_some();

            if has_filter && ui.button("Clear Filters").clicked() {
                self.filter_text.clear();
                self.filter_priority = None;
                self.filter_assignee = None;
                self.filter_status = None;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("Showing {} cards", self.cards.len()))
                        .color(theme::current(ui.ctx()).text_weak),
                );
            });
        });

        self.rebuild_cards();
        ui.add_space(6.0);

        let columns = [
            Column::new("backlog", "Backlog", Tone::Neutral),
            Column::new("todo", "To Do", Tone::Info),
            Column::new("in_progress", "In Progress", Tone::Accent).wip(5),
            Column::new("in_review", "In Review", Tone::Warn),
            Column::new("done", "Done", Tone::Ok),
            Column::new("canceled", "Canceled", Tone::Neutral),
        ];

        let priority_lanes = [
            Lane {
                id: "urgent".into(),
                title: "Urgent Priority".into(),
            },
            Lane {
                id: "high".into(),
                title: "High Priority".into(),
            },
            Lane {
                id: "medium".into(),
                title: "Medium Priority".into(),
            },
            Lane {
                id: "low".into(),
                title: "Low Priority".into(),
            },
        ];

        let mut dynamic_assignee_lanes: Vec<Lane> = Vec::new();
        if self.swimlane_mode == SwimlaneMode::Assignee {
            let mut set = HashSet::new();
            for i in &self.all_issues {
                if let Some(ref a) = i.assignee {
                    set.insert(a.clone());
                }
            }
            dynamic_assignee_lanes.push(Lane {
                id: "Unassigned".into(),
                title: "Unassigned".into(),
            });
            for a in set {
                dynamic_assignee_lanes.push(Lane {
                    id: a.clone(),
                    title: a,
                });
            }
        }

        let lanes_slice: &[Lane] = match self.swimlane_mode {
            SwimlaneMode::None => &[],
            SwimlaneMode::Priority => &priority_lanes,
            SwimlaneMode::Assignee => &dynamic_assignee_lanes,
        };

        let rule: &dyn Fn(&Card, &str) -> Result<(), String> = &|card, target_col| {
            if card.blocked.is_some()
                && (target_col == "in_progress"
                    || target_col == "in_review"
                    || target_col == "done")
            {
                return Err("Issue is blocked by an open dependency. Use Force Move in Inspector if permitted.".to_string());
            }
            Ok(())
        };

        let mut board = Board::new("taskcraft_board", &columns, &self.cards)
            .rule(rule)
            .selected(self.selected_issue_key.as_deref())
            .add_buttons(true)
            .column_width(260.0);

        if !lanes_slice.is_empty() {
            board = board.lanes(lanes_slice);
        }

        let evs = board.show(ui);

        for ev in evs {
            match ev {
                Event::Moved {
                    card, to, to_lane, ..
                } => {
                    if let Ok(st) = Status::parse(&to) {
                        self.execute_move_issue(&card, st, false);

                        // If moved across priority lanes, update priority too
                        if self.swimlane_mode == SwimlaneMode::Priority {
                            if let Ok(prio) = Priority::parse(&to_lane) {
                                self.execute_update_issue_patch(
                                    &card,
                                    IssuePatch {
                                        priority: Some(prio),
                                        ..Default::default()
                                    },
                                );
                            }
                        }
                    }
                }
                Event::Refused { card, to, reason } => {
                    self.toasts
                        .err(format!("Move refused for {card} -> {to}: {reason}"));
                }
                Event::Select(id) => {
                    self.select_issue(&id);
                }
                Event::Open(id) => {
                    self.select_issue(&id);
                    self.inspector_open = true;
                }
                Event::Add { column, lane } => {
                    let st = Status::parse(&column).ok();
                    let pr = Priority::parse(&lane).ok();
                    self.open_new_issue_modal(st, pr);
                    if self.swimlane_mode == SwimlaneMode::Assignee && lane != "Unassigned" {
                        self.new_issue_assignee = lane;
                    }
                }
                Event::Undo => {}
            }
        }
    }

    fn render_table_view(&mut self, ui: &mut egui::Ui) {
        let cols = [
            Col::new("Key", 0.12),
            Col::new("Title", 0.35),
            Col::new("Status", 0.13),
            Col::new("Priority", 0.11),
            Col::new("Assignee", 0.13),
            Col::new("Milestone", 0.12),
            Col::num("Pts", 0.08),
            Col::new("Due", 0.11),
            Col::new("Blockers", 0.11),
        ];

        let mut rows = Vec::new();
        for i in &self.all_issues {
            let blockers = self.ctx.tracker.open_blockers(i.id).unwrap_or_default();
            let block_cell = if !blockers.is_empty() {
                Cell::Pill(format!("Blocked ({})", blockers.len()), Tone::Danger)
            } else {
                Cell::Weak("-".into())
            };

            let cells = vec![
                Cell::Mono(i.key.clone()),
                Cell::Strong(i.title.clone()),
                Cell::Pill(i.status.as_str().to_string(), status_tone(i.status)),
                Cell::Pill(i.priority.as_str().to_string(), priority_tone(i.priority)),
                Cell::Person(i.assignee.clone().unwrap_or_else(|| "-".into())),
                Cell::Weak(i.milestone.clone().unwrap_or_else(|| "-".into())),
                Cell::Mono(
                    i.estimate
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "-".into()),
                ),
                Cell::Weak(
                    i.due_date
                        .map(|d| d.to_string())
                        .unwrap_or_else(|| "-".into()),
                ),
                block_cell,
            ];
            rows.push((i.key.clone(), cells));
        }

        ui.horizontal(|ui| {
            if widgets::primary(ui, "+ New Issue").clicked() {
                self.open_new_issue_modal(None, None);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("Total: {} issues", self.all_issues.len()))
                        .color(theme::current(ui.ctx()).text_weak),
                );
            });
        });
        ui.add_space(4.0);

        let events = Table::new("issues_table", &cols, &rows)
            .selected(self.selected_issue_key.as_deref())
            .filter_box(true)
            .show(ui);

        for ev in events {
            match ev {
                TableEvent::Select(id) => {
                    self.select_issue(&id);
                }
                TableEvent::Open(id) => {
                    self.select_issue(&id);
                    self.inspector_open = true;
                }
            }
        }
    }

    fn render_milestones_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("🎯 Milestones & Burn-up");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::primary(ui, "+ New Milestone").clicked() {
                    self.show_new_milestone = true;
                }
            });
        });
        ui.separator();

        if self.milestones.is_empty() {
            widgets::empty_state(
                ui,
                "No Milestones Defined",
                "Create a milestone to group issues, track story point progress, and see burn-up velocity.",
            );
            return;
        }

        let mut select_key = None;

        ScrollArea::vertical().show(ui, |ui| {
            for m in self.milestones.clone() {
                widgets::card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&m.title).family(semibold()).size(18.0));
                        if m.open {
                            widgets::pill(ui, "Open", Tone::Ok);
                        } else {
                            widgets::pill(ui, "Closed", Tone::Neutral);
                        }
                        if let Some(due) = m.due_date {
                            ui.label(
                                RichText::new(format!("Due: {due}"))
                                    .color(theme::current(ui.ctx()).text_weak),
                            );
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if m.open && ui.button("Close Milestone").clicked() {
                                if let Some(ref p) = self.selected_project {
                                    let _ = self.ctx.tracker.close_milestone(p, &m.title);
                                    self.toasts.ok(format!("Closed milestone '{}'", m.title));
                                    self.reload_active_data();
                                }
                            }
                        });
                    });

                    ui.add_space(8.0);

                    // Progress indicators
                    let issue_ratio = if m.issues_total > 0 {
                        (m.issues_done as f32 / m.issues_total as f32).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let points_ratio = if m.points_total > 0 {
                        (m.points_done as f32 / m.points_total as f32).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };

                    ui.columns(2, |cols| {
                        cols[0].vertical(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "Issues: {} / {} ({}%)",
                                    m.issues_done,
                                    m.issues_total,
                                    (issue_ratio * 100.0) as i64
                                ))
                                .family(semibold()),
                            );
                            ui.add(ProgressBar::new(issue_ratio).animate(false));
                        });

                        cols[1].vertical(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "Story Points: {} / {} ({}%)",
                                    m.points_done,
                                    m.points_total,
                                    (points_ratio * 100.0) as i64
                                ))
                                .family(semibold()),
                            );
                            ui.add(ProgressBar::new(points_ratio).animate(false));
                        });
                    });

                    ui.add_space(8.0);
                    ui.label(RichText::new("Assigned Issues:").family(semibold()));

                    let m_issues: Vec<(String, Status, String)> = self
                        .all_issues
                        .iter()
                        .filter(|i| i.milestone.as_deref() == Some(&m.title))
                        .map(|i| (i.key.clone(), i.status, i.title.clone()))
                        .collect();

                    if m_issues.is_empty() {
                        ui.label(
                            RichText::new("No issues assigned to this milestone yet.")
                                .italics()
                                .color(theme::current(ui.ctx()).text_weak),
                        );
                    } else {
                        for (k, st, tit) in m_issues {
                            ui.horizontal(|ui| {
                                if ui.link(&k).clicked() {
                                    select_key = Some(k.clone());
                                }
                                widgets::pill(ui, st.as_str(), status_tone(st));
                                ui.label(&tit);
                            });
                        }
                    }
                });
                ui.add_space(10.0);
            }
        });

        if let Some(k) = select_key {
            self.select_issue(&k);
        }
    }

    fn render_git_sync_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("⎇ Git Sync & Commit History");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::primary(ui, "⟳ Scan Git Commits").clicked() {
                    self.execute_git_sync();
                }
            });
        });
        ui.separator();

        let cur_proj = self
            .selected_project
            .as_ref()
            .and_then(|k| self.projects.iter().find(|p| &p.key == k));

        let repo_path = cur_proj
            .and_then(|p| p.repo_path.as_deref())
            .unwrap_or("No repository path configured for this project");
        let last_commit = cur_proj
            .and_then(|p| p.last_synced_commit.as_deref())
            .unwrap_or("Never synced");

        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Configured Repository:").family(semibold()));
                ui.label(repo_path);
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Last Synced Commit:").family(semibold()));
                ui.label(RichText::new(last_commit).family(egui::FontFamily::Monospace));
            });
            ui.add_space(4.0);
            ui.label(RichText::new("Git sync reads the repository commit log (read-only) and finds issue mentions like 'fixes OPS-12' or 'closes KEY-5'. It links commits idempotently and advances issue status.").italics().color(theme::current(ui.ctx()).text_weak));
        });

        ui.add_space(12.0);
        ui.heading("Recently Linked Commits");
        ui.separator();

        // Collect all commit links across issues in this project
        let mut all_commits = Vec::new();
        for i in &self.all_issues {
            if let Ok(detail) = self.ctx.tracker.detail(&i.key) {
                for c in detail.commits {
                    all_commits.push((i.key.clone(), c));
                }
            }
        }

        if all_commits.is_empty() {
            widgets::empty_state(
                ui,
                "No Git Commits Linked",
                "Click 'Scan Git Commits' above to scan commit messages in the project repository for issue references.",
            );
        } else {
            let mut select_key = None;
            ScrollArea::vertical().show(ui, |ui| {
                for (key, commit) in all_commits {
                    ui.horizontal(|ui| {
                        let short_hash = if commit.hash.len() > 8 {
                            &commit.hash[..8]
                        } else {
                            &commit.hash
                        };
                        ui.label(
                            RichText::new(short_hash)
                                .family(egui::FontFamily::Monospace)
                                .strong(),
                        );
                        if ui.link(&key).clicked() {
                            select_key = Some(key.clone());
                        }
                        if commit.closing {
                            widgets::pill(ui, "Closes", Tone::Ok);
                        } else {
                            widgets::pill(ui, "Mentions", Tone::Neutral);
                        }
                        ui.label(format!("by {}", commit.author));
                        ui.label(
                            RichText::new(&commit.subject).color(theme::current(ui.ctx()).text),
                        );
                    });
                    ui.separator();
                }
            });
            if let Some(k) = select_key {
                self.select_issue(&k);
            }
        }
    }

    fn render_history_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("⏱ Project Audit Log & Event Stream");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⟳ Refresh Log").clicked() {
                    self.reload_active_data();
                }
            });
        });
        ui.separator();

        // Collect events from all issues
        let mut events = Vec::new();
        for i in &self.all_issues {
            if let Ok(detail) = self.ctx.tracker.detail(&i.key) {
                for ev in detail.history {
                    events.push((i.key.clone(), ev));
                }
            }
        }
        events.sort_by(|a, b| b.1.at.cmp(&a.1.at));

        if events.is_empty() {
            widgets::empty_state(
                ui,
                "No Events Recorded",
                "Every mutation (status move, field update, relation change) records an immutable event here.",
            );
        } else {
            let mut select_key = None;
            ScrollArea::vertical().show(ui, |ui| {
                for (key, ev) in events {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&ev.at)
                                .color(theme::current(ui.ctx()).text_weak)
                                .size(12.0),
                        );
                        widgets::pill(ui, &ev.actor, Tone::Info);
                        if ui.link(&key).clicked() {
                            select_key = Some(key.clone());
                        }
                        ui.label(RichText::new(&ev.what).family(semibold()));
                    });
                    ui.separator();
                }
            });
            if let Some(k) = select_key {
                self.select_issue(&k);
            }
        }
    }

    // =========================================================================
    // INTERACTIVE ISSUE INSPECTOR (CLICKABLE DRAWER)
    // =========================================================================

    fn render_inspector(&mut self, ui: &mut egui::Ui) {
        let Some(detail) = self.selected_detail.clone() else {
            ui.label("No issue selected");
            return;
        };

        let issue = detail.issue.clone();
        let t = theme::current(ui.ctx());
        let mut select_key = None;
        let mut remove_relation = None;
        let mut remove_label = None;
        let mut post_comment = false;
        let mut delete_comment: Option<(i64, i64)> = None;
        let mut set_kind: Option<IssueKind> = None;
        let mut reparent: Option<Option<String>> = None;

        ui.horizontal(|ui| {
            ui.heading(
                RichText::new(format!("📌 {}", issue.key))
                    .family(egui::FontFamily::Monospace)
                    .strong(),
            );
            widgets::pill(ui, issue.kind.as_str(), kind_tone(issue.kind));
            widgets::pill(ui, issue.status.as_str(), status_tone(issue.status));
            widgets::pill(ui, issue.priority.as_str(), priority_tone(issue.priority));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("✕ Close").clicked() {
                    self.inspector_open = false;
                }
            });
        });
        ui.separator();

        ScrollArea::vertical().show(ui, |ui| {
            // Quick Status Actions
            ui.horizontal(|ui| {
                if !issue.status.is_finished() {
                    if ui.button("✓ Mark as Done").clicked() {
                        self.execute_move_issue(&issue.key, Status::Done, false);
                    }
                } else {
                    if ui.button("↺ Reopen (To Do)").clicked() {
                        self.execute_move_issue(&issue.key, Status::Todo, false);
                    }
                }

                if ui
                    .button("⚡ Force Move...")
                    .on_hover_text("Bypass dependency locks if blocked")
                    .clicked()
                {
                    self.force_move_target = Some(Status::InProgress);
                    self.show_force_move = true;
                }
            });
            ui.add_space(6.0);

            // Title editor
            ui.label(RichText::new("Title:").family(semibold()));
            let title_changed = ui
                .add(
                    egui::TextEdit::singleline(&mut self.edit_title)
                        .desired_width(ui.available_width()),
                )
                .lost_focus();
            if (title_changed || ui.button("Save Title").clicked())
                && self.edit_title != issue.title
            {
                self.execute_update_issue_patch(
                    &issue.key,
                    IssuePatch {
                        title: Some(self.edit_title.clone()),
                        ..Default::default()
                    },
                );
            }

            ui.add_space(8.0);

            // Status changer buttons
            ui.label(RichText::new("Change Status:").family(semibold()));
            ui.horizontal_wrapped(|ui| {
                for s in Status::ALL {
                    let is_active = issue.status == *s;
                    let btn = egui::Button::new(s.as_str().replace('_', " ").to_uppercase());
                    let btn = if is_active {
                        btn.fill(theme::tone_solid(status_tone(*s), t.dark))
                    } else {
                        btn
                    };
                    if ui.add(btn).clicked() && !is_active {
                        self.execute_move_issue(&issue.key, *s, false);
                    }
                }
            });

            ui.add_space(8.0);

            // Priority changer buttons
            ui.label(RichText::new("Priority:").family(semibold()));
            ui.horizontal(|ui| {
                for p in Priority::ALL {
                    let is_active = issue.priority == *p;
                    let btn = egui::Button::new(p.as_str().to_uppercase());
                    let btn = if is_active {
                        btn.fill(theme::tone_solid(priority_tone(*p), t.dark))
                    } else {
                        btn
                    };
                    if ui.add(btn).clicked() && !is_active {
                        self.execute_update_issue_patch(
                            &issue.key,
                            IssuePatch {
                                priority: Some(*p),
                                ..Default::default()
                            },
                        );
                    }
                }
            });

            ui.add_space(8.0);

            // Properties Grid
            egui::Grid::new("issue_props_grid")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Assignee:");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.edit_assignee)
                                .desired_width(140.0),
                        );
                        if ui.button("Set").clicked() {
                            let a = if self.edit_assignee.trim().is_empty() {
                                Some(None)
                            } else {
                                Some(Some(self.edit_assignee.trim().to_string()))
                            };
                            self.execute_update_issue_patch(
                                &issue.key,
                                IssuePatch {
                                    assignee: a,
                                    ..Default::default()
                                },
                            );
                        }
                        if ui.button("Clear").clicked() {
                            self.edit_assignee.clear();
                            self.execute_update_issue_patch(
                                &issue.key,
                                IssuePatch {
                                    assignee: Some(None),
                                    ..Default::default()
                                },
                            );
                        }
                    });
                    ui.end_row();

                    ui.label("Story Points:");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.edit_estimate).desired_width(70.0),
                        );
                        if ui.button("Set").clicked() {
                            let est = self.edit_estimate.trim().parse::<i64>().ok();
                            self.execute_update_issue_patch(
                                &issue.key,
                                IssuePatch {
                                    estimate: Some(est),
                                    ..Default::default()
                                },
                            );
                        }
                    });
                    ui.end_row();

                    ui.label("Due Date:");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.edit_due_date)
                                .hint_text("YYYY-MM-DD")
                                .desired_width(110.0),
                        );
                        if ui.button("Set").clicked() {
                            let d =
                                NaiveDate::parse_from_str(self.edit_due_date.trim(), "%Y-%m-%d")
                                    .ok();
                            self.execute_update_issue_patch(
                                &issue.key,
                                IssuePatch {
                                    due_date: Some(d),
                                    ..Default::default()
                                },
                            );
                        }
                        if ui.button("Today").clicked() {
                            let d = Local::now().date_naive();
                            self.edit_due_date = d.to_string();
                            self.execute_update_issue_patch(
                                &issue.key,
                                IssuePatch {
                                    due_date: Some(Some(d)),
                                    ..Default::default()
                                },
                            );
                        }
                        if ui.button("Clear").clicked() {
                            self.edit_due_date.clear();
                            self.execute_update_issue_patch(
                                &issue.key,
                                IssuePatch {
                                    due_date: Some(None),
                                    ..Default::default()
                                },
                            );
                        }
                    });
                    ui.end_row();

                    ui.label("Milestone:");
                    ui.horizontal(|ui| {
                        let mut ms_list = vec!["None".to_string()];
                        for m in &self.milestones {
                            ms_list.push(m.title.clone());
                        }
                        let sel = if self.edit_milestone.is_empty() {
                            "None".to_string()
                        } else {
                            self.edit_milestone.clone()
                        };

                        egui::ComboBox::from_id_salt("inspector_ms")
                            .selected_text(sel)
                            .width(150.0)
                            .show_ui(ui, |ui| {
                                for m in ms_list {
                                    let is_sel = if m == "None" {
                                        self.edit_milestone.is_empty()
                                    } else {
                                        self.edit_milestone == m
                                    };
                                    if ui.selectable_label(is_sel, &m).clicked() {
                                        if m == "None" {
                                            self.edit_milestone.clear();
                                            self.execute_update_issue_patch(
                                                &issue.key,
                                                IssuePatch {
                                                    milestone: Some(None),
                                                    ..Default::default()
                                                },
                                            );
                                        } else {
                                            self.edit_milestone = m.clone();
                                            self.execute_update_issue_patch(
                                                &issue.key,
                                                IssuePatch {
                                                    milestone: Some(Some(m)),
                                                    ..Default::default()
                                                },
                                            );
                                        }
                                    }
                                }
                            });
                    });
                    ui.end_row();
                });

            ui.add_space(8.0);

            // Labels / Tags management
            ui.label(RichText::new("Labels:").family(semibold()));
            ui.horizontal_wrapped(|ui| {
                for label in &issue.labels {
                    ui.horizontal(|ui| {
                        widgets::pill(ui, label, Tone::Info);
                        if ui.small_button("✕").on_hover_text("Remove label").clicked() {
                            remove_label = Some(label.clone());
                        }
                    });
                }
            });

            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_label_input)
                        .hint_text("New label")
                        .desired_width(120.0),
                );
                if ui.button("+ Add Label").clicked() && !self.new_label_input.trim().is_empty() {
                    let mut new_labels = issue.labels.clone();
                    let trimmed = self.new_label_input.trim().to_string();
                    if !new_labels.contains(&trimmed) {
                        new_labels.push(trimmed);
                        self.execute_update_issue_patch(
                            &issue.key,
                            IssuePatch {
                                labels: Some(new_labels),
                                ..Default::default()
                            },
                        );
                    }
                    self.new_label_input.clear();
                }
            });

            ui.add_space(10.0);

            // Description Editor
            ui.label(RichText::new("Description:").family(semibold()));
            ui.add(
                egui::TextEdit::multiline(&mut self.edit_description)
                    .desired_rows(6)
                    .desired_width(ui.available_width()),
            );
            if ui.button("Save Description").clicked() && self.edit_description != issue.description
            {
                self.execute_update_issue_patch(
                    &issue.key,
                    IssuePatch {
                        description: Some(self.edit_description.clone()),
                        ..Default::default()
                    },
                );
            }

            ui.add_space(12.0);
            ui.separator();

            // Hierarchy (Tier 5): kind, parent, children, rollup.
            ui.heading("Hierarchy");
            ui.horizontal(|ui| {
                ui.label("Kind:");
                egui::ComboBox::from_id_salt("issue_kind")
                    .selected_text(issue.kind.as_str())
                    .show_ui(ui, |ui| {
                        for k in IssueKind::ALL {
                            if ui.selectable_label(issue.kind == *k, k.as_str()).clicked()
                                && issue.kind != *k
                            {
                                set_kind = Some(*k);
                            }
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label("Parent:");
                match &issue.parent {
                    Some(p) => {
                        if ui.link(p).clicked() {
                            select_key = Some(p.clone());
                        }
                        if ui.small_button("Detach").clicked() {
                            reparent = Some(None);
                        }
                    }
                    None => {
                        ui.label(RichText::new("none").color(t.text_weak));
                    }
                }
                if issue.kind != IssueKind::Epic {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_parent)
                            .hint_text("Parent KEY-n")
                            .desired_width(90.0),
                    );
                    if ui
                        .add_enabled(
                            !self.new_parent.trim().is_empty(),
                            egui::Button::new("Set Parent"),
                        )
                        .clicked()
                    {
                        reparent = Some(Some(self.new_parent.trim().to_string()));
                    }
                }
            });
            if !detail.children.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Children:");
                    for c in &detail.children {
                        if ui.link(c).clicked() {
                            select_key = Some(c.clone());
                        }
                    }
                });
            }
            if let Some(r) = &detail.rollup {
                let text = if r.basis == "points" {
                    format!(
                        "{} of {} points done ({:.0}%) across {} issues",
                        r.points_done, r.points, r.percent_done, r.issues
                    )
                } else {
                    format!(
                        "{} of {} issues done ({:.0}%)",
                        r.issues_done, r.issues, r.percent_done
                    )
                };
                ui.add(
                    ProgressBar::new((r.percent_done / 100.0) as f32)
                        .desired_width(ui.available_width())
                        .text(text),
                );
            }

            ui.add_space(12.0);
            ui.separator();

            // Blockers & Relations (CRITICAL FEATURE!)
            ui.heading("🔗 Relations & Dependencies");

            if !detail.blocked_by_open.is_empty() {
                ui.add_space(4.0);
                egui::Frame::new()
                    .fill(theme::tone_colors(Tone::Danger, t.dark).1)
                    .stroke(Stroke::new(1.0, theme::tone_solid(Tone::Danger, t.dark)))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("⚠️ Gated: Blocked by open issues:")
                                .color(theme::tone_solid(Tone::Danger, t.dark))
                                .strong(),
                        );
                        for blocker in &detail.blocked_by_open {
                            ui.horizontal(|ui| {
                                if ui
                                    .link(blocker)
                                    .on_hover_text("Click to inspect blocker")
                                    .clicked()
                                {
                                    select_key = Some(blocker.clone());
                                }
                                ui.label("(must finish before moving to progress/done)");
                            });
                        }
                    });
                ui.add_space(6.0);
            }

            if detail.relations.is_empty() {
                ui.label(
                    RichText::new("No relations linked to this issue.")
                        .italics()
                        .color(t.text_weak),
                );
            } else {
                for rel in &detail.relations {
                    ui.horizontal(|ui| {
                        let kind_str = match rel.kind {
                            RelationKind::Blocks => {
                                if rel.direction == "outward" {
                                    "Blocks"
                                } else {
                                    "Blocked by"
                                }
                            }
                            RelationKind::RelatesTo => "Relates to",
                            RelationKind::Duplicates => "Duplicates",
                        };

                        let tone = if rel.kind == RelationKind::Blocks
                            && !rel.other_status.is_finished()
                        {
                            Tone::Danger
                        } else {
                            Tone::Info
                        };

                        widgets::pill(ui, kind_str, tone);
                        if ui.link(&rel.other).on_hover_text("Jump to issue").clicked() {
                            select_key = Some(rel.other.clone());
                        }
                        widgets::pill(ui, rel.other_status.as_str(), status_tone(rel.other_status));
                        ui.label(&rel.other_title);

                        if ui
                            .small_button("✕")
                            .on_hover_text("Unlink relation")
                            .clicked()
                        {
                            remove_relation =
                                Some((issue.key.clone(), rel.kind, rel.other.clone()));
                        }
                    });
                }
            }

            ui.add_space(6.0);
            ui.label(RichText::new("Add Relation:").family(semibold()));
            ui.horizontal(|ui| {
                let kind_label = match self.new_rel_kind {
                    RelationKind::Blocks => "Blocks",
                    RelationKind::RelatesTo => "Relates To",
                    RelationKind::Duplicates => "Duplicates",
                };
                egui::ComboBox::from_id_salt("rel_kind_select")
                    .selected_text(kind_label)
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(self.new_rel_kind == RelationKind::Blocks, "Blocks")
                            .clicked()
                        {
                            self.new_rel_kind = RelationKind::Blocks;
                        }
                        if ui
                            .selectable_label(
                                self.new_rel_kind == RelationKind::RelatesTo,
                                "Relates To",
                            )
                            .clicked()
                        {
                            self.new_rel_kind = RelationKind::RelatesTo;
                        }
                        if ui
                            .selectable_label(
                                self.new_rel_kind == RelationKind::Duplicates,
                                "Duplicates",
                            )
                            .clicked()
                        {
                            self.new_rel_kind = RelationKind::Duplicates;
                        }
                    });

                ui.add(
                    egui::TextEdit::singleline(&mut self.new_rel_target)
                        .hint_text("Target KEY-n")
                        .desired_width(110.0),
                );

                if ui.button("+ Link").clicked() {
                    let tgt = self.new_rel_target.clone();
                    self.execute_add_relation(&issue.key, self.new_rel_kind, &tgt);
                }
            });

            ui.add_space(12.0);
            ui.separator();

            // Commits
            ui.heading("⎇ Linked Commits");
            if detail.commits.is_empty() {
                ui.label(
                    RichText::new("No git commits linked to this issue.")
                        .italics()
                        .color(t.text_weak),
                );
            } else {
                for c in &detail.commits {
                    ui.horizontal(|ui| {
                        let short = if c.hash.len() > 8 {
                            &c.hash[..8]
                        } else {
                            &c.hash
                        };
                        ui.label(
                            RichText::new(short)
                                .family(egui::FontFamily::Monospace)
                                .strong(),
                        );
                        if c.closing {
                            widgets::pill(ui, "Closes", Tone::Ok);
                        } else {
                            widgets::pill(ui, "Mention", Tone::Neutral);
                        }
                        ui.label(format!("by {}", c.author));
                        ui.label(RichText::new(&c.subject).color(t.text));
                    });
                }
            }

            ui.add_space(12.0);
            ui.separator();

            // Comments (Tier 5)
            let live = detail.comments.iter().filter(|c| !c.deleted).count();
            ui.heading(format!("Comments ({live})"));
            if !detail.watchers.is_empty() {
                let names: Vec<String> = detail
                    .watchers
                    .iter()
                    .map(|p| format!("@{}", p.handle))
                    .collect();
                ui.label(
                    RichText::new(format!("Watching: {}", names.join(", "))).color(t.text_weak),
                );
            }
            let me = taskcraft_store::person_handle(&self.ctx.actor);
            for c in &detail.comments {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        widgets::pill(ui, &format!("@{}", c.author), Tone::Info);
                        ui.label(RichText::new(&c.created_at).color(t.text_weak).size(11.0));
                        if c.edited_at.is_some() {
                            ui.label(RichText::new("edited").italics().color(t.text_weak));
                        }
                        if c.deleted {
                            widgets::pill(ui, "Deleted", Tone::Neutral);
                        } else if c.author == me && ui.small_button("Delete").clicked() {
                            delete_comment = Some((c.id, c.version));
                        }
                    });
                    let body = RichText::new(&c.body);
                    ui.label(if c.deleted {
                        body.strikethrough().color(t.text_weak)
                    } else {
                        body.color(t.text)
                    });
                });
            }
            ui.add(
                egui::TextEdit::multiline(&mut self.new_comment)
                    .hint_text("Write a comment. @handle mentions someone.")
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            if ui
                .add_enabled(
                    !self.new_comment.trim().is_empty(),
                    egui::Button::new("Post Comment"),
                )
                .clicked()
            {
                post_comment = true;
            }

            ui.add_space(12.0);
            ui.separator();

            // History
            ui.collapsing("⏱ Audit History", |ui| {
                if detail.history.is_empty() {
                    ui.label("No history records");
                } else {
                    for h in &detail.history {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&h.at).color(t.text_weak).size(11.0));
                            widgets::pill(ui, &h.actor, Tone::Info);
                            ui.label(&h.what);
                        });
                    }
                }
            });
        });

        if let Some(lbl) = remove_label {
            let mut new_labels = issue.labels.clone();
            new_labels.retain(|l| l != &lbl);
            self.execute_update_issue_patch(
                &issue.key,
                IssuePatch {
                    labels: Some(new_labels),
                    ..Default::default()
                },
            );
        }
        if let Some(k) = select_key {
            self.select_issue(&k);
        }
        if let Some((src, kind, tgt)) = remove_relation {
            self.execute_remove_relation(&src, kind, &tgt);
        }
        if post_comment {
            self.execute_add_comment(&issue.key);
        }
        if let Some(k) = set_kind {
            self.execute_update_issue_patch(
                &issue.key,
                IssuePatch {
                    kind: Some(k),
                    ..Default::default()
                },
            );
        }
        if let Some(p) = reparent {
            self.execute_reparent(&issue.key, p);
        }
        if let Some((id, version)) = delete_comment {
            self.execute_delete_comment(&issue.key, id, version);
        }
    }

    // =========================================================================
    // MODALS & DIALOGS
    // =========================================================================

    fn render_modals(&mut self, ctx: &egui::Context) {
        // 1. New Issue Modal
        if self.show_new_issue {
            egui::Window::new("Create New Issue")
                .collapsible(false)
                .resizable(false)
                .default_width(480.0)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Project:");
                        ui.label(
                            RichText::new(self.selected_project.as_deref().unwrap_or("-"))
                                .family(semibold()),
                        );
                    });

                    ui.add_space(4.0);
                    ui.label(RichText::new("Title (required):").family(semibold()));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_issue_title)
                            .desired_width(ui.available_width()),
                    );

                    ui.add_space(4.0);
                    ui.label("Description:");
                    ui.add(
                        egui::TextEdit::multiline(&mut self.new_issue_desc)
                            .desired_rows(4)
                            .desired_width(ui.available_width()),
                    );

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Status:");
                        egui::ComboBox::from_id_salt("modal_status")
                            .selected_text(self.new_issue_status.as_str().to_uppercase())
                            .width(130.0)
                            .show_ui(ui, |ui| {
                                for s in Status::ALL {
                                    if ui
                                        .selectable_label(
                                            self.new_issue_status == *s,
                                            s.as_str().to_uppercase(),
                                        )
                                        .clicked()
                                    {
                                        self.new_issue_status = *s;
                                    }
                                }
                            });

                        ui.label("Priority:");
                        egui::ComboBox::from_id_salt("modal_prio")
                            .selected_text(self.new_issue_priority.as_str().to_uppercase())
                            .width(120.0)
                            .show_ui(ui, |ui| {
                                for p in Priority::ALL {
                                    if ui
                                        .selectable_label(
                                            self.new_issue_priority == *p,
                                            p.as_str().to_uppercase(),
                                        )
                                        .clicked()
                                    {
                                        self.new_issue_priority = *p;
                                    }
                                }
                            });
                    });

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Assignee:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_issue_assignee)
                                .desired_width(120.0),
                        );

                        ui.label("Pts:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_issue_estimate)
                                .desired_width(60.0),
                        );
                    });

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Due Date:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_issue_due_date)
                                .hint_text("YYYY-MM-DD")
                                .desired_width(120.0),
                        );

                        ui.label("Milestone:");
                        let mut m_opts = vec!["None".to_string()];
                        for m in &self.milestones {
                            m_opts.push(m.title.clone());
                        }
                        let sel = if self.new_issue_milestone.is_empty() {
                            "None"
                        } else {
                            &self.new_issue_milestone
                        };
                        egui::ComboBox::from_id_salt("modal_ms")
                            .selected_text(sel)
                            .width(140.0)
                            .show_ui(ui, |ui| {
                                for m in m_opts {
                                    let is_sel = if m == "None" {
                                        self.new_issue_milestone.is_empty()
                                    } else {
                                        self.new_issue_milestone == m
                                    };
                                    if ui.selectable_label(is_sel, &m).clicked() {
                                        if m == "None" {
                                            self.new_issue_milestone.clear();
                                        } else {
                                            self.new_issue_milestone = m;
                                        }
                                    }
                                }
                            });
                    });

                    ui.add_space(6.0);
                    ui.label("Labels (comma separated):");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_issue_labels)
                            .hint_text("bug, backend, urgent")
                            .desired_width(ui.available_width()),
                    );

                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if widgets::primary(ui, "Create Issue").clicked() {
                            self.execute_create_issue();
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_new_issue = false;
                        }
                    });
                });
        }

        // 2. New Project Modal
        if self.show_new_project {
            egui::Window::new("Create New Project")
                .collapsible(false)
                .resizable(false)
                .default_width(400.0)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label(RichText::new("Project Key (e.g. OPS, TC):").family(semibold()));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_project_key).desired_width(120.0),
                    );

                    ui.add_space(6.0);
                    ui.label(RichText::new("Project Name:").family(semibold()));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_project_name)
                            .desired_width(ui.available_width()),
                    );

                    ui.add_space(6.0);
                    ui.label("Repository Path (optional):");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_project_repo)
                            .hint_text("D:/Dev/repos/myrepo")
                            .desired_width(ui.available_width()),
                    );

                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if widgets::primary(ui, "Create Project").clicked() {
                            self.execute_create_project();
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_new_project = false;
                        }
                    });
                });
        }

        // 3. New Milestone Modal
        if self.show_new_milestone {
            egui::Window::new("Create New Milestone")
                .collapsible(false)
                .resizable(false)
                .default_width(380.0)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label(RichText::new("Milestone Title:").family(semibold()));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_milestone_title)
                            .desired_width(ui.available_width()),
                    );

                    ui.add_space(6.0);
                    ui.label("Due Date (optional, YYYY-MM-DD):");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_milestone_due)
                            .hint_text("2026-12-01")
                            .desired_width(140.0),
                    );

                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if widgets::primary(ui, "Create Milestone").clicked() {
                            self.execute_create_milestone();
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_new_milestone = false;
                        }
                    });
                });
        }

        // 4. Force Move Confirmation Dialog
        if self.show_force_move {
            let key_opt = self.selected_issue_key.clone();
            let target_opt = self.force_move_target;

            egui::Window::new("Force Status Move")
                .collapsible(false)
                .resizable(false)
                .default_width(360.0)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    if let (Some(ref key), Some(target)) = (key_opt, target_opt) {
                        ui.label(format!("This issue has open blockers. Are you sure you want to force move {key} to '{}'?", target.as_str()));
                        ui.label(RichText::new("Notice: Forced moves are recorded with your actor in the audit trail.").italics().color(theme::current(ui.ctx()).text_weak));

                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if widgets::danger(ui, "Force Move").clicked() {
                                self.execute_move_issue(key, target, true);
                                self.show_force_move = false;
                            }
                            if ui.button("Cancel").clicked() {
                                self.show_force_move = false;
                            }
                        });
                    } else {
                        self.show_force_move = false;
                    }
                });
        }
    }
}

impl App {
    fn paint_menu(&mut self, ui: &mut egui::Ui) {
        let docs = bookend::repo_docs(env!("CARGO_MANIFEST_DIR"));
        let spec = Spec {
            name: "TaskCraft",
            version: env!("CARGO_PKG_VERSION"),
            description: "Issues, the board, and git sync.",
            repository: "https://github.com/sandraschi/taskcraft",
            docs: &docs,
            dark: self.settings.dark,
            zoom: self.settings.zoom,
            file: self.file_items(),
            view: vec![
                Item::action("Board", "view.board").checked(self.current_view == ActiveView::Board),
                Item::action("Table", "view.table").checked(self.current_view == ActiveView::Table),
                Item::action("Milestones", "view.milestones")
                    .checked(self.current_view == ActiveView::Milestones),
                Item::action("Git sync", "view.git")
                    .checked(self.current_view == ActiveView::GitSync),
                Item::action("History", "view.history")
                    .checked(self.current_view == ActiveView::History),
            ],
            middle: self.middle_menus(),
        };
        let change = self.bar.show(ui, &spec);
        if let Some(dark) = change.dark {
            self.settings.dark = dark;
        }
        if let Some(zoom) = change.zoom {
            self.settings.zoom = zoom;
        }
        if let Some(id) = change.action.as_deref() {
            self.menu_action(id, true);
        }
    }

    /// File items above Quit.
    fn file_items(&self) -> Vec<Item> {
        let cmd = egui::Modifiers::COMMAND;
        vec![
            Item::action("New Project...", "file.new_project")
                .key(cmd, egui::Key::N)
                .key_text("Ctrl+N"),
            Item::action("Export Issues as CSV...", "file.export_csv")
                .key(cmd, egui::Key::E)
                .key_text("Ctrl+E")
                .enabled(self.selected_project.is_some())
                .hint("Every issue of the current project, semicolon-separated for Excel."),
            Item::sep(),
            Item::action("Refresh", "app.refresh")
                .key(egui::Modifiers::NONE, egui::Key::F5)
                .key_text("F5"),
        ]
    }

    /// Insert, Modify and Commands (Craft Menu Bar Standard middle menus).
    fn middle_menus(&self) -> Vec<Menu> {
        let cmd_shift = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
        let project = self.selected_project.is_some();
        let issue = self.selected_issue_key.is_some();
        let pick_issue = "Select an issue first.";
        let insert = Menu::new(
            "Insert",
            vec![
                Item::action("New Issue...", "insert.issue")
                    .key(cmd_shift, egui::Key::I)
                    .key_text("Ctrl+Shift+I")
                    .enabled(project),
                Item::action("New Milestone...", "insert.milestone").enabled(project),
                Item::action("Relation from Selected Issue...", "insert.relation")
                    .enabled(issue)
                    .hint(pick_issue),
            ],
        );
        let moves = Status::ALL
            .iter()
            .map(|s| Item::action(status_label(*s), &format!("modify.move.{}", s.as_str())))
            .collect();
        let priorities = Priority::ALL
            .iter()
            .map(|p| {
                Item::action(
                    priority_label(*p),
                    &format!("modify.priority.{}", p.as_str()),
                )
            })
            .collect();
        let modify = Menu::new(
            "Modify",
            vec![
                Item::sub("Move To", moves).enabled(issue),
                Item::sub("Priority", priorities).enabled(issue),
                Item::action("Edit Fields...", "modify.edit")
                    .enabled(issue)
                    .hint(pick_issue),
                Item::action("Open in Inspector", "modify.inspect")
                    .enabled(issue)
                    .hint(pick_issue),
            ],
        );
        let commands = Menu::new(
            "Commands",
            vec![
                Item::action("Run Git Sync", "cmd.git_sync").enabled(project),
                Item::action("Drain Cross-App Inbox", "cmd.inbox_drain"),
                Item::sep(),
                Item::action("Tool Palette...", "cmd.palette").key_text("Ctrl+K"),
                Item::action("Seed Demo Data", "cmd.demo_seed")
                    .enabled(self.projects.is_empty())
                    .hint("Only on an empty database."),
            ],
        );
        vec![insert, modify, commands]
    }

    /// Run a menu action. With `run` false it only reports whether the id is
    /// known (used by the menu test, so every menu item has a handler).
    fn menu_action(&mut self, id: &str, run: bool) -> bool {
        let status = id
            .strip_prefix("modify.move.")
            .and_then(|s| Status::parse(s).ok());
        let priority = id
            .strip_prefix("modify.priority.")
            .and_then(|s| Priority::parse(s).ok());
        let known = status.is_some()
            || priority.is_some()
            || matches!(
                id,
                "app.refresh"
                    | "file.new_project"
                    | "file.export_csv"
                    | "insert.issue"
                    | "insert.milestone"
                    | "insert.relation"
                    | "modify.edit"
                    | "modify.inspect"
                    | "cmd.git_sync"
                    | "cmd.inbox_drain"
                    | "cmd.palette"
                    | "cmd.demo_seed"
                    | "view.board"
                    | "view.table"
                    | "view.milestones"
                    | "view.git"
                    | "view.history"
            );
        if !known || !run {
            return known;
        }
        let selected = self.selected_issue_key.clone();
        if let (Some(s), Some(key)) = (status, selected.as_deref()) {
            self.execute_move_issue(key, s, false);
            return true;
        }
        if let (Some(p), Some(key)) = (priority, selected.as_deref()) {
            let patch = IssuePatch {
                priority: Some(p),
                ..IssuePatch::default()
            };
            self.execute_update_issue_patch(key, patch);
            return true;
        }
        match id {
            "app.refresh" => self.reload_projects(),
            "file.new_project" => self.show_new_project = true,
            "file.export_csv" => self.export_csv(),
            "insert.issue" => self.open_new_issue_modal(None, None),
            "insert.milestone" => self.show_new_milestone = true,
            "insert.relation" => {
                if let Some(key) = selected {
                    self.open_tool_form("task_relation_add", &[("source", key)]);
                }
            }
            "modify.edit" => {
                if let Some(key) = selected {
                    self.open_tool_form("task_issue_update", &[("issue", key)]);
                }
            }
            "modify.inspect" => self.inspector_open = selected.is_some(),
            "cmd.git_sync" => self.execute_git_sync(),
            "cmd.inbox_drain" => self.run_tool("task_inbox_drain"),
            "cmd.palette" => self.palette.open_list(),
            "cmd.demo_seed" => self.execute_demo_seed(),
            "view.board" => self.current_view = ActiveView::Board,
            "view.table" => self.current_view = ActiveView::Table,
            "view.milestones" => self.current_view = ActiveView::Milestones,
            "view.git" => self.current_view = ActiveView::GitSync,
            "view.history" => self.current_view = ActiveView::History,
            _ => {}
        }
        true
    }

    /// Open one tool's form in the palette with some fields filled in.
    fn open_tool_form(&mut self, tool: &str, prefill: &[(&str, String)]) {
        let Some(t) = tools().into_iter().find(|t| t["name"] == tool) else {
            self.toasts.err(format!("Unknown tool {tool}"));
            return;
        };
        self.palette.prefill = prefill
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect();
        self.palette.open_tool(&t);
        self.palette.prefill.clear();
    }

    /// Run a tool without arguments and report the result.
    fn run_tool(&mut self, tool: &str) {
        let args = serde_json::Value::Object(serde_json::Map::new());
        match Command::from_tool(tool, &args).and_then(|cmd| dispatch(&mut self.ctx, cmd)) {
            Ok(out) => {
                self.toasts.ok(format!("{tool}: {out}"));
                self.reload_projects();
            }
            Err(e) => self.toasts.err(format!("{tool} failed: {e}")),
        }
    }

    /// File > Export Issues as CSV.
    fn export_csv(&mut self) {
        let Some(project) = self.selected_project.clone() else {
            self.toasts.err("Select a project first");
            return;
        };
        let issues: Vec<&Issue> = self
            .all_issues
            .iter()
            .filter(|i| i.project == project)
            .collect();
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{project}-issues.csv"))
            .add_filter("CSV", &["csv"])
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, issues_csv(&issues)) {
            Ok(()) => self.toasts.ok(format!(
                "Exported {} issues to {}",
                issues.len(),
                path.display()
            )),
            Err(e) => self.toasts.err(format!("Export failed: {e}")),
        }
    }
}

fn status_label(s: Status) -> &'static str {
    match s {
        Status::Backlog => "Backlog",
        Status::Todo => "To Do",
        Status::InProgress => "In Progress",
        Status::InReview => "In Review",
        Status::Done => "Done",
        Status::Canceled => "Canceled",
    }
}

fn priority_label(p: Priority) -> &'static str {
    match p {
        Priority::Urgent => "Urgent",
        Priority::High => "High",
        Priority::Medium => "Medium",
        Priority::Low => "Low",
    }
}

/// Issues as CSV for Excel in German-speaking locales: UTF-8 with BOM,
/// semicolon-separated, CRLF line ends (RFC 4180 quoting).
fn issues_csv(issues: &[&Issue]) -> String {
    fn field(s: &str) -> String {
        if s.contains([';', '"', '\n', '\r']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    }
    let mut out = String::from("\u{feff}");
    out.push_str("key;title;status;priority;estimate;assignee;milestone;due_date;labels;created_at;updated_at;closed_at\r\n");
    for i in issues {
        let cols = [
            i.key.clone(),
            i.title.clone(),
            i.status.as_str().to_string(),
            i.priority.as_str().to_string(),
            i.estimate.map(|e| e.to_string()).unwrap_or_default(),
            i.assignee.clone().unwrap_or_default(),
            i.milestone.clone().unwrap_or_default(),
            i.due_date.map(|d| d.to_string()).unwrap_or_default(),
            i.labels.join(","),
            i.created_at.clone(),
            i.updated_at.clone(),
            i.closed_at.clone().unwrap_or_default(),
        ];
        let line: Vec<String> = cols.iter().map(|c| field(c)).collect();
        out.push_str(&line.join(";"));
        out.push_str("\r\n");
    }
    out
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.paint_menu(ui);

        // 1. Palette shortcut and signal dock refresh
        self.palette.handle_shortcut(&ctx);
        self.dock.refresh(&self.ctx.bus, APP_NAME, false);

        // 2. Render Top Menu Bar
        self.render_top_bar(ui);

        // 3. Render Status Bar
        self.render_status_bar(ui);

        // 4. Render Left Navigation Rail
        self.render_nav_rail(ui);

        // 5. Signals Dock (cross-app fleet bus)
        if self.dock.open {
            let mut action = None;
            egui::Panel::right("signals_dock_panel")
                .default_size(380.0)
                .frame(
                    egui::Frame::new()
                        .fill(theme::current(&ctx).surface)
                        .inner_margin(egui::Margin::same(12)),
                )
                .show(ui, |ui| {
                    action = self.dock.show(
                        ui,
                        &[
                            ("ticket.escalated", "deskcraft ticket becomes an issue"),
                            (
                                "contract.deadline",
                                "contractcraft notice date becomes an issue",
                            ),
                            ("capacity.changed", "teamcraft hours per week"),
                        ],
                        &[],
                    );
                });

            if let Some(act) = action {
                match act {
                    SignalAction::DrainAll => {
                        let _ = dispatch(&mut self.ctx, Command::InboxDrain);
                        self.reload_active_data();
                        self.toasts.ok("Applied incoming bus signals");
                    }
                    SignalAction::Retry(id) => {
                        let _ = self.ctx.bus.retry(APP_NAME, &id);
                        self.reload_active_data();
                    }
                }
            }
        }

        // 6. Right Inspector Drawer (if open and issue selected)
        if self.inspector_open && self.selected_detail.is_some() {
            egui::Panel::right("inspector_side_panel")
                .default_size(440.0)
                .frame(
                    egui::Frame::new()
                        .fill(theme::current(&ctx).surface)
                        .inner_margin(egui::Margin::same(12)),
                )
                .show(ui, |ui| {
                    self.render_inspector(ui);
                });
        }

        // 7. Central Panel: Active View
        egui::CentralPanel::default().show(ui, |ui| match self.current_view {
            ActiveView::Board => self.render_board_view(ui),
            ActiveView::Table => self.render_table_view(ui),
            ActiveView::Milestones => self.render_milestones_view(ui),
            ActiveView::GitSync => self.render_git_sync_view(ui),
            ActiveView::History => self.render_history_view(ui),
        });

        // 8. Palette handling
        if let Some((tool_name, args)) = self.palette.show(&ctx, &tools()) {
            if tool_name != "__back" {
                if let Ok(cmd) = Command::from_tool(&tool_name, &args) {
                    match dispatch(&mut self.ctx, cmd) {
                        Ok(_) => {
                            self.palette.close();
                            self.toasts.ok(format!("Executed {tool_name}"));
                            self.reload_projects();
                        }
                        Err(e) => {
                            self.palette.fail(e);
                        }
                    }
                } else {
                    self.palette.fail("Could not parse tool command".into());
                }
            }
        }

        // 9. Floating Modals
        self.render_modals(&ctx);

        // 10. Floating Toasts
        self.toasts.show(&ctx);
    }
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

/// `--smoke`: build the real menu bar on an in-memory database and check it
/// against the Craft Menu Bar Standard (bookend order, Help items).
fn smoke_menus() -> Result<(), String> {
    let tracker = Tracker::open_in_memory().map_err(|e| e.to_string())?;
    let ctx = Ctx {
        tracker,
        confinement: Confinement(None),
        bus: Bus::new(std::env::temp_dir().join("taskcraft_smoke_bus")),
        actor: "smoke".to_string(),
    };
    let app = App::new(ctx, PathBuf::from("smoke.db"));
    let spec = Spec {
        name: APP_NAME,
        version: env!("CARGO_PKG_VERSION"),
        description: "Issues, the board, and git sync.",
        repository: "",
        docs: &[],
        dark: true,
        zoom: 1.0,
        file: app.file_items(),
        view: Vec::new(),
        middle: app.middle_menus(),
    };
    bookend::check(&bookend::menus(&spec))
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--smoke") {
        if let Err(e) = smoke_menus() {
            eprintln!("smoke: menu: {e}");
            std::process::exit(1);
        }
        println!("smoke: ok (taskcraft-app)");
        return Ok(());
    }

    let cli = Cli::parse();
    if cli.smoke {
        if let Err(e) = smoke_menus() {
            eprintln!("smoke: menu: {e}");
            std::process::exit(1);
        }
        println!("smoke: ok (taskcraft-app)");
        return Ok(());
    }

    let db_path = cli.db.unwrap_or_else(default_db);
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let tracker = match Tracker::open(&db_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!(
                "Failed to open tracker database at {}: {e}",
                db_path.display()
            );
            std::process::exit(1);
        }
    };

    let ctx = Ctx {
        tracker,
        confinement: Confinement(None),
        bus: Bus::new(Bus::default_root()),
        actor: std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "desktop".to_string()),
    };

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 550.0])
            .with_title("TaskCraft Workstation"),
        ..Default::default()
    };

    let app_db_path = db_path.clone();
    eframe::run_native(
        "TaskCraft",
        native_options,
        Box::new(move |cc| {
            let mut app = App::new(ctx, app_db_path);
            if let Some(m) = app.ctx.tracker.migrated().map(|m| m.to_string()) {
                eprintln!("taskcraft: {m}");
                app.toasts.ok(m);
            }
            shell::install(&cc.egui_ctx, &app.settings);
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tones() {
        assert_eq!(status_tone(Status::Done), Tone::Ok);
        assert_eq!(status_tone(Status::Backlog), Tone::Neutral);
        assert_eq!(priority_tone(Priority::Urgent), Tone::Danger);
        assert_eq!(priority_tone(Priority::High), Tone::Warn);
        assert_eq!(priority_tone(Priority::Medium), Tone::Info);
        assert_eq!(priority_tone(Priority::Low), Tone::Neutral);
    }

    #[test]
    fn test_app_creation_with_in_memory_tracker() {
        let tracker = Tracker::open_in_memory().expect("open memory db");
        let ctx = Ctx {
            tracker,
            confinement: Confinement(None),
            bus: Bus::new(std::env::temp_dir().join("taskcraft_test_bus")),
            actor: "test_actor".to_string(),
        };
        let mut app = App::new(ctx, PathBuf::from("memory.db"));
        assert_eq!(app.current_view, ActiveView::Board);
        assert!(!app.inspector_open);

        // Create project and test reload
        app.new_project_key = "TC".to_string();
        app.new_project_name = "TaskCraft Test".to_string();
        app.execute_create_project();

        assert_eq!(app.projects.len(), 1);
        assert_eq!(app.selected_project.as_deref(), Some("TC"));

        // Create issue and verify
        app.new_issue_title = "First Test Issue".to_string();
        app.new_issue_status = Status::Todo;
        app.new_issue_priority = Priority::High;
        app.execute_create_issue();

        assert_eq!(app.all_issues.len(), 1);
        assert_eq!(app.all_issues[0].key, "TC-1");
        assert_eq!(app.cards.len(), 1);
        assert_eq!(app.cards[0].key, "TC-1");
        assert_eq!(app.selected_issue_key.as_deref(), Some("TC-1"));
        assert!(app.inspector_open);

        // Move issue
        app.execute_move_issue("TC-1", Status::InProgress, false);
        assert_eq!(app.all_issues[0].status, Status::InProgress);

        // Menu actions on the selected issue go through the same commands.
        assert!(app.menu_action("modify.priority.urgent", true));
        assert_eq!(app.all_issues[0].priority, Priority::Urgent);
        assert!(app.menu_action("modify.move.in_review", true));
        assert_eq!(app.all_issues[0].status, Status::InReview);
        assert!(app.menu_action("insert.relation", true));
        assert_eq!(app.palette.form_tool(), Some("task_relation_add"));
        assert_eq!(app.palette.form_value("source").as_deref(), Some("TC-1"));
    }

    fn memory_app() -> App {
        let tracker = Tracker::open_in_memory().expect("open memory db");
        let ctx = Ctx {
            tracker,
            confinement: Confinement(None),
            bus: Bus::new(std::env::temp_dir().join("taskcraft_test_bus_menu")),
            actor: "test_actor".to_string(),
        };
        App::new(ctx, PathBuf::from("memory.db"))
    }

    /// Craft Menu Bar Standard: Insert, Modify and Commands exist, every
    /// item has a handler, and issue items are disabled without a selection.
    #[test]
    fn every_menu_item_has_a_handler() {
        let mut app = memory_app();
        let menus = app.middle_menus();
        let titles: Vec<&str> = menus.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(titles, vec!["Insert", "Modify", "Commands"]);
        let mut all = menus.clone();
        all.push(Menu::new("File", app.file_items()));
        let mut ids = Vec::new();
        for leaf in craft_biz_ui::menu::leaves(&all) {
            if let craft_biz_ui::menu::Kind::Action(id) = &leaf.kind {
                ids.push(id.clone());
            }
        }
        assert!(ids.len() >= 20, "{ids:?}");
        for id in &ids {
            assert!(app.menu_action(id, false), "no handler for {id}");
        }
        assert!(!app.menu_action("modify.move.nonsense", false));
        assert_eq!(smoke_menus(), Ok(()));
        // Nothing selected: issue items are disabled, demo seed is enabled.
        let modify = &menus[1];
        assert!(modify.items.iter().all(|i| !i.enabled));
        let seed = menus[2].items.iter().find(|i| i.label == "Seed Demo Data");
        assert!(seed.is_some_and(|i| i.enabled));
    }

    /// Craft Menu Bar Standard, end to end on the real bar: with File open,
    /// hovering Commands switches menus; a Commands item runs through
    /// `dispatch` and changes the database.
    #[test]
    fn menu_bar_switches_on_hover_and_runs_commands() {
        use egui_kittest::kittest::Queryable;
        let mut h = egui_kittest::Harness::builder()
            .with_size([1280.0, 800.0])
            .build_ui_state(|ui, app: &mut App| app.paint_menu(ui), memory_app());
        h.run_steps(3);
        h.get_by_label("File").click();
        h.run_steps(2);
        assert!(
            h.query_by_label_contains("New Project...").is_some(),
            "File is open"
        );
        h.get_by_label("Commands").hover();
        h.run_steps(3);
        assert!(
            h.query_by_label_contains("New Project...").is_none(),
            "File closed"
        );
        assert!(
            h.query_by_label_contains("Run Git Sync").is_some(),
            "Commands open"
        );
        assert!(h.state().projects.is_empty());
        h.get_by_label_contains("Seed Demo Data").click();
        h.run_steps(3);
        assert!(
            !h.state().projects.is_empty(),
            "demo seed ran from the menu"
        );
    }

    /// Tofu Guard: every menu title, item and hint renders with the craft
    /// fonts (regular, semibold, bold, mono).
    #[test]
    fn menu_text_has_no_tofu() {
        let mut app = memory_app();
        app.new_project_key = "TC".to_string();
        app.new_project_name = "Test".to_string();
        app.execute_create_project();
        app.new_issue_title = "Glyphs".to_string();
        app.execute_create_issue();
        let spec = Spec {
            name: APP_NAME,
            version: env!("CARGO_PKG_VERSION"),
            description: "Issues, the board, and git sync.",
            repository: "",
            docs: &[],
            dark: true,
            zoom: 1.0,
            file: app.file_items(),
            view: Vec::new(),
            middle: app.middle_menus(),
        };
        let menus = bookend::menus(&spec);
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
        out.textures_delta.clear();
        let report = craft_biz_ui::menu::tofu_report(&ctx, &menus, &[]);
        assert!(report.is_empty(), "{report:?}");
        // The guard is live: a character no font ships is reported.
        let missing = craft_biz_ui::menu::missing_glyphs(&ctx, "ok \u{10fffd}");
        assert_eq!(missing, vec!['\u{10fffd}']);
    }

    /// Inspector hierarchy: reparent through the app path, then detach.
    #[test]
    fn inspector_reparents_and_shows_rollup() {
        let mut app = memory_app();
        app.new_project_key = "TC".to_string();
        app.new_project_name = "Test".to_string();
        app.execute_create_project();
        for title in ["Epic", "Story"] {
            app.new_issue_title = title.to_string();
            app.new_issue_estimate = "5".to_string();
            app.execute_create_issue();
        }
        app.execute_update_issue_patch(
            "TC-1",
            IssuePatch {
                kind: Some(IssueKind::Epic),
                ..Default::default()
            },
        );
        app.execute_reparent("TC-2", Some("TC-1".into()));
        app.load_issue_detail("TC-1");
        let d = app.selected_detail.clone().expect("detail");
        assert_eq!(d.issue.kind, IssueKind::Epic);
        assert_eq!(d.children, vec!["TC-2"]);
        assert_eq!(d.rollup.as_ref().map(|r| r.points), Some(5));
        // A refused move is a toast, not a change.
        app.execute_reparent("TC-1", Some("TC-2".into()));
        assert!(app
            .toasts
            .latest()
            .is_some_and(|(m, _)| m.contains("epic cannot have a parent")));
        app.execute_reparent("TC-2", None);
        app.load_issue_detail("TC-1");
        assert!(app
            .selected_detail
            .as_ref()
            .is_some_and(|d| d.rollup.is_none()));
    }

    /// Inspector comment thread: post (with an unknown mention), then delete.
    #[test]
    fn inspector_posts_and_deletes_comments() {
        let mut app = memory_app();
        app.new_project_key = "TC".to_string();
        app.new_project_name = "Test".to_string();
        app.execute_create_project();
        app.new_issue_title = "Discuss".to_string();
        app.new_issue_assignee = "Joe Mocky".to_string();
        app.execute_create_issue();
        app.new_comment = "  ".to_string();
        app.execute_add_comment("TC-1");
        assert_eq!(
            app.selected_detail.as_ref().map(|d| d.comments.len()),
            Some(0)
        );

        app.new_comment = "ping @joe-mocky and @nobody".to_string();
        app.execute_add_comment("TC-1");
        assert!(app.new_comment.is_empty(), "box cleared after posting");
        let d = app.selected_detail.clone().expect("detail");
        assert_eq!(d.comments.len(), 1);
        assert_eq!(d.comments[0].mentions, vec!["joe-mocky"]);
        assert_eq!(d.watchers.len(), 1);
        assert!(app
            .toasts
            .latest()
            .is_some_and(|(m, _)| m.contains("@nobody")));

        let (id, v) = (d.comments[0].id, d.comments[0].version);
        app.execute_delete_comment("TC-1", id, v);
        let d = app.selected_detail.clone().expect("detail");
        assert!(d.comments[0].deleted);
    }

    #[test]
    fn csv_export_quotes_and_uses_semicolons() {
        let mut app = memory_app();
        app.new_project_key = "TC".to_string();
        app.new_project_name = "Test".to_string();
        app.execute_create_project();
        app.new_issue_title = "Fix \"quoted\"; export".to_string();
        app.new_issue_labels = "bug, export".to_string();
        app.execute_create_issue();
        let issues: Vec<&Issue> = app.all_issues.iter().collect();
        let csv = issues_csv(&issues);
        assert!(csv.starts_with('\u{feff}'));
        let lines: Vec<&str> = csv.trim_start_matches('\u{feff}').split("\r\n").collect();
        assert!(lines[0].starts_with("key;title;status"));
        assert!(
            lines[1].starts_with("TC-1;\"Fix \"\"quoted\"\"; export\";"),
            "{}",
            lines[1]
        );
        assert!(lines[1].contains(";bug,export;"), "{}", lines[1]);
        assert_eq!(lines.len(), 3, "header, one row, trailing empty");
    }
}
