use crate::answers::{slugify, Answers, Stack};
use crate::opencode;
use anyhow::{Context, Result};
use minijinja::{Environment, Value};
use serde::Serialize;
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const AGENTS_TEMPLATE: &str = include_str!("../templates/AGENTS.md.j2");
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
    fn new(path: impl Into<PathBuf>, content: String) -> Self {
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

#[derive(Serialize)]
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
}

impl View {
    fn from_answers(answers: &Answers) -> Self {
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
            deadline: answers.deadline.clone(),
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
        }
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(Serialize)]
struct CommandView<'a> {
    #[serde(flatten)]
    base: &'a View,
    agent_line: String,
    model_line: String,
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
    env.add_template("agents", AGENTS_TEMPLATE)
        .context("invalid AGENTS template")?;
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

pub fn generate(answers: &Answers, agents: &BTreeSet<String>) -> Result<Vec<GeneratedFile>> {
    let env = environment()?;
    let view = View::from_answers(answers);
    let ctx = Value::from(minijinja::value::Serde(&view));

    let render = |name: &str| -> Result<String> {
        env.get_template(name)?
            .render(ctx.clone())
            .with_context(|| format!("could not render {name}"))
    };

    let mut files = Vec::new();
    files.push(GeneratedFile::new("AGENTS.md", render("agents")?));
    files.push(GeneratedFile::new("STATE.md", render("state")?));
    files.push(GeneratedFile::new(
        "specs/00-overview.md",
        render("overview")?,
    ));
    for feature in &view.must_haves {
        let feature_ctx = Value::from(minijinja::value::Serde(serde_json::json!({
            "project_name": view.project_name.clone(),
            "mode_label": view.mode_label.clone(),
            "feature": feature,
        })));
        let content = env
            .get_template("feature")?
            .render(feature_ctx)
            .with_context(|| format!("could not render feature {}", feature.slug))?;
        files.push(GeneratedFile::new(
            format!("specs/feature-{}.md", feature.slug),
            content,
        ));
    }
    files.push(GeneratedFile::new("specs/schema.yaml", render("schema")?));
    files.push(GeneratedFile::new("scripts/check.sh", render("check")?).executable());
    files.push(GeneratedFile::new("scripts/dev.sh", render("dev")?).executable());

    for (name, _) in COMMAND_TEMPLATES {
        let agent = opencode::agent_for(name, agents);
        let command_view = CommandView {
            base: &view,
            agent_line: agent
                .map(|agent| format!("agent: {agent}\n"))
                .unwrap_or_default(),
            model_line: answers
                .model_line()
                .map(|model| format!("model: {model}\n"))
                .unwrap_or_default(),
        };
        let template_name = format!("cmd-{name}");
        let content = env
            .get_template(&template_name)?
            .render(Value::from(minijinja::value::Serde(&command_view)))
            .with_context(|| format!("could not render command {name}"))?;
        files.push(GeneratedFile::new(
            format!("{}/{name}.md", opencode::COMMANDS_DIR),
            content,
        ));
    }

    if let Some(model) = answers.model_line() {
        let config = serde_json::json!({
            "$schema": opencode::CONFIG_SCHEMA,
            "model": model,
        });
        files.push(GeneratedFile::new(
            opencode::PROJECT_CONFIG,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&config).context("could not render opencode.json")?
            ),
        ));
    }

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
        let path = root.join(&file.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        fs::write(&path, &file.content)
            .with_context(|| format!("could not write {}", path.display()))?;
        if file.executable {
            set_executable(&path)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
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
    fs::write(&path, content).with_context(|| format!("could not write {}", path.display()))?;
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
            deadline: "2099-01-01 10:00".into(),
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
        let agents = files
            .iter()
            .find(|f| f.path == Path::new("AGENTS.md"))
            .unwrap();
        assert!(
            agents.content.lines().count() <= 40,
            "AGENTS.md has {} lines",
            agents.content.lines().count()
        );
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
        let build = files
            .iter()
            .find(|f| f.path == Path::new(".opencode/commands/build.md"))
            .unwrap();
        assert!(build.content.contains("model: provider/model"));
        let config = files
            .iter()
            .find(|f| f.path == Path::new("opencode.json"))
            .unwrap();
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
        let find = |path: &str| {
            files
                .iter()
                .find(|f| f.path == Path::new(path))
                .unwrap()
                .content
                .clone()
        };
        assert!(find(".opencode/commands/build.md").contains("agent: build"));
        assert!(find(".opencode/commands/review.md").contains("agent: reviewer"));
        assert!(!find(".opencode/commands/test.md").contains("agent:"));
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
