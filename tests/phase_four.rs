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
    fs::write(
        dir.path().join("src/main.rs"),
        "fn main() { println!(\"orbit\"); }\n",
    )
    .unwrap();
    let output = ghost(dir.path(), &["init"]);
    assert!(output.status.success(), "{}", stderr(&output));
    dir
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

fn evidence_files(dir: &Path) -> Vec<Value> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir.join(".ghost/evidence")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            files.push(read_json(&path));
        }
    }
    files
}

#[test]
fn context_packet_uses_task_metadata_and_respects_budget() {
    let dir = project();
    fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn token() -> &'static str { \"rotated\" }\n".repeat(120),
    )
    .unwrap();
    assert!(ghost(
        dir.path(),
        &[
            "task",
            "add",
            "Add token rotation",
            "--ref",
            "src/main.rs",
            "--check",
            "cargo test",
        ],
    )
    .status
    .success());

    let packet = ghost(
        dir.path(),
        &["context", "--task", "TASK-0001", "--budget", "800"],
    );
    assert!(packet.status.success(), "{}", stderr(&packet));
    let text = stdout(&packet);
    assert!(text.contains("Add token rotation"), "{text}");
    assert!(text.contains("cargo test"), "{text}");
    assert!(text.contains("src/main.rs"), "{text}");
    assert!(text.contains("Estimated"), "{text}");

    let json = ghost(
        dir.path(),
        &[
            "context",
            "--task",
            "TASK-0001",
            "--budget",
            "800",
            "--json",
        ],
    );
    assert!(json.status.success(), "{}", stderr(&json));
    let parsed: Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(parsed["schema_version"], 1);
    assert_eq!(parsed["task_id"], "TASK-0001");
    assert_eq!(parsed["budget_tokens"], 800);
    assert!(parsed["estimated_tokens"].as_u64().unwrap() <= 800);
    assert!(parsed["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| { file["path"] == "src/main.rs" }));
}

#[test]
fn verify_requires_approval_then_records_evidence_and_events() {
    let dir = project();
    assert!(ghost(
        dir.path(),
        &["task", "add", "Check the build", "--check", "true"],
    )
    .status
    .success());

    let refused = ghost(dir.path(), &["verify", "--task", "TASK-0001"]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        stderr(&refused).contains("not approved"),
        "{}",
        stderr(&refused)
    );

    let approved = ghost(dir.path(), &["verify", "--task", "TASK-0001", "--approve"]);
    assert!(approved.status.success(), "{}", stderr(&approved));
    assert!(
        stdout(&approved).contains("exit=0"),
        "{}",
        stdout(&approved)
    );

    let evidence = evidence_files(dir.path());
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0]["schema_version"], 1);
    assert_eq!(evidence[0]["command"], "true");
    assert_eq!(evidence[0]["exit_code"], 0);
    assert_eq!(evidence[0]["task_id"], "TASK-0001");
    assert!(!evidence[0]["source_hashes"].as_object().unwrap().is_empty());

    let task = read_json(&dir.path().join(".ghost/tasks/TASK-0001.json"));
    assert_eq!(task["evidence_ids"].as_array().unwrap().len(), 1);

    let entries = journal(dir.path());
    assert_eq!(entries.last().unwrap()["kind"], "verification");

    let listed = ghost(dir.path(), &["verify", "--list"]);
    assert!(listed.status.success(), "{}", stderr(&listed));
    assert!(stdout(&listed).contains("exit=0"), "{}", stdout(&listed));
    assert!(stdout(&listed).contains("TASK-0001"), "{}", stdout(&listed));

    let allowlist = ghost(dir.path(), &["verify", "--", "true"]);
    assert_eq!(allowlist.status.code(), Some(1));
    fs::write(
        dir.path().join(".ghost/verify.json"),
        "{\"schema_version\":1,\"allow\":[\"true\"]}\n",
    )
    .unwrap();
    let allowed = ghost(dir.path(), &["verify", "--", "true"]);
    assert!(allowed.status.success(), "{}", stderr(&allowed));
    assert_eq!(evidence_files(dir.path()).len(), 2);
}

#[test]
fn evidence_goes_stale_when_a_referenced_file_changes() {
    let dir = project();
    fs::write(dir.path().join("src/input.txt"), "one\n").unwrap();
    assert!(ghost(
        dir.path(),
        &[
            "task",
            "add",
            "Read input",
            "--ref",
            "src/input.txt",
            "--check",
            "true",
        ],
    )
    .status
    .success());
    assert!(ghost(dir.path(), &["task", "start", "TASK-0001"])
        .status
        .success());
    assert!(
        ghost(dir.path(), &["verify", "--task", "TASK-0001", "--approve"])
            .status
            .success()
    );

    let fresh = ghost(dir.path(), &["verify", "--list"]);
    assert!(!stdout(&fresh).contains("[stale]"), "{}", stdout(&fresh));

    fs::write(dir.path().join("src/input.txt"), "two\n").unwrap();
    let stale = ghost(dir.path(), &["verify", "--list"]);
    assert!(stale.status.success(), "{}", stderr(&stale));
    assert!(stdout(&stale).contains("[stale]"), "{}", stdout(&stale));

    let failing = ghost(dir.path(), &["verify", "--approve", "--", "false"]);
    assert_eq!(failing.status.code(), Some(1));
    assert!(stdout(&failing).contains("exit=1"), "{}", stdout(&failing));
}
