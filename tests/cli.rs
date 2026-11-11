use serde_json::json;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::thread::sleep;
use std::time::Duration;
use tempfile::TempDir;

fn fixture(run_cmd: &str, model: Option<&str>) -> String {
    json!({
        "project_name": "Orbit",
        "pitch": "A tiny dashboard for demo day.",
        "demo_steps": ["Open the dashboard", "Add a task", "See progress"],
        "event_name": "HackMIT 2026",
        "deadline": "2099-11-14 10:00",
        "team_size": "solo",
        "mode": "hackathon",
        "stack": "rust",
        "stack_details": "axum 0.8, serde 1",
        "run_cmd": run_cmd,
        "must_haves": ["Sign in", "Task board", "Export"],
        "nice_to_haves": ["Dark mode"],
        "existing_code": false,
        "risky_areas": ["auth"],
        "test_level": "smoke",
        "model_override": model,
        "skills": ["tdd"],
        "checkpoint_freq": "before_every",
        "created_at": "2026-01-01T09:00:00+00:00"
    })
    .to_string()
}

fn ghost(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ghost"));
    command.current_dir(dir);
    command
}

fn new_project(dir: &Path, run_cmd: &str, model: Option<&str>) -> Output {
    let answers = dir.join("answers.json");
    fs::write(&answers, fixture(run_cmd, model)).unwrap();
    ghost(dir)
        .args(["new", "--answers", "answers.json"])
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn wait_dead(pid: u32) {
    for _ in 0..60 {
        if !alive(pid) {
            return;
        }
        sleep(Duration::from_millis(50));
    }
    panic!("process {pid} is still alive");
}

#[test]
fn end_to_end_new_generates_files_and_status() {
    let dir = TempDir::new().unwrap();
    let output = new_project(dir.path(), "cargo run", None);
    assert!(output.status.success(), "{}", stderr(&output));

    for path in [
        "AGENTS.md",
        "STATE.md",
        "scripts/check.sh",
        "scripts/dev.sh",
        "specs/00-overview.md",
        "specs/feature-sign-in.md",
        "specs/feature-task-board.md",
        "specs/feature-export.md",
        "specs/schema.yaml",
        ".opencode/commands/plan.md",
        ".opencode/commands/build.md",
        ".opencode/commands/fix.md",
        ".opencode/commands/test.md",
        ".opencode/commands/review.md",
        ".opencode/commands/handoff.md",
        ".opencode/commands/checkpoint.md",
        ".ghost/answers.json",
    ] {
        assert!(dir.path().join(path).exists(), "missing {path}");
    }
    let agents = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(agents.lines().count() <= 40);
    assert!(!dir.path().join("opencode.json").exists());

    let status = ghost(dir.path()).arg("status").output().unwrap();
    assert!(status.status.success(), "{}", stderr(&status));
    let text = stdout(&status);
    assert!(text.contains("HackMIT 2026"), "{text}");
    assert!(text.lines().count() <= 12);
}

#[test]
fn new_refuses_to_overwrite_without_force() {
    let dir = TempDir::new().unwrap();
    assert!(new_project(dir.path(), "cargo run", None).status.success());
    let before = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    fs::write(dir.path().join("AGENTS.md"), "user content\n").unwrap();

    let second = ghost(dir.path())
        .args(["new", "--answers", "answers.json"])
        .output()
        .unwrap();
    assert!(!second.status.success());
    let error = stderr(&second);
    assert!(error.contains("refusing to overwrite"), "{error}");
    assert!(error.contains("AGENTS.md"), "{error}");
    assert_eq!(
        fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
        "user content\n"
    );

    let forced = ghost(dir.path())
        .args(["new", "--force", "--answers", "answers.json"])
        .output()
        .unwrap();
    assert!(forced.status.success(), "{}", stderr(&forced));
    assert_eq!(
        fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
        before
    );
}

#[test]
fn missing_answers_exits_one_with_friendly_message() {
    let dir = TempDir::new().unwrap();
    for args in [
        vec!["status"],
        vec!["regen"],
        vec!["check", "--timeout", "1"],
        vec!["down"],
        vec!["checkpoint", "test"],
    ] {
        let output = ghost(dir.path()).args(&args).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(
            stderr(&output).contains("run `ghost new` first"),
            "{args:?}"
        );
    }
}

#[test]
fn agent_detection_uses_env_override() {
    let dir = TempDir::new().unwrap();
    let opencode = TempDir::new().unwrap();
    let agents = opencode.path().join("agents");
    fs::create_dir_all(&agents).unwrap();
    fs::write(agents.join("build.md"), "").unwrap();
    fs::write(agents.join("reviewer.md"), "").unwrap();

    let answers = dir.path().join("answers.json");
    fs::write(&answers, fixture("cargo run", None)).unwrap();
    let output = ghost(dir.path())
        .args(["new", "--answers", "answers.json"])
        .env("GHOST_OPENCODE_DIR", opencode.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));

    let test = fs::read_to_string(dir.path().join(".opencode/commands/test.md")).unwrap();
    assert!(!test.contains("agent:"));
    let review = fs::read_to_string(dir.path().join(".opencode/commands/review.md")).unwrap();
    assert!(review.contains("agent: reviewer"));
    let build = fs::read_to_string(dir.path().join(".opencode/commands/build.md")).unwrap();
    assert!(build.contains("agent: build"));

    fs::write(agents.join("tester.md"), "").unwrap();
    let regen = ghost(dir.path())
        .arg("regen")
        .env("GHOST_OPENCODE_DIR", opencode.path())
        .output()
        .unwrap();
    assert!(regen.status.success(), "{}", stderr(&regen));
    let test = fs::read_to_string(dir.path().join(".opencode/commands/test.md")).unwrap();
    assert!(test.contains("agent: tester"));
}

#[test]
fn dev_script_start_stop_never_duplicates() {
    let dir = TempDir::new().unwrap();
    assert!(new_project(dir.path(), "sleep 300", None).status.success());

    let start = Command::new("bash")
        .args(["scripts/dev.sh", "start"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(start.status.success(), "{}", stderr(&start));
    let pid: u32 = fs::read_to_string(dir.path().join(".ghost/dev.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(alive(pid));

    let second = Command::new("bash")
        .args(["scripts/dev.sh", "start"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(second.status.code(), Some(1));
    assert!(stdout(&second).contains("already running"));

    let stop = Command::new("bash")
        .args(["scripts/dev.sh", "stop"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(stop.status.success());
    assert!(!dir.path().join(".ghost/dev.pid").exists());
    wait_dead(pid);

    let stop_again = Command::new("bash")
        .args(["scripts/dev.sh", "stop"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(stop_again.status.success());
    assert!(stdout(&stop_again).contains("not running"));
}

#[test]
fn ghost_down_stops_dev_server() {
    let dir = TempDir::new().unwrap();
    assert!(new_project(dir.path(), "sleep 300", None).status.success());
    let start = Command::new("bash")
        .args(["scripts/dev.sh", "start"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(start.status.success(), "{}", stderr(&start));
    let pid: u32 = fs::read_to_string(dir.path().join(".ghost/dev.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let down = ghost(dir.path()).arg("down").output().unwrap();
    assert!(down.status.success(), "{}", stderr(&down));
    wait_dead(pid);
    let down_again = ghost(dir.path()).arg("down").output().unwrap();
    assert!(down_again.status.success());
}

#[test]
fn check_timeout_kills_the_process_group() {
    let dir = TempDir::new().unwrap();
    assert!(new_project(dir.path(), "cargo run", None).status.success());
    fs::write(
        dir.path().join("scripts/check.sh"),
        "#!/usr/bin/env bash\nmkdir -p .ghost\necho $$ > .ghost/check.pid\nsleep 60\n",
    )
    .unwrap();

    let output = ghost(dir.path())
        .args(["check", "--timeout", "2"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(124), "{}", stderr(&output));
    assert!(stdout(&output).contains("check timed out after 2s"));
    let pid: u32 = fs::read_to_string(dir.path().join(".ghost/check.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    wait_dead(pid);
}

#[test]
fn check_passes_through_child_exit_code() {
    let dir = TempDir::new().unwrap();
    assert!(new_project(dir.path(), "cargo run", None).status.success());
    fs::write(
        dir.path().join("scripts/check.sh"),
        "#!/usr/bin/env bash\necho stage failing\nexit 7\n",
    )
    .unwrap();
    let output = ghost(dir.path()).arg("check").output().unwrap();
    assert_eq!(output.status.code(), Some(7));
}

#[test]
fn checkpoint_refuses_sensitive_files_and_commits_otherwise() {
    let dir = TempDir::new().unwrap();
    assert!(new_project(dir.path(), "cargo run", None).status.success());
    run_git(dir.path(), &["init", "-q"]);
    run_git(dir.path(), &["config", "user.email", "test@example.com"]);
    run_git(dir.path(), &["config", "user.name", "Ghost Test"]);

    let gitignore = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    let filtered: String = gitignore
        .lines()
        .filter(|line| *line != ".env" && *line != ".env.*")
        .map(|line| format!("{line}\n"))
        .collect();
    fs::write(dir.path().join(".gitignore"), filtered).unwrap();
    fs::write(dir.path().join(".env"), "SECRET=1\n").unwrap();

    let refused = ghost(dir.path())
        .args(["checkpoint", "unsafe"])
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(1));
    assert!(stderr(&refused).contains(".env"));
    assert!(!git_success(dir.path(), &["rev-parse", "HEAD"]));

    fs::remove_file(dir.path().join(".env")).unwrap();
    let committed = ghost(dir.path())
        .args(["checkpoint", "safe test"])
        .output()
        .unwrap();
    assert!(committed.status.success(), "{}", stderr(&committed));
    let subject = git_stdout(dir.path(), &["log", "-1", "--pretty=%s"]);
    assert_eq!(subject.trim(), "checkpoint: safe test");
    let remotes = git_stdout(dir.path(), &["remote"]);
    assert!(remotes.trim().is_empty());

    let again = ghost(dir.path())
        .args(["checkpoint", "again"])
        .output()
        .unwrap();
    assert!(again.status.success());
    assert!(stdout(&again).contains("nothing to commit"));
}

fn run_git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn git_success(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).to_string()
}
