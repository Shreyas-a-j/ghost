use crate::events::{self, Event};
use crate::persist::{self, FileLock};
use crate::project;
use crate::state::WorkflowState;
use crate::tasks::{self, TaskStatus};
use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub const HANDOFF_SCHEMA_VERSION: u32 = 1;
pub const HANDOFF_PATH: &str = ".ghost/handoff.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSnapshot {
    pub id: String,
    pub title: String,
    pub status: TaskStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffSnapshot {
    pub schema_version: u32,
    pub created_at: DateTime<Local>,
    pub revision: Option<String>,
    pub branch: Option<String>,
    pub dirty: bool,
    pub state: WorkflowState,
    pub tasks: Vec<TaskSnapshot>,
    pub evidence_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug)]
pub struct ResumeReport {
    pub snapshot: HandoffSnapshot,
    pub current_revision: Option<String>,
    pub current_branch: Option<String>,
    pub current_dirty: bool,
    pub stale: bool,
}

impl ResumeReport {
    pub fn render(&self) -> String {
        let snapshot = &self.snapshot;
        let revision = short(snapshot.revision.as_deref());
        let branch = snapshot.branch.as_deref().unwrap_or("no branch");
        let mut lines = vec![
            format!(
                "Handoff: {} ({branch} @ {revision})",
                snapshot.created_at.format("%Y-%m-%d %H:%M")
            ),
            format!("Tasks: {}", task_line(&snapshot.tasks)),
        ];
        if let Some(focus) = snapshot.state.focus.first() {
            lines.push(format!("Focus: {focus}"));
        }
        if !snapshot.state.next.is_empty() {
            let next: Vec<&str> = snapshot
                .state
                .next
                .iter()
                .map(String::as_str)
                .take(3)
                .collect();
            lines.push(format!("Next: {}", next.join("; ")));
        }
        if let Some(note) = &snapshot.note {
            lines.push(format!("Note: {note}"));
        }
        if self.stale {
            lines.push(format!(
                "Stale: snapshot {} != current {}; re-read changed files before continuing.",
                revision,
                short(self.current_revision.as_deref())
            ));
        } else {
            lines.push(format!(
                "Current: {} @ {}{}",
                self.current_branch.as_deref().unwrap_or("no branch"),
                short(self.current_revision.as_deref()),
                if self.current_dirty {
                    " (working tree has changes)"
                } else {
                    ""
                }
            ));
        }
        lines.join("\n")
    }
}

fn short(revision: Option<&str>) -> &str {
    revision
        .map(|revision| &revision[..revision.len().min(8)])
        .unwrap_or("no revision")
}

fn task_line(tasks: &[TaskSnapshot]) -> String {
    if tasks.is_empty() {
        return "none".to_string();
    }
    tasks
        .iter()
        .map(|task| format!("{} {} — {}", task.id, task.status.as_str(), task.title))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Freeze the current revision, workflow state, tasks, and evidence references.
pub fn capture(
    root: &Path,
    state: &WorkflowState,
    note: Option<&str>,
    lock: &FileLock,
) -> Result<HandoffSnapshot> {
    let tasks = tasks::list(root)?;
    let facts = project::discover(root)?;
    let git = facts.git.clone();
    let mut evidence = BTreeSet::new();
    for task in &tasks {
        evidence.extend(task.evidence_ids.iter().cloned());
    }
    let snapshot = HandoffSnapshot {
        schema_version: HANDOFF_SCHEMA_VERSION,
        created_at: Local::now(),
        revision: git.as_ref().and_then(|git| git.revision.clone()),
        branch: git.as_ref().and_then(|git| git.branch.clone()),
        dirty: git.map(|git| git.dirty).unwrap_or(false),
        state: state.clone(),
        tasks: tasks
            .iter()
            .map(|task| TaskSnapshot {
                id: task.id.clone(),
                title: task.title.clone(),
                status: task.status,
            })
            .collect(),
        evidence_ids: evidence.into_iter().collect(),
        note: note.map(events::redact).filter(|note| !note.is_empty()),
    };

    let target = persist::safe_target(root, Path::new(HANDOFF_PATH))?;
    let raw = serde_json::to_string_pretty(&snapshot).context("could not serialize handoff")?;
    persist::atomic_write(&target, format!("{raw}\n").as_bytes(), false)
        .with_context(|| format!("could not write {}", target.display()))?;
    events::append(
        root,
        &Event::handoff(snapshot.revision.as_deref(), snapshot.tasks.len(), note),
        lock,
    )?;
    Ok(snapshot)
}

pub fn load(root: &Path) -> Result<Option<HandoffSnapshot>> {
    let path = root.join(HANDOFF_PATH);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("could not read {}", path.display())),
    };
    let snapshot = serde_json::from_str(&raw)
        .with_context(|| format!("could not parse {}", path.display()))?;
    Ok(Some(snapshot))
}

/// Compare the snapshot revision with the working tree. Resume never rewrites
/// canonical state; it reports stale context and records the attempt.
pub fn resume(root: &Path, lock: &FileLock) -> Result<Option<ResumeReport>> {
    let Some(snapshot) = load(root)? else {
        return Ok(None);
    };
    let facts = project::discover(root)?;
    let git = facts.git;
    let current_revision = git.as_ref().and_then(|git| git.revision.clone());
    let current_branch = git.as_ref().and_then(|git| git.branch.clone());
    let current_dirty = git.map(|git| git.dirty).unwrap_or(false);
    let stale = match (snapshot.revision.as_deref(), current_revision.as_deref()) {
        (Some(snapshot), Some(current)) => snapshot != current,
        (Some(_), None) => true,
        (None, _) => false,
    };
    events::append(
        root,
        &Event::resume(
            snapshot.revision.as_deref(),
            current_revision.as_deref(),
            stale,
        ),
        lock,
    )?;
    Ok(Some(ResumeReport {
        snapshot,
        current_revision,
        current_branch,
        current_dirty,
        stale,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_snapshot_round_trips_tasks_and_state() {
        let dir = tempfile::tempdir().unwrap();
        let lock = FileLock::acquire(dir.path()).unwrap();
        tasks::add(dir.path(), "Ship it", None, &[], &[], &lock).unwrap();
        let state = WorkflowState {
            schema_version: crate::state::STATE_SCHEMA_VERSION,
            focus: vec!["Finish Phase 3".to_string()],
            next: vec!["Run tests".to_string()],
            done: Vec::new(),
            decisions: Vec::new(),
            blockers: Vec::new(),
            updated_at: Local::now(),
        };
        let snapshot = capture(dir.path(), &state, Some("handoff"), &lock).unwrap();
        assert_eq!(snapshot.tasks.len(), 1);
        assert_eq!(snapshot.note.as_deref(), Some("handoff"));
        let loaded = load(dir.path()).unwrap().unwrap();
        assert_eq!(loaded.schema_version, HANDOFF_SCHEMA_VERSION);
        assert_eq!(loaded.tasks[0].id, "TASK-0001");
        let journal = events::load(dir.path()).unwrap();
        assert_eq!(journal.len(), 1);
    }
}
