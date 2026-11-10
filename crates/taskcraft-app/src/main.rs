//! `taskcraft-app`: Native eframe/egui desktop workstation for TaskCraft.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use craft_biz::bus::Bus;
use craft_biz_ui::kanban::{Board, Card, Column, Event};
use craft_biz_ui::Tone;
use eframe::egui::{self, Color32, RichText};
use std::path::PathBuf;
use taskcraft_core::{dispatch, Command, Confinement, Ctx};
use taskcraft_store::{IssueFilter, Priority, Project, Status, Tracker};

struct App {
    ctx: Ctx,
    projects: Vec<Project>,
    selected_project: Option<String>,
    cards: Vec<Card>,
    selected_issue_key: Option<String>,
    selected_issue_details: Option<serde_json::Value>,
    status_msg: String,
}

impl App {
    fn new(ctx: Ctx) -> Self {
        let mut app = Self {
            ctx,
            projects: Vec::new(),
            selected_project: None,
            cards: Vec::new(),
            selected_issue_key: None,
            selected_issue_details: None,
            status_msg: "Ready".to_string(),
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
        self.reload_board();
    }

    fn reload_board(&mut self) {
        let Some(ref p_key) = self.selected_project else {
            self.cards.clear();
            return;
        };

        if let Ok(res) = self.ctx.tracker.list(&IssueFilter {
            project: Some(p_key.clone()),
            include_finished: true,
            limit: 1000,
            ..Default::default()
        }) {
            let mut cards = Vec::new();
            for i in res {
                let col = i.status.as_str().to_string();
                let accent = match i.priority {
                    Priority::Urgent => Tone::Danger,
                    Priority::High => Tone::Warn,
                    Priority::Medium => Tone::Info,
                    Priority::Low => Tone::Neutral,
                };
                let badge = Some((i.priority.as_str().to_uppercase(), accent));

                let mut meta = Vec::new();
                if let Some(est) = i.estimate {
                    meta.push(format!("{est} pts"));
                }
                if let Some(due) = i.due_date {
                    meta.push(format!("Due {due}"));
                }

                let blockers = self.ctx.tracker.open_blockers(i.id).unwrap_or_default();
                let blocked = if !blockers.is_empty() {
                    Some(format!("Blocked by {} issue(s)", blockers.len()))
                } else {
                    None
                };

                let tags = i.labels.into_iter().map(|l| (l, Tone::Info)).collect();

                cards.push(Card {
                    id: i.key.clone(),
                    column: col,
                    lane: String::new(),
                    key: i.key.clone(),
                    title: i.title,
                    tags,
                    accent,
                    badge,
                    assignee: i.assignee,
                    meta,
                    blocked,
                    done: i.status.is_finished(),
                });
            }
            self.cards = cards;
        }

        if let Some(ref key) = self.selected_issue_key {
            let k = key.clone();
            self.load_issue_details(&k);
        }
    }

    fn load_issue_details(&mut self, key: &str) {
        if let Ok(res) = dispatch(
            &mut self.ctx,
            Command::IssueShow {
                issue: key.to_string(),
            },
        ) {
            self.selected_issue_details = Some(res);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.horizontal(|ui| {
            ui.heading(
                RichText::new("🎯 TaskCraft Kanban")
                    .color(Color32::from_rgb(56, 189, 248))
                    .strong(),
            );
            ui.separator();

            ui.label("Project:");
            let mut switch_to = None;
            for p in &self.projects {
                let is_sel = self.selected_project.as_deref() == Some(&p.key);
                if ui.selectable_label(is_sel, &p.name).clicked() {
                    switch_to = Some(p.key.clone());
                }
            }
            if let Some(new_p) = switch_to {
                self.selected_project = Some(new_p);
                self.reload_board();
            }

            ui.separator();
            if ui.button("🔄 Refresh").clicked() {
                self.reload_board();
                self.status_msg = "Board refreshed".to_string();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("Actor: {}", self.ctx.actor));
            });
        });

        ui.separator();

        let columns = [
            Column::new("backlog", "Backlog", Tone::Neutral),
            Column::new("todo", "To Do", Tone::Info),
            Column::new("in_progress", "In Progress", Tone::Accent),
            Column::new("in_review", "In Review", Tone::Warn),
            Column::new("done", "Done", Tone::Ok),
        ];

        let rule: &dyn Fn(&Card, &str) -> Result<(), String> = &|card, target_col| {
            if card.blocked.is_some() && target_col == "in_progress" {
                return Err("Cannot move blocked card to In Progress".to_string());
            }
            Ok(())
        };

        let evs = Board::new("taskcraft_board", &columns, &self.cards)
            .rule(rule)
            .selected(self.selected_issue_key.as_deref())
            .show(ui);

        let mut reload_needed = false;
        for ev in evs {
            match ev {
                Event::Moved { card, to, .. } => {
                    if let Ok(st) = Status::parse(&to) {
                        match dispatch(
                            &mut self.ctx,
                            Command::IssueMove {
                                issue: card.clone(),
                                status: st,
                                force: false,
                            },
                        ) {
                            Ok(_) => {
                                self.status_msg = format!("Moved {card} -> {to}");
                                reload_needed = true;
                            }
                            Err(e) => {
                                self.status_msg = format!("Refused move: {e}");
                            }
                        }
                    }
                }
                Event::Refused { card, to, reason } => {
                    self.status_msg = format!("Move refused for {card} to {to}: {reason}");
                }
                Event::Select(id) => {
                    self.selected_issue_key = Some(id.clone());
                    self.load_issue_details(&id);
                }
                _ => {}
            }
        }

        if reload_needed {
            self.reload_board();
        }

        // Details drawer if selected
        if let Some(ref det) = self.selected_issue_details {
            ui.separator();
            ui.horizontal(|ui| {
                let title = det
                    .pointer("/issue/title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Issue Details");
                let key = det
                    .pointer("/issue/key")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let status = det
                    .pointer("/issue/status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let priority = det
                    .pointer("/issue/priority")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let desc = det
                    .pointer("/issue/description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                ui.vertical(|ui| {
                    ui.heading(format!("{key}: {title}"));
                    ui.label(format!("Status: {status} | Priority: {priority}"));
                    if !desc.is_empty() {
                        ui.label(RichText::new(desc).italics());
                    }
                });
            });
        }

        ui.separator();
        ui.horizontal(|ui| {
            ui.label(format!(
                "Total Cards: {} | Selected: {}",
                self.cards.len(),
                self.selected_issue_key.as_deref().unwrap_or("-")
            ));
            ui.separator();
            ui.label(&self.status_msg);
        });
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

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--smoke") {
        println!("smoke: ok (taskcraft-app)");
        return Ok(());
    }

    let db_path = default_db();
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let tracker = match Tracker::open(&db_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to open tracker database at {}: {e}", db_path.display());
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
            .with_inner_size([1200.0, 760.0])
            .with_min_inner_size([800.0, 500.0])
            .with_title("TaskCraft Kanban"),
        ..Default::default()
    };

    eframe::run_native(
        "TaskCraft",
        native_options,
        Box::new(|_cc| Ok(Box::new(App::new(ctx)))),
    )
}
