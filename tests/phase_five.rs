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

#[test]
fn corrupt_state_is_reported_and_preserved() {
    let dir = project();
    let path = dir.path().join(".ghost/state.json");
    fs::write(&path, "{\"schema_version\": 1,").unwrap();

    let output = ghost(dir.path(), &["status"]);
    assert_eq!(output.status.code(), Some(1));
    let error = stderr(&output);
    assert!(error.contains("could not parse"), "{error}");
    assert!(error.contains("state.json"), "{error}");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "{\"schema_version\": 1,"
    );
}

#[test]
fn corrupt_task_is_reported_and_preserved() {
    let dir = project();
    assert!(ghost(dir.path(), &["task", "add", "Keep me"])
        .status
        .success());
    let path = dir.path().join(".ghost/tasks/TASK-0001.json");
    fs::write(&path, "{\"schema_version\": 1,").unwrap();

    let output = ghost(dir.path(), &["task", "list"]);
    assert_eq!(output.status.code(), Some(1));
    let error = stderr(&output);
    assert!(error.contains("could not parse"), "{error}");
    assert!(error.contains("TASK-0001.json"), "{error}");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "{\"schema_version\": 1,"
    );
}

#[test]
fn live_lock_blocks_and_a_dead_lock_is_reclaimed() {
    let dir = project();
    let lock = dir.path().join(".ghost/.lock");
    fs::write(&lock, format!("{}\n", std::process::id())).unwrap();

    let blocked = ghost(dir.path(), &["task", "add", "Blocked"]);
    assert_eq!(blocked.status.code(), Some(1));
    assert!(
        stderr(&blocked).contains("another ghost process"),
        "{}",
        stderr(&blocked)
    );
    assert!(lock.exists());

    fs::write(&lock, "4000000\n").unwrap();
    let reclaimed = ghost(dir.path(), &["task", "add", "Reclaimed"]);
    assert!(reclaimed.status.success(), "{}", stderr(&reclaimed));
    assert!(!lock.exists());
    let listed = ghost(dir.path(), &["task", "list"]);
    assert!(String::from_utf8_lossy(&listed.stdout).contains("TASK-0001"));
}

#[test]
fn corrupt_manifest_blocks_regen_without_touching_views() {
    let dir = project();
    let agents = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    let manifest = dir.path().join(".ghost/generated-manifest.json");
    fs::write(&manifest, "not json").unwrap();

    let output = ghost(dir.path(), &["regen"]);
    assert_eq!(output.status.code(), Some(1));
    let error = stderr(&output);
    assert!(error.contains("could not parse"), "{error}");
    assert!(error.contains("generated-manifest.json"), "{error}");
    assert_eq!(
        fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
        agents
    );
}

#[test]
fn leftover_temporary_files_are_ignored() {
    let dir = project();
    let temp = dir.path().join(".ghost/.state.json.ghost-tmp-424242-0");
    fs::write(&temp, "half-written").unwrap();

    let output = ghost(dir.path(), &["status"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let state = fs::read_to_string(dir.path().join(".ghost/state.json")).unwrap();
    assert!(serde_json::from_str::<serde_json::Value>(&state).is_ok());
    assert_eq!(fs::read_to_string(&temp).unwrap(), "half-written");
}
