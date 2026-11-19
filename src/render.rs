use crate::adapter::{implementation, Adapter, AdapterId, COMMANDS};
use crate::answers::{slugify, Answers, Stack};
use anyhow::{Context, Result};
use minijinja::{Environment, Value};
use serde::Serialize;
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const INSTRUCTIONS_TEMPLATE: &str = include_str!("../templates/AGENTS.md.j2");
const STATE_TEMPLATE: &str = include_str!("../templates/STATE.md.j2");
const OVERVIEW_TEMPLATE: &str = include_str!("../templates/overview.md.j2");
const FEATURE_TEMPLATE: &str = include_str!("../templates/feature.md.j2");
const SCHEMA_TEMPLATE: &str = include_str!("../templates/schema.yaml.j2");
const CHECK_TEMPLATE: &str = include_str!("../templates/check.sh.j2");
const DEV_TEMPLATE: &str = include_str!("../templates/dev.sh.j2");

const COMMAND_TEMPLATES: [(&str, &str); 7] = [
    ("plan", include_str!("../templates/commands/plan.md.j2")),
    ("build", include_str!("../templates/commands/build.md.j2")),
    ("fix", include_str!("../templates/commands/fix.md.j2")),
    ("test", include_str!("../templates/commands/test.md.j2")),
    ("review", include_str!("../templates/commands/review.md.j2")),
    (
        "handoff",
        include_str!("../templates/commands/handoff.md.j2"),
    ),
    (
        "checkpoint",
        include_str!("../templates/commands/checkpoint.md.j2"),
    ),
];

pub const GITIGNORE_LINES: [&str; 4] = [".env", ".env.*", ".ghost/dev.pid", ".ghost/dev.log"];

#[derive(Debug, Clone)]
pub struct GeneratedFile {
    pub path: PathBuf,
    pub content: String,
    pub executable: bool,
}

impl GeneratedFile {
    pub(crate) fn new(path: impl Into<PathBuf>, content: String) -> Self {
        let mut content = content;
        if !content.ends_with('\n') {
            content.push('\n');
        }
        Self {
            path: path.into(),
            content,
            executable: false,
        }
    }

    fn executable(mut self) -> Self {
        self.executable = true;
        self
    }
}

#[derive(Serialize, Clone)]
pub struct FeatureView {
    pub title: String,
    pub slug: String,
    pub demo_step: String,
}

#[derive(Serialize, Clone)]
struct View {
    project_name: String,
    pitch: String,
    mode_label: String,
    mode_line: String,
    stack_line: String,
    event_name: String,
    team_size: String,
    deadline: String,
    demo_steps: Vec<String>,
    must_haves: Vec<FeatureView>,
    nice_to_haves: Vec<String>,
    risky_areas: String,
    test_level_label: String,
    existing_code: bool,
    skills_line: String,
    hackathon: bool,
    run_cmd: String,
    run_cmd_quoted: String,
    checkpoint_before: bool,
    is_rust: bool,
    is_python: bool,
    is_ts: bool,
    is_other: bool,
    run_tests: bool,
    has_commands: bool,
    instructions_file: String,
}

impl View {
    fn from_answers(answers: &Answers, has_commands: bool) -> Self {
        let must_haves = answers
            .must_haves
            .iter()
            .enumerate()
            .map(|(index, title)| FeatureView {
                title: title.clone(),
                slug: slugify(&answers.must_haves[index]),
                demo_step: answers
                    .demo_steps
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| "<fill in>".to_string()),
            })
            .collect();
        View {
            project_name: answers.project_name.clone(),
            pitch: answers.pitch.clone(),
            mode_label: answers.mode.label().to_string(),
            mode_line: answers.mode.line().to_string(),
            stack_line: answers.stack_line(),
            event_name: answers.event_name.clone(),
            team_size: answers.team_size.label().to_string(),
            deadline: answers.deadline.clone().unwrap_or_default(),
            demo_steps: answers.demo_steps.clone(),
            must_haves,
            nice_to_haves: answers.nice_to_haves.clone(),
            risky_areas: answers.risky_label(),
            test_level_label: answers.test_level.label().to_string(),
            existing_code: answers.existing_code,
            skills_line: answers.skills_label(),
            hackathon: answers.mode == crate::answers::Mode::Hackathon,
            run_cmd: answers.run_cmd.clone(),
            run_cmd_quoted: shell_quote(&answers.run_cmd),
            checkpoint_before: answers.checkpoint_freq
                == crate::answers::CheckpointFreq::BeforeEvery,
            is_rust: answers.stack == Stack::Rust,
            is_python: answers.stack == Stack::Python,
            is_ts: answers.stack == Stack::TypeScriptNode,
            is_other: answers.stack == Stack::Other,
            run_tests: answers.test_level.runs_tests(),
            has_commands,
            instructions_file: "AGENTS.md".to_string(),
        }
    }
}

#[derive(Serialize)]
struct InstructionView<'a> {
    #[serde(flatten)]
    base: &'a View,
    command_layout: String,
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn environment() -> Result<Environment<'static>> {
    let mut env = Environment::new();
    env.set_syntax(
        minijinja::syntax::SyntaxConfig::builder()
            .trim_blocks(true)
            .lstrip_blocks(true)
            .build()
            .context("invalid template syntax config")?,
    );
    env.add_template("instructions", INSTRUCTIONS_TEMPLATE)
        .context("invalid instructions template")?;
    env.add_template("state", STATE_TEMPLATE)
        .context("invalid STATE template")?;
    env.add_template("overview", OVERVIEW_TEMPLATE)
        .context("invalid overview template")?;
    env.add_template("feature", FEATURE_TEMPLATE)
        .context("invalid feature template")?;
    env.add_template("schema", SCHEMA_TEMPLATE)
        .context("invalid schema template")?;
    env.add_template("check", CHECK_TEMPLATE)
        .context("invalid check template")?;
    env.add_template("dev", DEV_TEMPLATE)
        .context("invalid dev template")?;
    for (name, source) in COMMAND_TEMPLATES {
        env.add_template_owned(format!("cmd-{name}"), source)
            .with_context(|| format!("invalid {name} command template"))?;
    }
    Ok(env)
}

fn render(env: &Environment<'_>, name: &str, ctx: Value) -> Result<String> {
    env.get_template(name)?
        .render(ctx)
        .with_context(|| format!("could not render {name}"))
}

fn common_files(env: &Environment<'_>, view: &View) -> Result<Vec<GeneratedFile>> {
    let ctx = Value::from(minijinja::value::Serde(view));
    let mut files = Vec::new();
    files.push(GeneratedFile::new(
        "STATE.md",
        render(env, "state", ctx.clone())?,
    ));
    files.push(GeneratedFile::new(
        "specs/00-overview.md",
        render(env, "overview", ctx.clone())?,
    ));
    for feature in &view.must_haves {
        let feature_ctx = Value::from(minijinja::value::Serde(serde_json::json!({
            "project_name": view.project_name.clone(),
            "mode_label": view.mode_label.clone(),
            "hackathon": view.hackathon,
            "feature": feature,
        })));
        let content = render(env, "feature", feature_ctx)
            .with_context(|| format!("could not render feature {}", feature.slug))?;
        files.push(GeneratedFile::new(
            format!("specs/feature-{}.md", feature.slug),
            content,
        ));
    }
    files.push(GeneratedFile::new(
        "specs/schema.yaml",
        render(env, "schema", ctx.clone())?,
    ));
    files.push(
        GeneratedFile::new("scripts/check.sh", render(env, "check", ctx.clone())?).executable(),
    );
    files.push(GeneratedFile::new("scripts/dev.sh", render(env, "dev", ctx)?).executable());
    Ok(files)
}

fn instruction_file(
    env: &Environment<'_>,
    view: &View,
    path: &str,
    command_layout: &str,
) -> Result<GeneratedFile> {
    let ctx = Value::from(minijinja::value::Serde(&InstructionView {
        base: view,
        command_layout: command_layout.to_string(),
    }));
    Ok(GeneratedFile::new(path, render(env, "instructions", ctx)?))
}

fn command_files(
    env: &Environment<'_>,
    view: &View,
    answers: &Answers,
    agents: &BTreeSet<String>,
    adapter: &dyn Adapter,
) -> Result<Vec<GeneratedFile>> {
    let Some(directory) = adapter.commands_dir() else {
        return Ok(Vec::new());
    };
    let mut view = view.clone();
    view.instructions_file = adapter
        .instruction_file()
        .unwrap_or("AGENTS.md")
        .to_string();
    let ctx = Value::from(minijinja::value::Serde(&view));
    let mut files = Vec::new();
    for spec in COMMANDS {
        let body = render(env, &format!("cmd-{}", spec.name), ctx.clone())?;
        let header = adapter.command_header(&spec, answers, agents);
        files.push(GeneratedFile::new(
            format!("{directory}/{}.md", spec.name),
            format!("{header}{body}"),
        ));
    }
    Ok(files)
}

fn ordered_adapters(adapters: &[AdapterId]) -> Vec<AdapterId> {
    let mut ordered: Vec<AdapterId> = AdapterId::ALL
        .iter()
        .copied()
        .filter(|adapter| adapters.contains(adapter))
        .collect();
    if ordered.is_empty() {
        ordered.push(AdapterId::OpenCode);
    }
    ordered
}

#[cfg(test)]
pub fn generate(answers: &Answers, agents: &BTreeSet<String>) -> Result<Vec<GeneratedFile>> {
    generate_for(&[AdapterId::OpenCode], answers, agents)
}

pub fn generate_for(
    adapters: &[AdapterId],
    answers: &Answers,
    agents: &BTreeSet<String>,
) -> Result<Vec<GeneratedFile>> {
    let env = environment()?;
    let selected: Vec<&dyn Adapter> = ordered_adapters(adapters)
        .into_iter()
        .map(implementation)
        .collect();
    let has_commands = selected
        .iter()
        .any(|adapter| adapter.capabilities().commands);
    let view = View::from_answers(answers, has_commands);
    let mut files = Vec::new();

    // OpenCode and Codex share AGENTS.md; emit each instruction file once and
    // mention a command directory only when the adapter that owns it has one.
    let mut emitted = HashSet::new();
    for adapter in &selected {
        let Some(path) = adapter.instruction_file() else {
            continue;
        };
        if !emitted.insert(path) {
            continue;
        }
        let layout = adapter
            .commands_dir()
            .or_else(|| {
                selected
                    .iter()
                    .filter(|other| other.instruction_file() == Some(path))
                    .find_map(|other| other.commands_dir())
            })
            .unwrap_or("");
        files.push(instruction_file(&env, &view, path, layout)?);
    }

    files.extend(common_files(&env, &view)?);

    for adapter in &selected {
        files.extend(command_files(&env, &view, answers, agents, *adapter)?);
        files.extend(adapter.config_files(answers)?);
    }

    let mut seen = HashSet::new();
    files.retain(|file| seen.insert(file.path.clone()));
    Ok(files)
}

pub fn existing_targets(root: &Path, files: &[GeneratedFile]) -> Vec<String> {
    let mut found: Vec<String> = files
        .iter()
        .filter(|file| root.join(&file.path).exists())
        .map(|file| file.path.display().to_string())
        .collect();
    found.sort();
    found
}

pub fn write_files(root: &Path, files: &[GeneratedFile]) -> Result<()> {
    for file in files {
        let path = crate::persist::safe_target(root, &file.path)?;
        crate::persist::atomic_write(&path, file.content.as_bytes(), file.executable)
            .with_context(|| format!("could not write {}", path.display()))?;
    }
    Ok(())
}

/// Stable manifest entries for a generated file set.
pub fn manifest_entries(files: &[GeneratedFile]) -> Vec<(String, String)> {
    files
        .iter()
        .map(|file| {
            let path = file.path.to_string_lossy().replace('\\', "/");
            (path, crate::persist::sha256_hex(file.content.as_bytes()))
        })
        .collect()
}

pub fn append_gitignore(root: &Path) -> Result<()> {
    let path = root.join(".gitignore");
    let existing = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err).with_context(|| format!("could not read {}", path.display())),
    };
    let mut present: HashSet<&str> = existing.lines().map(str::trim).collect();
    let mut additions = Vec::new();
    for line in GITIGNORE_LINES {
        if present.insert(line) {
            additions.push(line);
        }
    }
    if additions.is_empty() {
        return Ok(());
    }
    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    for line in additions {
        content.push_str(line);
        content.push('\n');
    }
    let target = crate::persist::safe_target(root, Path::new(".gitignore"))?;
    crate::persist::atomic_write(&target, content.as_bytes(), false)
        .with_context(|| format!("could not write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::answers::{Answers, CheckpointFreq, Mode, RiskArea, Stack, TeamSize, TestLevel};
    use chrono::{Local, TimeZone};
    use std::collections::BTreeSet;

    fn sample(stack: Stack, mode: Mode) -> Answers {
        Answers {
            project_name: "Orbit".into(),
            pitch: "A tiny dashboard for demo day.".into(),
            demo_steps: vec![
                "Open the dashboard".into(),
                "Add a task".into(),
                "See progress".into(),
            ],
            event_name: "HackMIT 2026".into(),
            deadline: Some("2099-01-01 10:00".into()),
            team_size: TeamSize::Solo,
            mode,
            stack,
            stack_details: "axum 0.8".into(),
            run_cmd: "cargo run".into(),
            must_haves: vec!["Sign in".into(), "Task board".into(), "Export".into()],
            nice_to_haves: vec!["Dark mode".into()],
            existing_code: false,
            risky_areas: vec![RiskArea::Auth],
            test_level: TestLevel::Smoke,
            model_override: None,
            skills: vec![],
            checkpoint_freq: CheckpointFreq::BeforeEvery,
            created_at: Local.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap(),
        }
    }

    fn assert_clean(files: &[GeneratedFile]) {
        for file in files {
            assert!(
                !file.content.contains("ALWAYS"),
                "{} contains ALWAYS",
                file.path.display()
            );
            assert!(
                !file.content.contains("NEVER"),
                "{} contains NEVER",
                file.path.display()
            );
        }
        if let Some(agents) = files
            .iter()
            .find(|f| f.path == Path::new("AGENTS.md") || f.path == Path::new("CLAUDE.md"))
        {
            assert!(
                agents.content.lines().count() <= 40,
                "{} has {} lines",
                agents.path.display(),
                agents.content.lines().count()
            );
        }
    }

    fn find<'a>(files: &'a [GeneratedFile], path: &str) -> &'a GeneratedFile {
        files
            .iter()
            .find(|file| file.path == Path::new(path))
            .unwrap_or_else(|| panic!("missing {path}"))
    }

    #[test]
    fn agents_stays_short_across_combinations() {
        let empty = BTreeSet::new();
        for stack in [
            Stack::Rust,
            Stack::Python,
            Stack::TypeScriptNode,
            Stack::Other,
        ] {
            for mode in [Mode::Hackathon, Mode::Normal] {
                for existing in [false, true] {
                    for level in [TestLevel::None, TestLevel::Smoke, TestLevel::PerFeature] {
                        for model in [None, Some("provider/model".to_string())] {
                            for skills in [vec![], vec!["debugging".into(), "tdd".into()]] {
                                let mut answers = sample(stack, mode);
                                answers.existing_code = existing;
                                answers.test_level = level;
                                answers.model_override = model.clone();
                                answers.skills = skills;
                                let files = generate(&answers, &empty).unwrap();
                                assert_clean(&files);
                                if model.is_none() {
                                    for file in &files {
                                        assert!(!file.content.contains("model:"));
                                        assert_ne!(file.path, Path::new("opencode.json"));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn writes_expected_files_for_each_stack() {
        for (stack, mode) in [
            (Stack::Rust, Mode::Hackathon),
            (Stack::Python, Mode::Normal),
            (Stack::TypeScriptNode, Mode::Hackathon),
            (Stack::Other, Mode::Normal),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let files = generate(&sample(stack, mode), &BTreeSet::new()).unwrap();
            write_files(dir.path(), &files).unwrap();
            for path in [
                "AGENTS.md",
                "STATE.md",
                "specs/00-overview.md",
                "specs/feature-sign-in.md",
                "specs/feature-task-board.md",
                "specs/feature-export.md",
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
            ] {
                assert!(dir.path().join(path).exists(), "missing {path}");
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                for script in ["scripts/check.sh", "scripts/dev.sh"] {
                    let mode = fs::metadata(dir.path().join(script))
                        .unwrap()
                        .permissions()
                        .mode();
                    assert_eq!(mode & 0o111, 0o111, "{script} is not executable");
                }
            }
            for command in [
                "plan",
                "build",
                "fix",
                "test",
                "review",
                "handoff",
                "checkpoint",
            ] {
                let path = dir.path().join(format!(".opencode/commands/{command}.md"));
                let content = fs::read_to_string(path).unwrap();
                assert!(
                    content.lines().count() < 25,
                    "{command} command is too long"
                );
            }
        }
    }

    #[test]
    fn model_override_is_used_only_when_set() {
        let mut answers = sample(Stack::Rust, Mode::Normal);
        answers.model_override = Some("provider/model".into());
        let files = generate(&answers, &BTreeSet::new()).unwrap();
        let build = find(&files, ".opencode/commands/build.md");
        assert!(build.content.contains("model: provider/model"));
        let config = find(&files, "opencode.json");
        let parsed: serde_json::Value = serde_json::from_str(&config.content).unwrap();
        assert_eq!(parsed["model"], "provider/model");
        assert_eq!(parsed["$schema"], crate::opencode::CONFIG_SCHEMA);
    }

    #[test]
    fn agent_lines_follow_detected_agents() {
        let agents: BTreeSet<String> = ["build".to_string(), "reviewer".to_string()]
            .into_iter()
            .collect();
        let files = generate(&sample(Stack::Rust, Mode::Normal), &agents).unwrap();
        assert!(find(&files, ".opencode/commands/build.md")
            .content
            .contains("agent: build"));
        assert!(find(&files, ".opencode/commands/review.md")
            .content
            .contains("agent: reviewer"));
        assert!(!find(&files, ".opencode/commands/test.md")
            .content
            .contains("agent:"));
    }

    #[test]
    fn claude_adapter_writes_instructions_and_commands() {
        let files = generate_for(
            &[AdapterId::Claude],
            &sample(Stack::Rust, Mode::Normal),
            &BTreeSet::new(),
        )
        .unwrap();
        let claude = find(&files, "CLAUDE.md");
        assert!(claude.content.contains(".claude/commands"));
        assert!(!files.iter().any(|file| file.path == Path::new("AGENTS.md")));
        let build = find(&files, ".claude/commands/build.md");
        assert!(build
            .content
            .contains("description: Implement one feature spec"));
        assert!(build.content.contains("argument-hint: <feature-slug>"));
        assert!(build.content.contains("$ARGUMENTS"));
        assert!(!build.content.contains("agent:"));
        assert!(!files
            .iter()
            .any(|file| file.path == Path::new("opencode.json")));
    }

    #[test]
    fn codex_adapter_writes_agents_md_without_project_commands() {
        let files = generate_for(
            &[AdapterId::Codex],
            &sample(Stack::Rust, Mode::Normal),
            &BTreeSet::new(),
        )
        .unwrap();
        let agents = find(&files, "AGENTS.md");
        assert!(!agents.content.contains(".opencode/commands"));
        assert!(!agents.content.contains(".claude/commands"));
        assert!(!files
            .iter()
            .any(|file| file.path.starts_with(".opencode") || file.path.starts_with(".claude")));
        let state = find(&files, "STATE.md");
        assert!(!state.content.contains("`/plan`"));
        assert!(state.content.contains("Read `specs/00-overview.md`"));
        assert!(!files.iter().any(|file| file.path == Path::new("CLAUDE.md")));
    }

    #[test]
    fn combined_adapters_share_common_files_without_duplicates() {
        let files = generate_for(
            &[AdapterId::OpenCode, AdapterId::Claude, AdapterId::Codex],
            &sample(Stack::Rust, Mode::Normal),
            &BTreeSet::new(),
        )
        .unwrap();
        let mut paths = HashSet::new();
        for file in &files {
            assert!(
                paths.insert(file.path.clone()),
                "duplicate {}",
                file.path.display()
            );
        }
        for path in [
            "AGENTS.md",
            "CLAUDE.md",
            ".opencode/commands/build.md",
            ".claude/commands/build.md",
            "STATE.md",
        ] {
            find(&files, path);
        }
        let state = find(&files, "STATE.md");
        assert!(state.content.contains("`/plan`"));
    }

    #[test]
    fn check_script_matches_stack_and_test_level() {
        let render_script = |stack, level| {
            let mut answers = sample(stack, Mode::Normal);
            answers.test_level = level;
            generate(&answers, &BTreeSet::new())
                .unwrap()
                .into_iter()
                .find(|f| f.path == Path::new("scripts/check.sh"))
                .unwrap()
                .content
        };
        let rust = render_script(Stack::Rust, TestLevel::Smoke);
        assert!(rust.contains("cargo fmt --check"));
        assert!(rust.contains("cargo clippy"));
        assert!(rust.contains("cargo test"));
        let rust_none = render_script(Stack::Rust, TestLevel::None);
        assert!(!rust_none.contains("cargo test"));
        assert!(rust_none.contains("skip test"));
        assert!(render_script(Stack::Python, TestLevel::Smoke).contains("ruff check"));
        assert!(render_script(Stack::TypeScriptNode, TestLevel::Smoke).contains("npm test"));
        assert!(render_script(Stack::Other, TestLevel::Smoke).contains("add your own stages"));
    }

    #[test]
    fn gitignore_append_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
        append_gitignore(dir.path()).unwrap();
        append_gitignore(dir.path()).unwrap();
        let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert_eq!(content.matches(".ghost/dev.log").count(), 1);
        assert!(content.starts_with("target/\n"));
        assert!(content.contains(".env.*\n"));
    }
}
