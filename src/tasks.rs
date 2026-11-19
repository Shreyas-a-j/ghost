use crate::events;
use crate::persist::{self, FileLock};
use crate::project;
use anyhow::{ensure, Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const TASK_SCHEMA_VERSION: u32 = 1;
pub const TASKS_DIR: &str = ".ghost/tasks";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Backlog,
    Ready,
    InProgress,
    Verification,
    Done,
    Blocked,
    Cancelled,
}

impl TaskStatus {
    pub const ALL: [Self; 7] = [
        Self::Backlog,
        Self::Ready,
        Self::InProgress,
        Self::Verification,
        Self::Done,
        Self::Blocked,
        Self::Cancelled,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Backlog => "backlog",
            Self::Ready => "ready",
            Self::InProgress => "in_progress",
            Self::Verification => "verification",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        let value = value.trim().to_ascii_lowercase();
        Self::ALL
            .iter()
            .copied()
            .find(|status| status.as_str() == value)
            .with_context(|| {
                format!(
                    "unknown task status {value:?}; expected one of: {}",
                    Self::ALL
                        .iter()
                        .map(|status| status.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
    }

    /// The documented state machine. `cancelled` and `done` are terminal.
    pub fn can_transition_to(self, next: Self) -> bool {
        use TaskStatus::*;
        matches!(
            (self, next),
            (Backlog, Ready)
                | (Backlog, InProgress)
                | (Backlog, Cancelled)
                | (Ready, InProgress)
                | (Ready, Blocked)
                | (Ready, Cancelled)
                | (InProgress, Verification)
                | (InProgress, Blocked)
                | (InProgress, Cancelled)
                | (Verification, Done)
                | (Verification, InProgress)
                | (Verification, Blocked)
                | (Verification, Cancelled)
                | (Blocked, Ready)
                | (Blocked, InProgress)
                | (Blocked, Cancelled)
        )
    }

    pub fn active(self) -> bool {
        !matches!(self, Self::Done | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<String>,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(default)]
    pub acceptance_checks: Vec<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
}

impl Task {
    pub fn transition_to(&mut self, next: TaskStatus) -> Result<()> {
        ensure!(
            self.status.can_transition_to(next),
            "cannot move {} from {} to {}",
            self.id,
            self.status.as_str(),
            next.as_str()
        );
        self.status = next;
        self.updated_at = Local::now();
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        ensure_id(&self.id)?;
        ensure!(
            !self.title.trim().is_empty() && self.title.chars().count() <= 200,
            "task title must be 1-200 characters"
        );
        for reference in &self.references {
            persist::validate_relative(Path::new(reference))
                .with_context(|| format!("invalid task reference {reference:?}"))?;
        }
        ensure!(
            self.acceptance_checks
                .iter()
                .all(|check| !check.trim().is_empty()),
            "acceptance checks must not be empty"
        );
        Ok(())
    }
}

pub fn tasks_dir(root: &Path) -> PathBuf {
    root.join(TASKS_DIR)
}

fn ensure_id(id: &str) -> Result<()> {
    let digits = id
        .strip_prefix("TASK-")
        .with_context(|| format!("invalid task id {id:?}; expected TASK-0001"))?;
    ensure!(
        digits.len() >= 4 && digits.chars().all(|ch| ch.is_ascii_digit()),
        "invalid task id {id:?}; expected TASK-0001"
    );
    Ok(())
}

fn task_path(id: &str) -> PathBuf {
    Path::new(TASKS_DIR).join(format!("{id}.json"))
}

pub fn list(root: &Path) -> Result<Vec<Task>> {
    let directory = tasks_dir(root);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err).with_context(|| format!("could not read {}", directory.display()))
        }
    };
    let mut tasks = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("could not read {}", path.display()))?;
        let task: Task = serde_json::from_str(&raw)
            .with_context(|| format!("could not parse {}", path.display()))?;
        task.validate()
            .with_context(|| format!("invalid task {}", path.display()))?;
        tasks.push(task);
    }
    tasks.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(tasks)
}

pub fn load(root: &Path, id: &str) -> Result<Task> {
    ensure_id(id)?;
    let path = root.join(task_path(id));
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("could not read {}; run `ghost task list`", path.display()))?;
    let task: Task = serde_json::from_str(&raw)
        .with_context(|| format!("could not parse {}", path.display()))?;
    task.validate()?;
    Ok(task)
}

pub fn save(root: &Path, task: &Task, lock: &FileLock) -> Result<()> {
    let _ = lock;
    task.validate()?;
    let relative = task_path(&task.id);
    let target = persist::safe_target(root, &relative)?;
    let raw = serde_json::to_string_pretty(task).context("could not serialize task")?;
    persist::atomic_write(&target, format!("{raw}\n").as_bytes(), false)
        .with_context(|| format!("could not write {}", target.display()))
}

pub fn next_id(root: &Path) -> Result<String> {
    let highest = list(root)?
        .iter()
        .filter_map(|task| task.id.strip_prefix("TASK-"))
        .filter_map(|digits| digits.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    Ok(format!("TASK-{:04}", highest + 1))
}

pub fn add(
    root: &Path,
    title: &str,
    owner: Option<&str>,
    references: &[String],
    checks: &[String],
    lock: &FileLock,
) -> Result<Task> {
    let id = next_id(root)?;
    let now = Local::now();
    let task = Task {
        schema_version: TASK_SCHEMA_VERSION,
        id,
        title: events::redact(title),
        status: TaskStatus::Backlog,
        owner_agent: owner.map(events::redact).filter(|owner| !owner.is_empty()),
        base_revision: None,
        references: references.to_vec(),
        acceptance_checks: checks.to_vec(),
        evidence_ids: Vec::new(),
        created_at: now,
        updated_at: now,
    };
    save(root, &task, lock)?;
    Ok(task)
}

/// Activate a task. Re-running on an active task is idempotent and updates the
/// owner, so two agents can pick up the same record without creating a duplicate.
pub fn start(
    root: &Path,
    id: &str,
    owner: Option<&str>,
    lock: &FileLock,
) -> Result<(Task, Option<TaskStatus>)> {
    let mut task = load(root, id)?;
    let previous = if task.status == TaskStatus::InProgress {
        None
    } else {
        let previous = task.status;
        task.transition_to(TaskStatus::InProgress)?;
        Some(previous)
    };
    if let Some(owner) = owner {
        let owner = events::redact(owner);
        if !owner.is_empty() {
            task.owner_agent = Some(owner);
        }
    }
    if task.base_revision.is_none() {
        task.base_revision = project::discover(root)
            .ok()
            .and_then(|facts| facts.git)
            .and_then(|git| git.revision);
    }
    task.updated_at = Local::now();
    save(root, &task, lock)?;
    Ok((task, previous))
}

pub fn transition(
    root: &Path,
    id: &str,
    next: TaskStatus,
    lock: &FileLock,
) -> Result<(Task, TaskStatus)> {
    let mut task = load(root, id)?;
    let previous = task.status;
    task.transition_to(next)?;
    if next == TaskStatus::InProgress && task.base_revision.is_none() {
        task.base_revision = project::discover(root)
            .ok()
            .and_then(|facts| facts.git)
            .and_then(|git| git.revision);
    }
    save(root, &task, lock)?;
    Ok((task, previous))
}

/// One compact line for `ghost status`; `None` when the project has no tasks.
pub fn dashboard(root: &Path) -> Result<Option<String>> {
    let tasks = list(root)?;
    if tasks.is_empty() {
        return Ok(None);
    }
    let active = tasks.iter().filter(|task| task.status.active()).count();
    let blocked = tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Blocked)
        .count();
    let done = tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Done)
        .count();
    let current = [
        TaskStatus::InProgress,
        TaskStatus::Verification,
        TaskStatus::Ready,
    ]
    .iter()
    .find_map(|status| tasks.iter().find(|task| task.status == *status));
    let mut line = format!("Tasks:  {active} active, {blocked} blocked, {done} done");
    if let Some(task) = current {
        let title: String = task.title.chars().take(40).collect();
        line.push_str(&format!(
            " ({task_id} {status}: {title})",
            task_id = task.id,
            status = task.status.as_str(),
        ));
    }
    Ok(Some(line))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock(root: &Path) -> FileLock {
        FileLock::acquire(root).unwrap()
    }

    #[test]
    fn documented_transitions_are_enforced() {
        use TaskStatus::*;
        assert!(Backlog.can_transition_to(Ready));
        assert!(Ready.can_transition_to(InProgress));
        assert!(InProgress.can_transition_to(Verification));
        assert!(Verification.can_transition_to(Done));
        assert!(Verification.can_transition_to(InProgress));
        assert!(Blocked.can_transition_to(Ready));
        assert!(!Done.can_transition_to(InProgress));
        assert!(!Cancelled.can_transition_to(Backlog));
        assert!(!Backlog.can_transition_to(Done));
        assert!(!InProgress.can_transition_to(Ready));
    }

    #[test]
    fn add_allocates_sequential_ids_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let lock = lock(dir.path());
        let first = add(dir.path(), "First task", None, &[], &[], &lock).unwrap();
        let second = add(
            dir.path(),
            "Second task",
            Some("claude"),
            &["src/main.rs".to_string()],
            &["cargo test".to_string()],
            &lock,
        )
        .unwrap();
        assert_eq!(first.id, "TASK-0001");
        assert_eq!(second.id, "TASK-0002");
        assert_eq!(second.status, TaskStatus::Backlog);
        assert_eq!(list(dir.path()).unwrap().len(), 2);
        assert_eq!(
            load(dir.path(), "TASK-0002")
                .unwrap()
                .owner_agent
                .as_deref(),
            Some("claude")
        );
        assert!(load(dir.path(), "../escape").is_err());
    }

    #[test]
    fn start_and_transition_follow_the_state_machine() {
        let dir = tempfile::tempdir().unwrap();
        let lock = lock(dir.path());
        add(dir.path(), "Work", None, &[], &[], &lock).unwrap();
        let (task, previous) = start(dir.path(), "TASK-0001", Some("opencode"), &lock).unwrap();
        assert_eq!(previous, Some(TaskStatus::Backlog));
        assert_eq!(task.status, TaskStatus::InProgress);
        assert_eq!(task.owner_agent.as_deref(), Some("opencode"));

        let (task, _) =
            transition(dir.path(), "TASK-0001", TaskStatus::Verification, &lock).unwrap();
        assert_eq!(task.status, TaskStatus::Verification);
        assert!(transition(dir.path(), "TASK-0001", TaskStatus::Backlog, &lock).is_err());
        let (task, _) = transition(dir.path(), "TASK-0001", TaskStatus::Done, &lock).unwrap();
        assert_eq!(task.status, TaskStatus::Done);
    }

    #[test]
    fn dashboard_prefers_in_progress() {
        let dir = tempfile::tempdir().unwrap();
        let lock = lock(dir.path());
        add(dir.path(), "Ready", None, &[], &[], &lock).unwrap();
        add(dir.path(), "Working", None, &[], &[], &lock).unwrap();
        start(dir.path(), "TASK-0002", None, &lock).unwrap();
        let line = dashboard(dir.path()).unwrap().unwrap();
        assert!(line.contains("2 active"), "{line}");
        assert!(line.contains("TASK-0002"), "{line}");
    }
}
