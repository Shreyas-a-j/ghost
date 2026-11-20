use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

const GENERATED_FILES: &[&str] = &[
    "AGENTS.md",
    "STATE.md",
    "specs/00-overview.md",
    "specs/schema.yaml",
    "scripts/check.sh",
    "scripts/dev.sh",
    ".opencode/commands/plan.md",
    ".opencode/commands/build.md",
    ".opencode/commands/fix.md",
    ".opencode/commands/test.md",
    ".opencode/commands/review.md",
    ".opencode/commands/handoff.md",
    ".opencode/commands/checkpoint.md",
    ".ghost/answers.json",
    ".ghost/config.toml",
    ".ghost/project.json",
    ".ghost/state.json",
    ".ghost/generated-manifest.json",
];

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

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

struct Case {
    stack: &'static str,
    manifest: &'static str,
    manifest_body: &'static str,
    source: &'static str,
    source_body: &'static str,
}

const CASES: [Case; 3] = [
    Case {
        stack: "rust",
        manifest: "Cargo.toml",
        manifest_body: "[package]\nname = \"orbit\"\nversion = \"0.1.0\"\n",
        source: "src/main.rs",
        source_body: "fn main() {}\n",
    },
    Case {
        stack: "python",
        manifest: "pyproject.toml",
        manifest_body: "[project]\nname = \"orbit\"\nversion = \"0.1.0\"\n",
        source: "app.py",
        source_body: "print('hi')\n",
    },
    Case {
        stack: "typescript_node",
        manifest: "package.json",
        manifest_body: "{\"name\":\"orbit\",\"scripts\":{\"test\":\"true\"}}\n",
        source: "index.js",
        source_body: "console.log('hi');\n",
    },
];

fn project(case: &Case) -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(case.manifest), case.manifest_body).unwrap();
    let source = dir.path().join(case.source);
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(source, case.source_body).unwrap();
    fs::write(dir.path().join(".gitignore"), "local-cache/\n").unwrap();
    dir
}

#[test]
fn init_detects_existing_rust_python_and_node_without_data_loss() {
    for case in &CASES {
        let dir = project(case);
        let output = ghost(dir.path(), &["init"]);
        assert!(
            output.status.success(),
            "{}: {}",
            case.stack,
            stderr(&output)
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(case.manifest)).unwrap(),
            case.manifest_body
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(case.source)).unwrap(),
            case.source_body
        );

        for path in GENERATED_FILES {
            assert!(
                dir.path().join(path).exists(),
                "{}: missing {path}",
                case.stack
            );
        }

        let config = fs::read_to_string(dir.path().join(".ghost/config.toml")).unwrap();
        assert!(config.contains("schema_version = 1"));
        assert!(config.contains("profile = \"normal\""));
        assert!(config.contains("adapters = [\"opencode\"]"));

        let project = read_json(&dir.path().join(".ghost/project.json"));
        assert_eq!(project["schema_version"], 1);
        assert_eq!(project["name"], "orbit");
        assert_eq!(project["stack"], case.stack);
        assert_eq!(project["profile"], "normal");
        assert!(project["languages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == case.stack));

        let state = read_json(&dir.path().join(".ghost/state.json"));
        assert_eq!(state["schema_version"], 1);
        assert_eq!(
            state["focus"][0],
            "Project initialized. Read `specs/00-overview.md` and run `/plan`."
        );
        assert_eq!(state["next"][0], "/plan");

        let manifest = read_json(&dir.path().join(".ghost/generated-manifest.json"));
        assert_eq!(manifest["schema_version"], 1);
        assert!(manifest["files"].get("AGENTS.md").is_some());
        assert!(manifest["files"].get("scripts/check.sh").is_some());

        let agents = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(agents.contains("Mode: Follow the global workflow."));
        assert!(!agents.contains("Hackathon sprint"));
        let overview = fs::read_to_string(dir.path().join("specs/00-overview.md")).unwrap();
        assert!(overview.contains("## Primary flow"));
        assert!(overview.contains("None defined yet"));

        let ignore = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        for entry in [
            "local-cache/",
            ".env",
            ".env.*",
            ".ghost/dev.pid",
            ".ghost/dev.log",
        ] {
            assert_eq!(
                ignore.lines().filter(|line| *line == entry).count(),
                1,
                "{}: {entry}",
                case.stack
            );
        }

        let status = ghost(dir.path(), &["status"]);
        assert!(status.status.success(), "{}", stderr(&status));
        let text = stdout(&status);
        assert!(text.contains("no deadline"), "{text}");
        assert!(text.contains("orbit"), "{text}");
        assert!(!text.contains("TIME'S UP"), "{text}");
        assert!(text.lines().count() <= 12, "{text}");
    }
}

#[test]
fn init_is_idempotent_and_requires_force_for_changed_files() {
    let case = &CASES[0];
    let dir = project(case);

    let first = ghost(dir.path(), &["init"]);
    assert!(first.status.success(), "{}", stderr(&first));
    let generated_agents = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();

    let second = ghost(dir.path(), &["init"]);
    assert!(second.status.success(), "{}", stderr(&second));
    assert_eq!(
        fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
        generated_agents
    );

    fs::write(dir.path().join("AGENTS.md"), "local notes\n").unwrap();
    let refused = ghost(dir.path(), &["init"]);
    assert!(!refused.status.success());
    let error = stderr(&refused);
    assert!(error.contains("refusing to overwrite"), "{error}");
    assert!(error.contains("AGENTS.md"), "{error}");
    assert_eq!(
        fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
        "local notes\n"
    );

    let forced = ghost(dir.path(), &["init", "--force"]);
    assert!(forced.status.success(), "{}", stderr(&forced));
    assert_eq!(
        fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
        generated_agents
    );
}

#[test]
fn init_force_migrates_handwritten_state_before_overwriting_the_view() {
    let case = &CASES[0];
    let dir = project(case);
    fs::write(dir.path().join("AGENTS.md"), "my own agent notes\n").unwrap();
    fs::write(
        dir.path().join("STATE.md"),
        "# State\n<!-- ghost:focus -->\n## Current focus\n- Developer handoff\n",
    )
    .unwrap();

    let refused = ghost(dir.path(), &["init"]);
    assert!(!refused.status.success());
    assert_eq!(
        fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
        "my own agent notes\n"
    );

    let forced = ghost(dir.path(), &["init", "--force"]);
    assert!(forced.status.success(), "{}", stderr(&forced));
    let state = read_json(&dir.path().join(".ghost/state.json"));
    assert_eq!(state["focus"][0], "Developer handoff");
}

#[test]
fn regen_migrates_a_v1_project_to_canonical_state() {
    let case = &CASES[0];
    let dir = project(case);
    let answers = dir.path().join("answers.json");
    fs::write(
        &answers,
        json!({
            "project_name": "Orbit",
            "pitch": "Track tasks.",
            "demo_steps": ["Open", "Create", "View"],
            "event_name": "HackMIT 2026",
            "deadline": "2099-01-01 12:00",
            "team_size": "solo",
            "mode": "hackathon",
            "stack": "rust",
            "stack_details": "Cargo",
            "run_cmd": "cargo run",
            "must_haves": ["Task board"],
            "nice_to_haves": [],
            "existing_code": true,
            "risky_areas": [],
            "test_level": "smoke",
            "model_override": null,
            "skills": [],
            "checkpoint_freq": "manual",
            "created_at": "2026-01-01T00:00:00Z"
        })
        .to_string(),
    )
    .unwrap();
    let created = ghost(dir.path(), &["new", "--answers", "answers.json"]);
    assert!(created.status.success(), "{}", stderr(&created));

    for path in [
        ".ghost/state.json",
        ".ghost/project.json",
        ".ghost/config.toml",
        ".ghost/generated-manifest.json",
    ] {
        fs::remove_file(dir.path().join(path)).unwrap();
    }
    let state_markdown = fs::read_to_string(dir.path().join("STATE.md")).unwrap();
    let updated = state_markdown.replace(
        "- Project initialized. Read `specs/00-overview.md` and run `/plan`.",
        "- Resume the migration",
    );
    fs::write(dir.path().join("STATE.md"), updated).unwrap();

    let regen = ghost(dir.path(), &["regen", "--force"]);
    assert!(regen.status.success(), "{}", stderr(&regen));
    let state = read_json(&dir.path().join(".ghost/state.json"));
    assert_eq!(state["schema_version"], 1);
    assert_eq!(state["focus"][0], "Resume the migration");
    assert!(dir.path().join(".ghost/project.json").exists());
    assert!(dir.path().join(".ghost/config.toml").exists());
    assert!(dir.path().join(".ghost/generated-manifest.json").exists());
}

#[cfg(unix)]
#[test]
fn init_refuses_to_write_through_symlinks() {
    use std::os::unix::fs::symlink;

    let case = &CASES[0];
    let dir = project(case);
    let outside = TempDir::new().unwrap();
    let target = outside.path().join("AGENTS.md");
    fs::write(&target, "outside\n").unwrap();
    symlink(&target, dir.path().join("AGENTS.md")).unwrap();

    let refused = ghost(dir.path(), &["init"]);
    assert!(!refused.status.success());
    assert!(stderr(&refused).contains("AGENTS.md"));
    assert_eq!(fs::read_to_string(&target).unwrap(), "outside\n");

    let forced = ghost(dir.path(), &["init", "--force"]);
    assert!(!forced.status.success());
    assert!(stderr(&forced).contains("symlink"), "{}", stderr(&forced));
    assert_eq!(fs::read_to_string(&target).unwrap(), "outside\n");
}
