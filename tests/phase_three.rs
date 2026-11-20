use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

fn ghost(dir: &Path, args: &[&str]) -> Output {
    let opencode = dir.join(".test-opencode");
    Command::new(env!("CARGO_BIN_EXE_ghost"))
        .args(args)
        .current_dir(dir)
        .env("GHOST_OPENCODE_DIR", &opencode)
        .env("GHOST_AGENTS_DIR", opencode.join("skills"))
        .output()
        .unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn project() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"orbit\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    let output = ghost(dir.path(), &["init"]);
    assert!(output.status.success(), "{}", stderr(&output));
    dir
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn init_git_with_baseline(dir: &Path) {
    git(dir, &["init", "-q"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Ghost Test"]);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "baseline"]);
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn journal(dir: &Path) -> Vec<Value> {
    fs::read_to_string(dir.join(".ghost/events.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn task_lifecycle_is_recorded_in_a_redacted_journal() {
    let dir = project();
    let added = ghost(
        dir.path(),
        &[
            "task",
            "add",
            "Add token rotation",
            "--owner",
            "claude",
            "--ref",
            "src/main.rs",
            "--check",
            "cargo test",
            "--json",
        ],
    );
    assert!(added.status.success(), "{}", stderr(&added));
    let task = read_json(&dir.path().join(".ghost/tasks/TASK-0001.json"));
    assert_eq!(task["schema_version"], 1);
    assert_eq!(task["status"], "backlog");
    assert_eq!(task["owner_agent"], "claude");
    assert_eq!(task["references"][0], "src/main.rs");
    assert_eq!(task["acceptance_checks"][0], "cargo test");

    let started = ghost(
        dir.path(),
        &["task", "start", "TASK-0001", "--owner", "opencode"],
    );
    assert!(started.status.success(), "{}", stderr(&started));
    assert!(stdout(&started).contains("in_progress"));
    for status in ["verification", "done"] {
        let moved = ghost(dir.path(), &["task", "move", "TASK-0001", status]);
        assert!(moved.status.success(), "{}", stderr(&moved));
    }
    let task = read_json(&dir.path().join(".ghost/tasks/TASK-0001.json"));
    assert_eq!(task["status"], "done");
    assert_eq!(task["owner_agent"], "opencode");

    let refused = ghost(dir.path(), &["task", "move", "TASK-0001", "verification"]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("cannot move"),
        "{}",
        stderr(&refused)
    );

    let entries = journal(dir.path());
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0]["kind"], "task_created");
    assert_eq!(entries[1]["from"], "backlog");
    assert_eq!(entries[1]["to"], "in_progress");
    assert_eq!(entries[3]["to"], "done");
    for entry in &entries {
        assert_eq!(entry["schema_version"], 1);
    }

    let ignore = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(ignore.lines().any(|line| line == ".ghost/.lock"));

    let status = ghost(dir.path(), &["status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    assert!(stdout(&status).contains("Tasks:"), "{}", stdout(&status));
    assert!(stdout(&status).contains("1 done"), "{}", stdout(&status));
}

#[test]
fn handoff_freezes_state_and_resume_flags_a_stale_revision() {
    let dir = project();
    init_git_with_baseline(dir.path());
    assert!(ghost(dir.path(), &["task", "add", "Ship Phase 3"])
        .status
        .success());
    assert!(ghost(dir.path(), &["task", "start", "TASK-0001"])
        .status
        .success());

    let handoff = ghost(dir.path(), &["handoff", "--note", "resume here"]);
    assert!(handoff.status.success(), "{}", stderr(&handoff));
    let snapshot = read_json(&dir.path().join(".ghost/handoff.json"));
    assert_eq!(snapshot["schema_version"], 1);
    assert_eq!(snapshot["note"], "resume here");
    assert_eq!(snapshot["tasks"][0]["id"], "TASK-0001");
    assert!(snapshot["revision"].as_str().unwrap().len() >= 7);

    let resume = ghost(dir.path(), &["resume"]);
    assert_eq!(resume.status.code(), Some(0), "{}", stderr(&resume));
    assert!(stdout(&resume).contains("Current:"), "{}", stdout(&resume));
    assert!(!stdout(&resume).contains("Stale:"));

    let mut stale = snapshot.clone();
    stale["revision"] = Value::String("deadbeefdeadbeef".to_string());
    fs::write(
        dir.path().join(".ghost/handoff.json"),
        format!("{}\n", serde_json::to_string_pretty(&stale).unwrap()),
    )
    .unwrap();
    let resent = ghost(dir.path(), &["resume"]);
    assert_eq!(resent.status.code(), Some(2), "{}", stderr(&resent));
    assert!(stdout(&resent).contains("Stale:"), "{}", stdout(&resent));

    let kinds: Vec<String> = journal(dir.path())
        .iter()
        .map(|entry| entry["kind"].as_str().unwrap().to_string())
        .collect();
    assert!(kinds.contains(&"handoff".to_string()));
    assert_eq!(kinds.iter().filter(|kind| *kind == "resume").count(), 2);
}

#[test]
fn checkpoint_metadata_is_committed_with_the_checkpoint() {
    let dir = project();
    init_git_with_baseline(dir.path());
    fs::write(
        dir.path().join("src/main.rs"),
        "fn main() { println!(\"hi\"); }\n",
    )
    .unwrap();

    let checkpoint = ghost(dir.path(), &["checkpoint", "phase three"]);
    assert!(checkpoint.status.success(), "{}", stderr(&checkpoint));
    let events = journal(dir.path());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["kind"], "checkpoint");
    assert_eq!(events[0]["metadata"]["message"], "phase three");
    assert!(events[0]["revision"].as_str().is_some());

    let again = ghost(dir.path(), &["checkpoint"]);
    assert!(again.status.success(), "{}", stderr(&again));
    assert!(
        stdout(&again).contains("nothing to commit"),
        "{}",
        stdout(&again)
    );
}
