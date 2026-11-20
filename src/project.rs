use crate::answers::{Answers, CheckpointFreq, Mode, RiskArea, Stack, TeamSize, TestLevel};
use anyhow::Result;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFacts {
    pub revision: Option<String>,
    pub branch: Option<String>,
    pub dirty: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectFacts {
    pub name: String,
    pub stacks: Vec<Stack>,
    pub manifests: Vec<String>,
    pub run_cmd: String,
    pub node_manager: Option<String>,
    pub has_code: bool,
    pub git: Option<GitFacts>,
}

impl ProjectFacts {
    pub fn primary_stack(&self) -> Stack {
        self.stacks.first().copied().unwrap_or(Stack::Other)
    }

    pub fn stack_details(&self) -> String {
        match self.primary_stack() {
            Stack::Rust => "Cargo".to_string(),
            Stack::Python => "Python".to_string(),
            Stack::TypeScriptNode => self
                .node_manager
                .as_deref()
                .map(|manager| format!("Node ({manager})"))
                .unwrap_or_else(|| "Node".to_string()),
            Stack::Other => String::new(),
        }
    }

    pub fn languages(&self) -> Vec<String> {
        self.stacks
            .iter()
            .map(|stack| match stack {
                Stack::Rust => "rust",
                Stack::Python => "python",
                Stack::TypeScriptNode => "typescript_node",
                Stack::Other => "other",
            })
            .map(str::to_string)
            .collect()
    }
}

/// Inspect a directory without modifying it.
pub fn discover(dir: &Path) -> Result<ProjectFacts> {
    let root = if dir.exists() {
        dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf())
    } else {
        dir.to_path_buf()
    };
    let mut stacks = Vec::new();
    let mut manifests = Vec::new();

    let rust_manifest = root.join("Cargo.toml");
    if rust_manifest.is_file() {
        stacks.push(Stack::Rust);
        manifests.push("Cargo.toml".to_string());
    }

    let mut python_manifest = None;
    for candidate in ["pyproject.toml", "setup.py", "Pipfile", "requirements.txt"] {
        if root.join(candidate).is_file() {
            python_manifest = Some(candidate);
            break;
        }
    }
    if let Some(manifest) = python_manifest {
        stacks.push(Stack::Python);
        manifests.push(manifest.to_string());
    }

    if root.join("package.json").is_file() {
        stacks.push(Stack::TypeScriptNode);
        manifests.push("package.json".to_string());
    }

    if stacks.is_empty() {
        stacks.push(Stack::Other);
    }

    let name = project_name(&root, &stacks);
    let node_manager = if stacks.contains(&Stack::TypeScriptNode) {
        Some(node_package_manager(&root).to_string())
    } else {
        None
    };
    let facts = ProjectFacts {
        name,
        stacks,
        manifests,
        run_cmd: String::new(),
        node_manager,
        has_code: false,
        git: git_facts(&root),
    };
    let run_cmd = default_run_command(&root, &facts);
    let has_code = !facts.manifests.is_empty()
        || ["src", "app", "lib", "tests", "test"]
            .iter()
            .any(|path| root.join(path).exists());
    Ok(ProjectFacts {
        run_cmd,
        has_code,
        ..facts
    })
}

fn project_name(root: &Path, stacks: &[Stack]) -> String {
    if stacks.contains(&Stack::Rust) {
        if let Some(name) = toml_section_name(&root.join("Cargo.toml"), "package") {
            return name;
        }
    }
    if stacks.contains(&Stack::Python) {
        if let Some(name) = toml_section_name(&root.join("pyproject.toml"), "project")
            .or_else(|| toml_section_name(&root.join("pyproject.toml"), "tool.poetry"))
        {
            return name;
        }
    }
    if stacks.contains(&Stack::TypeScriptNode) {
        if let Some(name) = package_name(&root.join("package.json")) {
            return name;
        }
    }
    root.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("project")
        .to_string()
}

fn toml_section_name(path: &Path, section: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let header = format!("[{section}]");
    let mut in_section = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_section = line == header;
            continue;
        }
        if in_section {
            if let Some(value) = line.strip_prefix("name") {
                let value = value.trim().strip_prefix('=')?.trim();
                return unquote(value);
            }
        }
    }
    None
}

fn package_name(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value
        .get("name")
        .and_then(|name| name.as_str())
        .map(str::to_string)
}

fn unquote(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        Some(value[1..value.len() - 1].to_string())
    } else if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn node_package_manager(root: &Path) -> &'static str {
    if root.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if root.join("yarn.lock").exists() {
        "yarn"
    } else {
        "npm"
    }
}

fn default_run_command(root: &Path, facts: &ProjectFacts) -> String {
    match facts.primary_stack() {
        Stack::Rust => "cargo run".to_string(),
        Stack::TypeScriptNode => {
            let manager = facts.node_manager.as_deref().unwrap_or("npm");
            if package_name(&root.join("package.json")).is_some() {
                let raw = fs::read_to_string(root.join("package.json")).unwrap_or_default();
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) {
                    let scripts = json.get("scripts");
                    if scripts.and_then(|s| s.get("dev")).is_some() {
                        return format!("{manager} run dev");
                    }
                    if scripts.and_then(|s| s.get("start")).is_some() {
                        return format!("{manager} start");
                    }
                }
            }
            String::new()
        }
        Stack::Python => {
            for candidate in ["main.py", "app.py", "manage.py"] {
                if root.join(candidate).is_file() {
                    return format!("python {candidate}");
                }
            }
            String::new()
        }
        Stack::Other => String::new(),
    }
}

fn git_facts(root: &Path) -> Option<GitFacts> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let revision = git_stdout(root, &["rev-parse", "HEAD"]);
    let branch = git_stdout(root, &["rev-parse", "--abbrev-ref", "HEAD"]);
    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .map(|output| output.status.success() && !output.stdout.is_empty())
        .unwrap_or(false);
    Some(GitFacts {
        revision,
        branch,
        dirty,
    })
}

fn git_stdout(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Build a non-hackathon answer set from repository facts.
pub fn generic_answers(facts: &ProjectFacts, name: Option<&str>) -> Answers {
    let project_name = name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| facts.name.clone());
    let stack = facts.primary_stack();
    let pitch = if facts.has_code {
        format!(
            "An existing {} project, documented and coordinated with Ghost.",
            stack.label()
        )
    } else {
        format!("A {} project coordinated with Ghost.", stack.label())
    };
    Answers {
        project_name,
        pitch,
        demo_steps: Vec::new(),
        event_name: String::new(),
        deadline: None,
        team_size: TeamSize::Solo,
        mode: Mode::Normal,
        stack,
        stack_details: facts.stack_details(),
        run_cmd: facts.run_cmd.clone(),
        must_haves: Vec::new(),
        nice_to_haves: Vec::new(),
        existing_code: facts.has_code,
        risky_areas: vec![RiskArea::None],
        test_level: TestLevel::Smoke,
        model_override: None,
        skills: Vec::new(),
        checkpoint_freq: CheckpointFreq::Manual,
        created_at: Local::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_rust_python_and_node() {
        let cases = [
            (
                "Cargo.toml",
                "[package]\nname = \"orbit\"\n",
                Stack::Rust,
                "orbit",
            ),
            (
                "pyproject.toml",
                "[project]\nname = \"orbit\"\n",
                Stack::Python,
                "orbit",
            ),
            (
                "package.json",
                "{\"name\":\"orbit\",\"scripts\":{\"dev\":\"vite\"}}",
                Stack::TypeScriptNode,
                "orbit",
            ),
        ];
        for (manifest, content, expected, name) in cases {
            let dir = tempfile::tempdir().unwrap();
            fs::write(dir.path().join(manifest), content).unwrap();
            let facts = discover(dir.path()).unwrap();
            assert_eq!(facts.primary_stack(), expected);
            assert_eq!(facts.name, name);
            assert!(facts.has_code);
            assert!(facts.manifests.contains(&manifest.to_string()));
        }
    }

    #[test]
    fn detects_node_runner_from_scripts() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"name":"web","scripts":{"dev":"vite"}}"#,
        )
        .unwrap();
        let facts = discover(dir.path()).unwrap();
        assert_eq!(facts.run_cmd, "npm run dev");
        assert_eq!(facts.stack_details(), "Node (npm)");
    }

    #[test]
    fn generic_answers_never_require_hackathon_fields() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"orbit\"\n",
        )
        .unwrap();
        let facts = discover(dir.path()).unwrap();
        let answers = generic_answers(&facts, None);
        assert_eq!(answers.mode, Mode::Normal);
        assert!(answers.deadline.is_none());
        assert!(answers.must_haves.is_empty());
        answers.validate_structure().unwrap();
    }

    #[test]
    fn unknown_repository_is_other() {
        let dir = tempfile::tempdir().unwrap();
        let facts = discover(dir.path()).unwrap();
        assert_eq!(facts.primary_stack(), Stack::Other);
        assert!(!facts.has_code);
    }
}
