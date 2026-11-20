use crate::persist::{self, FileLock};
use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

pub const EVENTS_SCHEMA_VERSION: u32 = 1;
pub const EVENTS_PATH: &str = ".ghost/events.jsonl";
pub const MAX_FIELD_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    TaskCreated,
    TaskTransition,
    Handoff,
    Resume,
    Checkpoint,
    Verification,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub schema_version: u32,
    pub timestamp: DateTime<Local>,
    pub kind: EventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

impl Event {
    fn new(kind: EventKind) -> Self {
        Self {
            schema_version: EVENTS_SCHEMA_VERSION,
            timestamp: Local::now(),
            kind,
            task_id: None,
            from: None,
            to: None,
            revision: None,
            metadata: BTreeMap::new(),
        }
    }

    pub fn task_created(task_id: &str, title: &str, owner: Option<&str>) -> Self {
        let mut event = Self::new(EventKind::TaskCreated).with_task(task_id);
        event.meta("title", title);
        if let Some(owner) = owner {
            event.meta("owner", owner);
        }
        event
    }

    pub fn task_transition(task_id: &str, from: &str, to: &str, owner: Option<&str>) -> Self {
        let mut event = Self::new(EventKind::TaskTransition).with_task(task_id);
        event.from = Some(from.to_string());
        event.to = Some(to.to_string());
        if let Some(owner) = owner {
            event.meta("owner", owner);
        }
        event
    }

    pub fn handoff(revision: Option<&str>, task_count: usize, note: Option<&str>) -> Self {
        let mut event = Self::new(EventKind::Handoff);
        event.revision = revision.map(str::to_string);
        event.meta("tasks", &task_count.to_string());
        if let Some(note) = note {
            event.meta("note", note);
        }
        event
    }

    pub fn resume(
        snapshot_revision: Option<&str>,
        current_revision: Option<&str>,
        stale: bool,
    ) -> Self {
        let mut event = Self::new(EventKind::Resume);
        event.revision = current_revision.map(str::to_string);
        if let Some(revision) = snapshot_revision {
            event.meta("snapshot_revision", revision);
        }
        event.meta("stale", if stale { "true" } else { "false" });
        event
    }

    pub fn checkpoint(revision: Option<&str>, message: &str) -> Self {
        let mut event = Self::new(EventKind::Checkpoint);
        event.revision = revision.map(str::to_string);
        event.meta("message", message);
        event
    }

    pub fn verification(
        task_id: Option<&str>,
        run_id: &str,
        command: &str,
        exit_code: i32,
        timed_out: bool,
        revision: Option<&str>,
    ) -> Self {
        let mut event = Self::new(EventKind::Verification);
        if let Some(task_id) = task_id {
            event = event.with_task(task_id);
        }
        event.revision = revision.map(str::to_string);
        event.meta("run", run_id);
        event.meta("command", command);
        event.meta("exit_code", &exit_code.to_string());
        if timed_out {
            event.meta("timed_out", "true");
        }
        event
    }

    fn with_task(mut self, task_id: &str) -> Self {
        self.task_id = Some(redact(task_id));
        self
    }

    /// Metadata is deliberately short and line-free. Raw prompts, logs, and
    /// secrets must never reach the journal.
    fn meta(&mut self, key: &str, value: &str) {
        self.metadata.insert(redact(key), redact(value));
    }
}

/// Collapse control characters and cap length so the journal stays metadata-only.
pub fn redact(value: &str) -> String {
    let masked = mask_secrets(value);
    let mut cleaned = String::with_capacity(masked.len().min(MAX_FIELD_CHARS));
    let mut last_space = false;
    for ch in masked.chars() {
        let ch = if ch.is_control() { ' ' } else { ch };
        if ch == ' ' {
            if last_space {
                continue;
            }
            last_space = true;
        } else {
            last_space = false;
        }
        if cleaned.chars().count() >= MAX_FIELD_CHARS {
            cleaned.push_str("...");
            break;
        }
        cleaned.push(ch);
    }
    cleaned.trim().to_string()
}

/// Remove terminal control characters (including ESC) without truncating.
/// Apply this to values loaded from repository files before printing them.
pub fn terminal_safe(value: &str) -> String {
    value
        .chars()
        .filter(|ch| *ch == '\t' || !ch.is_control())
        .collect()
}

const SECRET_KEYS: [&str; 13] = [
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "api-key",
    "apikey",
    "authorization",
    "private_key",
    "private-key",
    "access_key",
    "client_secret",
    "bearer",
];

/// Mask `key=value` and `key: value` pairs for common credential names. This is
/// deliberately narrow; it reduces accidental journal/evidence leakage but is
/// not a secret scanner.
fn mask_secrets(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for key in SECRET_KEYS {
        let mut from = 0;
        while let Some(offset) = lower[from..].find(key) {
            let start = from + offset;
            let key_end = start + key.len();
            let before_ok = start == 0
                || lower[..start]
                    .chars()
                    .next_back()
                    .map(|ch| !ch.is_alphanumeric())
                    .unwrap_or(true);
            let mut cursor = key_end;
            while lower[cursor..].starts_with(' ') {
                cursor += 1;
            }
            let separator = lower[cursor..].chars().next();
            if before_ok && matches!(separator, Some('=') | Some(':')) {
                let mut value_start = cursor + 1;
                while let Some(ch) = lower[value_start..].chars().next() {
                    if ch == ' ' || ch == '"' || ch == '\'' {
                        value_start += ch.len_utf8();
                    } else {
                        break;
                    }
                }
                let mut value_end = value_start
                    + lower[value_start..]
                        .find(|ch: char| {
                            ch.is_whitespace() || matches!(ch, ',' | ';' | '"' | '\'' | '}' | ']')
                        })
                        .unwrap_or(lower.len() - value_start);
                if key == "authorization"
                    && matches!(&lower[value_start..value_end], "bearer" | "basic")
                {
                    while lower[value_end..].starts_with(' ') {
                        value_end += 1;
                    }
                    value_end += lower[value_end..]
                        .find(|ch: char| {
                            ch.is_whitespace() || matches!(ch, ',' | ';' | '"' | '\'' | '}' | ']')
                        })
                        .unwrap_or(lower.len() - value_end);
                }
                if value_end > value_start {
                    ranges.push((value_start, value_end));
                }
            }
            from = key_end;
        }
    }
    if ranges.is_empty() {
        return input.to_string();
    }
    ranges.sort_unstable();
    let mut out = String::with_capacity(input.len());
    let mut last = 0;
    for (start, end) in ranges {
        if start < last {
            continue;
        }
        out.push_str(&input[last..start]);
        out.push_str("[redacted]");
        last = end;
    }
    out.push_str(&input[last..]);
    out
}

/// Append one event. The caller owns the `.ghost` lock.
pub fn append(root: &Path, event: &Event, lock: &FileLock) -> Result<()> {
    let _ = lock;
    let target = persist::safe_target(root, Path::new(EVENTS_PATH))?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    let line = serde_json::to_string(event).context("could not serialize event")?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&target)
        .with_context(|| format!("could not open {}", target.display()))?;
    writeln!(file, "{line}").with_context(|| format!("could not append {}", target.display()))?;
    file.sync_all()
        .with_context(|| format!("could not sync {}", target.display()))?;
    Ok(())
}

/// Read the journal for diagnostics and tests. Malformed lines are skipped
/// rather than blocking the CLI on a partially written historical record.
#[cfg(test)]
pub fn load(root: &Path) -> Result<Vec<Event>> {
    let path = root.join(EVENTS_PATH);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("could not read {}", path.display())),
    };
    Ok(text
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redaction_is_single_line_and_bounded() {
        let raw = format!("{}\n\tapi_key=secret", "long ".repeat(100));
        let redacted = redact(&raw);
        assert!(!redacted.contains('\n'));
        assert!(!redacted.contains('\t'));
        assert!(redacted.chars().count() <= MAX_FIELD_CHARS + 3);
        assert!(redacted.ends_with("..."));
    }

    #[test]
    fn secrets_are_masked_before_storage() {
        let raw = "api_key=abc123 password: hunter2 Authorization: Bearer tok-9";
        let cleaned = redact(raw);
        assert!(!cleaned.contains("abc123"), "{cleaned}");
        assert!(!cleaned.contains("hunter2"), "{cleaned}");
        assert!(!cleaned.contains("tok-9"), "{cleaned}");
        assert!(cleaned.contains("[redacted]"), "{cleaned}");
        assert_eq!(redact("build passed"), "build passed");
    }

    #[test]
    fn events_are_metadata_only_and_append() {
        let dir = tempfile::tempdir().unwrap();
        let lock = FileLock::acquire(dir.path()).unwrap();
        let event = Event::task_transition("TASK-0001", "backlog", "in_progress", Some("claude"));
        append(dir.path(), &event, &lock).unwrap();
        append(
            dir.path(),
            &Event::handoff(Some("abc"), 1, Some("line\nbreak")),
            &lock,
        )
        .unwrap();
        let loaded = load(dir.path()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].kind, EventKind::TaskTransition);
        assert_eq!(loaded[0].to.as_deref(), Some("in_progress"));
        assert_eq!(loaded[1].metadata["note"], "line break");
        assert_eq!(loaded[1].schema_version, EVENTS_SCHEMA_VERSION);
    }
}
