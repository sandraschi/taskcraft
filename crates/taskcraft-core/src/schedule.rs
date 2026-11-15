//! Critical-path scheduler (Tier 7, `docs/PM_SPEC.md` sections 2-3).
//!
//! Pure functions: no store, no I/O. Works on working-day indices, then maps
//! indices to dates on a working calendar.
//!
//! Conventions (PM_SPEC section 3, Tier 7):
//! - Day indices count working days from the project start. A task with
//!   ES 0 and duration 3 occupies days 0, 1, 2; its EF is 3 (exclusive).
//! - Finish-to-start lag 0: the successor starts on day `EF` of the
//!   predecessor. Lags are in working days and may be negative.
//! - Float = LS - ES. Critical = float <= 0.
//! - Manually scheduled tasks keep their start; constraints they break are
//!   reported as violations, never fixed silently.

use chrono::NaiveDate;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Caller's task id (an issue id in taskcraft).
pub type TaskId = u64;

/// Dependency type, matching the relation kinds `blocks` (FS),
/// `starts_with` (SS) and `finishes_with` (FF).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepKind {
    /// Successor starts after the predecessor finishes.
    FinishToStart,
    /// Successor starts no earlier than the predecessor starts.
    StartToStart,
    /// Successor finishes no earlier than the predecessor finishes.
    FinishToFinish,
}

/// One task to schedule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    /// Id.
    pub id: TaskId,
    /// Working days; 0 is a milestone.
    pub duration: u32,
    /// Fixed start (working-day index) for manually scheduled tasks.
    pub manual_start: Option<i64>,
}

impl Task {
    /// Automatically scheduled task.
    #[must_use]
    pub fn auto(id: TaskId, duration: u32) -> Self {
        Self {
            id,
            duration,
            manual_start: None,
        }
    }

    /// Manually scheduled task fixed at a working-day index.
    #[must_use]
    pub fn manual(id: TaskId, duration: u32, start: i64) -> Self {
        Self {
            id,
            duration,
            manual_start: Some(start),
        }
    }
}

/// A dependency between two tasks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dep {
    /// Predecessor.
    pub from: TaskId,
    /// Successor.
    pub to: TaskId,
    /// Type.
    pub kind: DepKind,
    /// Lag in working days.
    pub lag: i32,
}

impl Dep {
    /// Dependency of any kind with a lag.
    #[must_use]
    pub fn new(from: TaskId, to: TaskId, kind: DepKind, lag: i32) -> Self {
        Self {
            from,
            to,
            kind,
            lag,
        }
    }

    /// Finish-to-start without lag.
    #[must_use]
    pub fn fs(from: TaskId, to: TaskId) -> Self {
        Self::new(from, to, DepKind::FinishToStart, 0)
    }
}

/// Computed times of one task, in working-day indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// Earliest start.
    pub es: i64,
    /// Earliest finish (exclusive).
    pub ef: i64,
    /// Latest start.
    pub ls: i64,
    /// Latest finish (exclusive).
    pub lf: i64,
}

impl Slot {
    /// Total float in working days.
    #[must_use]
    pub fn float(&self) -> i64 {
        self.ls - self.es
    }

    /// On the critical path.
    #[must_use]
    pub fn is_critical(&self) -> bool {
        self.float() <= 0
    }
}

/// A dependency a manually scheduled task does not honour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    /// The manual task.
    pub task: TaskId,
    /// Index into the `deps` slice passed to [`schedule`].
    pub dep: usize,
    /// How many working days too early it is.
    pub by_days: i64,
}

/// Result of [`schedule`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schedule {
    /// Times per task.
    pub slots: BTreeMap<TaskId, Slot>,
    /// Project end (largest EF).
    pub end: i64,
    /// Critical tasks in dependency order.
    pub critical: Vec<TaskId>,
    /// Constraints broken by manual tasks.
    pub violations: Vec<Violation>,
}

/// Why a schedule could not be computed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScheduleError {
    /// Two tasks share an id.
    DuplicateTask(TaskId),
    /// A dependency names a task that is not in the list.
    UnknownTask(TaskId),
    /// A task depends on itself.
    SelfDependency(TaskId),
    /// The dependencies (all kinds together) contain a cycle through these
    /// tasks (sorted).
    Cycle(Vec<TaskId>),
}

impl std::fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScheduleError::DuplicateTask(id) => write!(f, "task {id} is listed twice"),
            ScheduleError::UnknownTask(id) => write!(f, "dependency names unknown task {id}"),
            ScheduleError::SelfDependency(id) => write!(f, "task {id} depends on itself"),
            ScheduleError::Cycle(ids) => write!(f, "dependency cycle through tasks {ids:?}"),
        }
    }
}

impl std::error::Error for ScheduleError {}

/// Earliest start the dependency `d` allows for its successor.
fn required_start(d: &Dep, pred: &Slot, succ_duration: i64) -> i64 {
    let lag = i64::from(d.lag);
    match d.kind {
        DepKind::FinishToStart => pred.ef + lag,
        DepKind::StartToStart => pred.es + lag,
        DepKind::FinishToFinish => pred.ef + lag - succ_duration,
    }
}

/// Latest finish the dependency `d` allows for its predecessor.
fn allowed_finish(d: &Dep, succ: &Slot, pred_duration: i64) -> i64 {
    let lag = i64::from(d.lag);
    match d.kind {
        DepKind::FinishToStart => succ.ls - lag,
        DepKind::StartToStart => succ.ls - lag + pred_duration,
        DepKind::FinishToFinish => succ.lf - lag,
    }
}

/// Topological order of task indices (Kahn, stable by input order).
fn topo_order(
    tasks: &[Task],
    deps: &[Dep],
    index: &HashMap<TaskId, usize>,
) -> Result<Vec<usize>, ScheduleError> {
    let n = tasks.len();
    let mut indegree = vec![0usize; n];
    let mut succs: Vec<Vec<usize>> = vec![Vec::new(); n];
    for d in deps {
        let (Some(&a), Some(&b)) = (index.get(&d.from), index.get(&d.to)) else {
            continue;
        };
        succs[a].push(b);
        indegree[b] += 1;
    }
    let mut ready: BTreeSet<usize> = (0..n).filter(|&i| indegree[i] == 0).collect();
    let mut order = Vec::with_capacity(n);
    while let Some(i) = ready.pop_first() {
        order.push(i);
        for &s in &succs[i] {
            indegree[s] -= 1;
            if indegree[s] == 0 {
                ready.insert(s);
            }
        }
    }
    if order.len() < n {
        let mut ids: Vec<TaskId> = (0..n)
            .filter(|&i| indegree[i] > 0)
            .map(|i| tasks[i].id)
            .collect();
        ids.sort_unstable();
        return Err(ScheduleError::Cycle(ids));
    }
    Ok(order)
}

/// Forward and backward pass. Deterministic for a given input.
pub fn schedule(tasks: &[Task], deps: &[Dep]) -> Result<Schedule, ScheduleError> {
    let mut index = HashMap::with_capacity(tasks.len());
    for (i, t) in tasks.iter().enumerate() {
        if index.insert(t.id, i).is_some() {
            return Err(ScheduleError::DuplicateTask(t.id));
        }
    }
    for d in deps {
        if d.from == d.to {
            return Err(ScheduleError::SelfDependency(d.from));
        }
        for id in [d.from, d.to] {
            if !index.contains_key(&id) {
                return Err(ScheduleError::UnknownTask(id));
            }
        }
    }
    let order = topo_order(tasks, deps, &index)?;

    let mut incoming: Vec<Vec<usize>> = vec![Vec::new(); tasks.len()];
    let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); tasks.len()];
    for (k, d) in deps.iter().enumerate() {
        if let (Some(&a), Some(&b)) = (index.get(&d.from), index.get(&d.to)) {
            outgoing[a].push(k);
            incoming[b].push(k);
        }
    }

    // Forward pass.
    let mut slots: Vec<Slot> = vec![
        Slot {
            es: 0,
            ef: 0,
            ls: 0,
            lf: 0
        };
        tasks.len()
    ];
    let mut violations = Vec::new();
    for &i in &order {
        let dur = i64::from(tasks[i].duration);
        let mut earliest = 0i64;
        let mut per_dep = Vec::new();
        for &k in &incoming[i] {
            let d = &deps[k];
            let Some(&p) = index.get(&d.from) else {
                continue;
            };
            let req = required_start(d, &slots[p], dur);
            per_dep.push((k, req));
            earliest = earliest.max(req);
        }
        let es = match tasks[i].manual_start {
            Some(fixed) => {
                for (k, req) in per_dep {
                    if fixed < req {
                        violations.push(Violation {
                            task: tasks[i].id,
                            dep: k,
                            by_days: req - fixed,
                        });
                    }
                }
                fixed
            }
            None => earliest,
        };
        slots[i].es = es;
        slots[i].ef = es + dur;
    }
    let end = slots.iter().map(|s| s.ef).max().unwrap_or(0);

    // Backward pass.
    for &i in order.iter().rev() {
        let dur = i64::from(tasks[i].duration);
        let mut lf = end;
        for &k in &outgoing[i] {
            let d = &deps[k];
            let Some(&s) = index.get(&d.to) else { continue };
            lf = lf.min(allowed_finish(d, &slots[s], dur));
        }
        slots[i].lf = lf;
        slots[i].ls = lf - dur;
    }

    let critical = order
        .iter()
        .filter(|&&i| slots[i].is_critical())
        .map(|&i| tasks[i].id)
        .collect();
    Ok(Schedule {
        slots: tasks.iter().zip(slots).map(|(t, s)| (t.id, s)).collect(),
        end,
        critical,
        violations,
    })
}

/// Duration in working days for a task, first match wins (PM_SPEC v4):
/// explicit days; else `ceil(estimate_minutes / daily_minutes)` when both are
/// known; else 1 day. The flag is true when the 1-day default was used.
#[must_use]
pub fn duration_for(
    duration_days: Option<u32>,
    estimate_minutes: Option<u32>,
    daily_minutes: Option<u32>,
) -> (u32, bool) {
    if let Some(d) = duration_days {
        return (d, false);
    }
    match (estimate_minutes, daily_minutes) {
        (Some(est), Some(daily)) if daily > 0 => (est.div_ceil(daily).max(1), false),
        _ => (1, true),
    }
}

/// Upper bound on calendar days scanned when mapping indices to dates.
const MAX_SCAN_DAYS: u32 = 20_000;

/// Which days are working days.
pub trait WorkCalendar {
    /// True if work happens on `date`.
    fn is_working(&self, date: NaiveDate) -> bool;
}

/// Austrian calendar: Monday to Friday minus the national public holidays
/// (`craft_biz::holidays`), plus company closures and extra working days.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AtCalendar {
    /// Days the company is closed.
    pub closures: BTreeSet<NaiveDate>,
    /// Days worked although they are weekends or holidays.
    pub extra_working: BTreeSet<NaiveDate>,
}

impl WorkCalendar for AtCalendar {
    fn is_working(&self, date: NaiveDate) -> bool {
        if self.extra_working.contains(&date) {
            return true;
        }
        if self.closures.contains(&date) {
            return false;
        }
        craft_biz::holidays::is_working_day(date).unwrap_or(false)
    }
}

/// Maps working-day indices to dates from a project start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DayMap {
    days: Vec<NaiveDate>,
}

impl DayMap {
    /// Working days from the first working day on or after `start`, enough
    /// to cover index `last`. `None` if the calendar has no working days in
    /// the scan window.
    pub fn new(cal: &impl WorkCalendar, start: NaiveDate, last: i64) -> Option<Self> {
        let want = usize::try_from(last.max(0)).ok()? + 1;
        let mut days = Vec::with_capacity(want);
        let mut d = start;
        let mut scanned = 0u32;
        while days.len() < want {
            if cal.is_working(d) {
                days.push(d);
            }
            d = d.succ_opt()?;
            scanned += 1;
            if scanned > MAX_SCAN_DAYS {
                return None;
            }
        }
        Some(Self { days })
    }

    /// Date of a working-day index.
    #[must_use]
    pub fn date(&self, index: i64) -> Option<NaiveDate> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.days.get(i))
            .copied()
    }

    /// First and last day (inclusive) of a slot. A milestone (duration 0)
    /// gets its start day for both.
    #[must_use]
    pub fn span(&self, slot: &Slot) -> Option<(NaiveDate, NaiveDate)> {
        let start = self.date(slot.es)?;
        let finish = if slot.ef > slot.es {
            self.date(slot.ef - 1)?
        } else {
            start
        };
        Some((start, finish))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap_or_default()
    }

    fn slot(s: &Schedule, id: TaskId) -> Slot {
        s.slots.get(&id).copied().unwrap_or(Slot {
            es: -1,
            ef: -1,
            ls: -1,
            lf: -1,
        })
    }

    /// (es, ef, ls, lf, float)
    fn row(s: &Schedule, id: TaskId) -> (i64, i64, i64, i64, i64) {
        let x = slot(s, id);
        (x.es, x.ef, x.ls, x.lf, x.float())
    }

    // Ids: A=1 B=2 C=3 D=4 E=5.
    fn fixture_a() -> (Vec<Task>, Vec<Dep>) {
        (
            vec![
                Task::auto(1, 3),
                Task::auto(2, 2),
                Task::auto(3, 4),
                Task::auto(4, 2),
                Task::auto(5, 3),
            ],
            vec![
                Dep::fs(1, 2),
                Dep::fs(1, 3),
                Dep::fs(2, 4),
                Dep::fs(3, 4),
                Dep::fs(4, 5),
            ],
        )
    }

    #[test]
    fn fixture_a_finish_to_start_network() {
        let (tasks, deps) = fixture_a();
        let s = schedule(&tasks, &deps).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(row(&s, 1), (0, 3, 0, 3, 0));
        assert_eq!(row(&s, 2), (3, 5, 5, 7, 2));
        assert_eq!(row(&s, 3), (3, 7, 3, 7, 0));
        assert_eq!(row(&s, 4), (7, 9, 7, 9, 0));
        assert_eq!(row(&s, 5), (9, 12, 9, 12, 0));
        assert_eq!(s.end, 12);
        assert_eq!(s.critical, vec![1, 3, 4, 5]);
        assert!(s.violations.is_empty());
    }

    #[test]
    fn fixture_b_mixed_links_with_lag() {
        let tasks = vec![
            Task::auto(1, 4),
            Task::auto(2, 3),
            Task::auto(3, 2),
            Task::auto(4, 2),
        ];
        let deps = vec![
            Dep::new(1, 2, DepKind::StartToStart, 1),
            Dep::new(1, 3, DepKind::FinishToFinish, 0),
            Dep::new(2, 4, DepKind::FinishToStart, 2),
            Dep::new(3, 4, DepKind::FinishToStart, 0),
        ];
        let s = schedule(&tasks, &deps).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(row(&s, 1), (0, 4, 0, 4, 0));
        assert_eq!(row(&s, 2), (1, 4, 1, 4, 0));
        assert_eq!(row(&s, 3), (2, 4, 4, 6, 2));
        assert_eq!(row(&s, 4), (6, 8, 6, 8, 0));
        assert_eq!(s.end, 8);
        assert_eq!(s.critical, vec![1, 2, 4]);
    }

    #[test]
    fn fixture_c_dates_on_the_austrian_calendar() {
        let tasks = vec![Task::auto(1, 3), Task::auto(2, 2), Task::auto(3, 1)];
        let deps = vec![Dep::fs(1, 2), Dep::fs(2, 3)];
        let s = schedule(&tasks, &deps).unwrap_or_else(|e| unreachable!("{e}"));

        let open = AtCalendar::default();
        let map = DayMap::new(&open, d(2026, 12, 23), s.end).unwrap_or_else(|| unreachable!());
        assert_eq!(
            map.span(&slot(&s, 1)),
            Some((d(2026, 12, 23), d(2026, 12, 28)))
        );
        assert_eq!(
            map.span(&slot(&s, 2)),
            Some((d(2026, 12, 29), d(2026, 12, 30)))
        );
        assert_eq!(
            map.span(&slot(&s, 3)),
            Some((d(2026, 12, 31), d(2026, 12, 31)))
        );

        let closed = AtCalendar {
            closures: BTreeSet::from([d(2026, 12, 24)]),
            ..AtCalendar::default()
        };
        let map = DayMap::new(&closed, d(2026, 12, 23), s.end).unwrap_or_else(|| unreachable!());
        assert_eq!(
            map.span(&slot(&s, 1)),
            Some((d(2026, 12, 23), d(2026, 12, 29)))
        );
        assert_eq!(
            map.span(&slot(&s, 2)),
            Some((d(2026, 12, 30), d(2026, 12, 31)))
        );
        assert_eq!(map.span(&slot(&s, 3)), Some((d(2027, 1, 4), d(2027, 1, 4))));
    }

    #[test]
    fn fixture_d_cycle_is_refused_across_kinds() {
        let (tasks, mut deps) = fixture_a();
        deps.push(Dep::new(3, 1, DepKind::StartToStart, 0));
        assert_eq!(
            schedule(&tasks, &deps),
            Err(ScheduleError::Cycle(vec![1, 2, 3, 4, 5]))
        );
        let (tasks, _) = fixture_a();
        let ff_cycle = vec![
            Dep::new(1, 2, DepKind::FinishToFinish, 0),
            Dep::new(2, 1, DepKind::FinishToStart, 0),
        ];
        assert_eq!(
            schedule(&tasks, &ff_cycle),
            Err(ScheduleError::Cycle(vec![1, 2]))
        );
    }

    #[test]
    fn fixture_d_manual_task_reports_violation() {
        let (mut tasks, deps) = fixture_a();
        tasks[1] = Task::manual(2, 2, 1);
        let s = schedule(&tasks, &deps).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(slot(&s, 2).es, 1, "manual start is kept");
        assert_eq!(
            s.violations,
            vec![Violation {
                task: 2,
                dep: 0,
                by_days: 2
            }]
        );
        // The rest of the network still follows A and C.
        assert_eq!(row(&s, 4), (7, 9, 7, 9, 0));
    }

    #[test]
    fn baseline_slip_follows_the_spec() {
        // C from 4 to 6 days: C, D, E and the end slip by 2.
        let (mut tasks, deps) = fixture_a();
        let before = schedule(&tasks, &deps).unwrap_or_else(|e| unreachable!("{e}"));
        tasks[2].duration = 6;
        let after = schedule(&tasks, &deps).unwrap_or_else(|e| unreachable!("{e}"));
        for id in [4, 5] {
            assert_eq!(slot(&after, id).es - slot(&before, id).es, 2);
        }
        assert_eq!(slot(&after, 3).ef - slot(&before, 3).ef, 2);
        assert_eq!(after.end - before.end, 2);
        // B gains float: 2 before, 4 after.
        assert_eq!(slot(&after, 2).float(), 4);
    }

    #[test]
    fn milestones_and_input_errors() {
        let tasks = vec![Task::auto(1, 3), Task::auto(2, 0)];
        let s = schedule(&tasks, &[Dep::new(1, 2, DepKind::FinishToFinish, 0)])
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(row(&s, 2), (3, 3, 3, 3, 0));

        assert_eq!(
            schedule(&[Task::auto(1, 1), Task::auto(1, 2)], &[]),
            Err(ScheduleError::DuplicateTask(1))
        );
        assert_eq!(
            schedule(&tasks, &[Dep::fs(1, 9)]),
            Err(ScheduleError::UnknownTask(9))
        );
        assert_eq!(
            schedule(&tasks, &[Dep::fs(1, 1)]),
            Err(ScheduleError::SelfDependency(1))
        );
        let empty = schedule(&[], &[]).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(empty.end, 0);
        assert!(empty.critical.is_empty());
    }

    #[test]
    fn negative_lag_overlaps() {
        // B may start 1 day before A finishes.
        let tasks = vec![Task::auto(1, 3), Task::auto(2, 2)];
        let s = schedule(&tasks, &[Dep::new(1, 2, DepKind::FinishToStart, -1)])
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(slot(&s, 2).es, 2);
        assert_eq!(s.end, 4);
    }

    #[test]
    fn duration_rules() {
        assert_eq!(duration_for(Some(5), Some(60), Some(480)), (5, false));
        assert_eq!(duration_for(None, Some(1000), Some(480)), (3, false));
        assert_eq!(duration_for(None, Some(0), Some(480)), (1, false));
        assert_eq!(duration_for(None, Some(600), None), (1, true));
        assert_eq!(duration_for(None, None, Some(480)), (1, true));
        assert_eq!(duration_for(None, Some(600), Some(0)), (1, true));
    }

    #[test]
    fn day_map_bounds() {
        let cal = AtCalendar::default();
        let map = DayMap::new(&cal, d(2026, 11, 7), 4).unwrap_or_else(|| unreachable!());
        // Saturday start: index 0 is Monday 9.
        assert_eq!(map.date(0), Some(d(2026, 11, 9)));
        assert_eq!(map.date(4), Some(d(2026, 11, 13)));
        assert_eq!(map.date(5), None);
        assert_eq!(map.date(-1), None);
    }
}
