use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

const FIXTURES: [(&str, &str); 3] = [
    ("opencode", include_str!("fixtures/phase_two/opencode.json")),
    ("claude", include_str!("fixtures/phase_two/claude.json")),
    ("codex", include_str!("fixtures/phase_two/codex.json")),
];

const COMMANDS: [&str; 7] = [
    "plan",
    "build",
    "fix",
    "test",
    "review",
    "handoff",
    "checkpoint",
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

fn project() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"orbit\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    dir
}

fn config_adapters(dir: &Path) -> String {
    let config = fs::read_to_string(dir.join(".ghost/config.toml")).unwrap();
    config
        .lines()
        .find(|line| line.starts_with("adapters"))
        .unwrap_or_default()
        .to_string()
}

fn split_frontmatter(content: &str) -> (&str, &str) {
    let rest = content
        .strip_prefix("---\n")
        .unwrap_or_else(|| panic!("missing frontmatter start:\n{content}"));
    let (header, body) = rest
        .split_once("\n---\n")
        .unwrap_or_else(|| panic!("missing frontmatter end:\n{content}"));
    (header, body)
}

#[test]
fn adapter_fixtures_match_generated_surfaces() {
    for (name, raw) in FIXTURES {
        let fixture: Value = serde_json::from_str(raw).unwrap();
        let dir = project();
        let output = ghost(dir.path(), &["init", "--adapter", name]);
        assert!(output.status.success(), "{name}: {}", stderr(&output));

        for path in fixture["required_files"].as_array().unwrap() {
            let path = path.as_str().unwrap();
            assert!(
                dir.path().join(path).exists(),
                "{name}: missing required {path}"
            );
        }
        for path in fixture["forbidden_files"].as_array().unwrap() {
            let path = path.as_str().unwrap();
            assert!(!dir.path().join(path).exists(), "{name}: unexpected {path}");
        }

        assert_eq!(
            config_adapters(dir.path()),
            format!("adapters = [\"{name}\"]")
        );

        let instruction_path = dir.path().join(match name {
            "claude" => "CLAUDE.md",
            _ => "AGENTS.md",
        });
        let instructions = fs::read_to_string(&instruction_path).unwrap();
        for layout in fixture["instruction_layout"].as_array().unwrap() {
            let layout = layout.as_str().unwrap();
            assert!(
                instructions.contains(layout),
                "{name}: instructions do not mention {layout}"
            );
        }

        let state = fs::read_to_string(dir.path().join("STATE.md")).unwrap();
        let has_commands = fixture["state_has_slash_commands"].as_bool().unwrap();
        assert_eq!(
            state.contains("`/plan`"),
            has_commands,
            "{name}: unexpected slash commands in STATE.md:\n{state}"
        );
        assert!(
            state.contains("Read `specs/00-overview.md`"),
            "{name}: {state}"
        );
        let overview = fs::read_to_string(dir.path().join("specs/00-overview.md")).unwrap();
        assert_eq!(
            overview.contains("`/plan`"),
            has_commands,
            "{name}: unexpected slash commands in overview:\n{overview}"
        );

        if let Some(directory) = fixture["commands_dir"].as_str() {
            let hints = fixture["argument_hint_commands"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap())
                .collect::<Vec<_>>();
            let forbidden = fixture["forbidden_command_text"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap())
                .collect::<Vec<_>>();
            for command in COMMANDS {
                let path = dir.path().join(directory).join(format!("{command}.md"));
                let content = fs::read_to_string(&path)
                    .unwrap_or_else(|_| panic!("{name}: missing {}", path.display()));
                let (header, body) = split_frontmatter(&content);
                assert!(
                    header.lines().any(|line| line.starts_with("description: ")),
                    "{name}/{command}: missing description"
                );
                assert!(
                    !body.starts_with("---"),
                    "{name}/{command}: duplicate frontmatter"
                );
                let expects_hint = hints.contains(&command);
                assert_eq!(
                    header.contains("argument-hint:"),
                    expects_hint,
                    "{name}/{command}: argument-hint mismatch"
                );
                for text in &forbidden {
                    assert!(
                        !content.contains(text),
                        "{name}/{command}: unexpected {text}"
                    );
                }
            }
        }
    }
}

#[test]
fn combined_adapters_share_common_files_and_respect_command_conventions() {
    let dir = project();
    let output = ghost(
        dir.path(),
        &[
            "init",
            "--adapter",
            "opencode",
            "--adapter",
            "claude",
            "--adapter",
            "codex",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));

    assert_eq!(
        config_adapters(dir.path()),
        "adapters = [\"opencode\",\"claude\",\"codex\"]"
    );
    for path in [
        "AGENTS.md",
        "CLAUDE.md",
        "STATE.md",
        ".opencode/commands/build.md",
        ".claude/commands/build.md",
    ] {
        assert!(dir.path().join(path).exists(), "missing {path}");
    }
    assert!(dir.path().join("STATE.md").exists());
    let state = fs::read_to_string(dir.path().join("STATE.md")).unwrap();
    assert!(state.contains("`/plan`"));

    let opencode = fs::read_to_string(dir.path().join(".opencode/commands/build.md")).unwrap();
    assert!(opencode.contains("`AGENTS.md`"));
    assert!(!opencode.contains("argument-hint:"));
    let claude = fs::read_to_string(dir.path().join(".claude/commands/build.md")).unwrap();
    assert!(claude.contains("`CLAUDE.md`"));
    assert!(claude.contains("argument-hint: <feature-slug>"));
}

#[test]
fn regen_keeps_or_replaces_the_saved_adapter_selection() {
    let dir = project();
    let first = ghost(dir.path(), &["init", "--adapter", "claude"]);
    assert!(first.status.success(), "{}", stderr(&first));
    assert_eq!(config_adapters(dir.path()), "adapters = [\"claude\"]");

    let kept = ghost(dir.path(), &["regen"]);
    assert!(kept.status.success(), "{}", stderr(&kept));
    assert_eq!(config_adapters(dir.path()), "adapters = [\"claude\"]");
    assert!(dir.path().join(".claude/commands/build.md").exists());

    let replaced = ghost(dir.path(), &["regen", "--adapter", "codex"]);
    assert!(replaced.status.success(), "{}", stderr(&replaced));
    assert_eq!(config_adapters(dir.path()), "adapters = [\"codex\"]");
    let agents = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(!agents.contains(".claude/commands"));
    let state = fs::read_to_string(dir.path().join("STATE.md")).unwrap();
    assert!(!state.contains("`/plan`"), "{state}");
}

#[test]
fn unknown_adapters_are_rejected_before_writing() {
    let dir = project();
    let output = ghost(dir.path(), &["init", "--adapter", "gemini"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("unknown adapter"),
        "{}",
        stderr(&output)
    );
    assert!(!dir.path().join("AGENTS.md").exists());
    assert!(!dir.path().join(".ghost/config.toml").exists());
}

#[test]
fn adapters_command_reports_capabilities_as_json() {
    let dir = project();
    let output = ghost(dir.path(), &["adapters", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let info: Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = info.as_array().unwrap();
    assert_eq!(entries.len(), 3);
    for entry in entries {
        let capabilities = entry["capabilities"].as_object().unwrap();
        for key in [
            "project_instructions",
            "commands",
            "hooks",
            "skill_files",
            "model_config",
            "automated_execution",
        ] {
            assert!(capabilities.contains_key(key), "missing {key}");
        }
    }
    let codex = entries.iter().find(|entry| entry["id"] == "codex").unwrap();
    assert_eq!(codex["capabilities"]["commands"], false);
}
