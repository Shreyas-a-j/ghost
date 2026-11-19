use crate::answers::Answers;
use crate::opencode;
use crate::render::GeneratedFile;
use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterId {
    OpenCode,
    Claude,
    Codex,
}

impl AdapterId {
    pub const ALL: [Self; 3] = [Self::OpenCode, Self::Claude, Self::Codex];

    pub fn id(self) -> &'static str {
        match self {
            Self::OpenCode => "opencode",
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OpenCode => "OpenCode",
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "opencode" => Some(Self::OpenCode),
            "claude" | "claude_code" | "claude-code" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    pub fn from_names(names: &[String]) -> Result<Vec<Self>> {
        let mut adapters = Vec::new();
        for name in names {
            let Some(adapter) = Self::parse(name) else {
                bail!("unknown adapter {name:?}; supported: opencode, claude, codex");
            };
            if !adapters.contains(&adapter) {
                adapters.push(adapter);
            }
        }
        if adapters.is_empty() {
            adapters.push(Self::OpenCode);
        }
        adapters.sort();
        Ok(adapters)
    }

    pub fn names(adapters: &[Self]) -> Vec<String> {
        adapters
            .iter()
            .map(|adapter| adapter.id().to_string())
            .collect()
    }

    pub fn supports_commands(self) -> bool {
        implementation(self).capabilities().commands
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub hint: &'static str,
}

/// Workflow commands shared by every command-capable adapter. The body
/// templates live in `templates/commands/`; adapters only decide the
/// frontmatter and destination directory.
pub const COMMANDS: [CommandSpec; 7] = [
    CommandSpec {
        name: "plan",
        description: "Fill the project specs and pin versions",
        hint: "",
    },
    CommandSpec {
        name: "build",
        description: "Implement one feature spec",
        hint: "<feature-slug>",
    },
    CommandSpec {
        name: "fix",
        description: "Find the root cause and make the smallest repair",
        hint: "<symptom or log excerpt>",
    },
    CommandSpec {
        name: "test",
        description: "Add tests for one feature's scenarios",
        hint: "<feature-slug>",
    },
    CommandSpec {
        name: "review",
        description: "Review the current diff against the relevant spec",
        hint: "",
    },
    CommandSpec {
        name: "handoff",
        description: "Write the current state for a fresh session",
        hint: "",
    },
    CommandSpec {
        name: "checkpoint",
        description: "Commit a local checkpoint",
        hint: "[message]",
    },
];

#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub project_instructions: bool,
    pub commands: bool,
    pub hooks: bool,
    pub skill_files: bool,
    pub model_config: bool,
    pub automated_execution: bool,
}

impl Capabilities {
    /// Short, stable capability list for `ghost adapters`.
    pub fn summary(&self) -> Vec<&'static str> {
        let mut features = Vec::new();
        if self.project_instructions {
            features.push("instructions");
        }
        if self.commands {
            features.push("commands");
        }
        if self.hooks {
            features.push("hooks");
        }
        if self.skill_files {
            features.push("skill-files");
        }
        if self.model_config {
            features.push("model-config");
        }
        if self.automated_execution {
            features.push("automated-execution");
        }
        features
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DetectionResult {
    pub installed: bool,
    pub version: Option<String>,
    pub configured: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdapterInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub detection: DetectionResult,
    pub capabilities: Capabilities,
}

/// An agent integration. Implementations own only their surface conventions:
/// where instructions and commands live, how command frontmatter is shaped,
/// and which config files they accept. Shared views are rendered once by
/// `render::generate_for`.
pub trait Adapter: Sync {
    fn id(&self) -> AdapterId;
    fn capabilities(&self) -> Capabilities;
    fn detect(&self, root: &Path) -> DetectionResult;
    fn instruction_file(&self) -> Option<&'static str>;
    fn commands_dir(&self) -> Option<&'static str>;
    fn command_header(
        &self,
        spec: &CommandSpec,
        answers: &Answers,
        agents: &BTreeSet<String>,
    ) -> String;
    fn config_files(&self, answers: &Answers) -> Result<Vec<GeneratedFile>>;
}

pub fn implementations() -> &'static [&'static dyn Adapter] {
    &[&OpenCodeAdapter, &ClaudeAdapter, &CodexAdapter]
}

pub fn implementation(id: AdapterId) -> &'static dyn Adapter {
    implementations()
        .iter()
        .copied()
        .find(|adapter| adapter.id() == id)
        .expect("every AdapterId has an implementation")
}

#[cfg(test)]
pub fn capabilities(id: AdapterId) -> Capabilities {
    implementation(id).capabilities()
}

pub fn detect_all(root: &Path) -> Vec<AdapterInfo> {
    implementations()
        .iter()
        .map(|adapter| {
            let id = adapter.id();
            AdapterInfo {
                id: id.id(),
                label: id.label(),
                detection: adapter.detect(root),
                capabilities: adapter.capabilities(),
            }
        })
        .collect()
}

struct OpenCodeAdapter;

impl Adapter for OpenCodeAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::OpenCode
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            project_instructions: true,
            commands: true,
            hooks: false,
            skill_files: false,
            model_config: true,
            automated_execution: false,
        }
    }

    fn detect(&self, root: &Path) -> DetectionResult {
        detect_with(
            "opencode",
            opencode::config_dir(),
            root.join(".opencode").is_dir(),
        )
    }

    fn instruction_file(&self) -> Option<&'static str> {
        Some("AGENTS.md")
    }

    fn commands_dir(&self) -> Option<&'static str> {
        Some(opencode::COMMANDS_DIR)
    }

    fn command_header(
        &self,
        spec: &CommandSpec,
        answers: &Answers,
        agents: &BTreeSet<String>,
    ) -> String {
        let mut header = format!("---\ndescription: {}\n", spec.description);
        if let Some(agent) = opencode::agent_for(spec.name, agents) {
            header.push_str(&format!("agent: {agent}\n"));
        }
        if let Some(model) = answers.model_line() {
            header.push_str(&format!("model: {model}\n"));
        }
        header.push_str("---\n");
        header
    }

    fn config_files(&self, answers: &Answers) -> Result<Vec<GeneratedFile>> {
        let Some(model) = answers.model_line() else {
            return Ok(Vec::new());
        };
        let config = serde_json::json!({
            "$schema": opencode::CONFIG_SCHEMA,
            "model": model,
        });
        Ok(vec![GeneratedFile::new(
            opencode::PROJECT_CONFIG,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&config)
                    .map_err(|err| anyhow::anyhow!("could not render opencode.json: {err}"))?
            ),
        )])
    }
}

struct ClaudeAdapter;

impl Adapter for ClaudeAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::Claude
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            project_instructions: true,
            commands: true,
            hooks: false,
            skill_files: false,
            model_config: false,
            automated_execution: false,
        }
    }

    fn detect(&self, root: &Path) -> DetectionResult {
        detect_with(
            "claude",
            home_dir().join(".claude"),
            root.join(".claude").is_dir(),
        )
    }

    fn instruction_file(&self) -> Option<&'static str> {
        Some("CLAUDE.md")
    }

    fn commands_dir(&self) -> Option<&'static str> {
        Some(".claude/commands")
    }

    fn command_header(
        &self,
        spec: &CommandSpec,
        _answers: &Answers,
        _agents: &BTreeSet<String>,
    ) -> String {
        let mut header = format!("---\ndescription: {}\n", spec.description);
        if !spec.hint.is_empty() {
            header.push_str(&format!("argument-hint: {}\n", spec.hint));
        }
        header.push_str("---\n");
        header
    }

    fn config_files(&self, _answers: &Answers) -> Result<Vec<GeneratedFile>> {
        Ok(Vec::new())
    }
}

struct CodexAdapter;

impl Adapter for CodexAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::Codex
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            project_instructions: true,
            commands: false,
            hooks: false,
            skill_files: false,
            model_config: false,
            automated_execution: false,
        }
    }

    fn detect(&self, root: &Path) -> DetectionResult {
        detect_with(
            "codex",
            home_dir().join(".codex"),
            root.join("AGENTS.md").is_file(),
        )
    }

    fn instruction_file(&self) -> Option<&'static str> {
        Some("AGENTS.md")
    }

    fn commands_dir(&self) -> Option<&'static str> {
        None
    }

    fn command_header(
        &self,
        spec: &CommandSpec,
        _answers: &Answers,
        _agents: &BTreeSet<String>,
    ) -> String {
        format!("---\ndescription: {}\n---\n", spec.description)
    }

    fn config_files(&self, _answers: &Answers) -> Result<Vec<GeneratedFile>> {
        Ok(Vec::new())
    }
}

fn detect_with(binary: &str, config_dir: PathBuf, project_marker: bool) -> DetectionResult {
    let version = command_version(binary);
    let installed = version.is_some();
    let configured = config_dir.is_dir() || project_marker;
    let detail = match (installed, configured) {
        (true, true) => format!("found {binary} and project/user configuration"),
        (true, false) => format!("found {binary}; no project configuration yet"),
        (false, true) => format!("{binary} not on PATH; configuration exists"),
        (false, false) => format!("{binary} not found; no configuration"),
    };
    DetectionResult {
        installed,
        version,
        configured,
        detail,
    }
}

fn command_version(binary: &str) -> Option<String> {
    let output = Command::new(binary).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_deduplicates_adapter_names() {
        let names = vec![
            "opencode".to_string(),
            "claude".to_string(),
            "claude-code".to_string(),
            "codex".to_string(),
        ];
        let adapters = AdapterId::from_names(&names).unwrap();
        assert_eq!(
            adapters,
            vec![AdapterId::OpenCode, AdapterId::Claude, AdapterId::Codex]
        );
        assert_eq!(
            AdapterId::names(&adapters),
            vec!["opencode", "claude", "codex"]
        );
        assert!(AdapterId::from_names(&["unknown".to_string()]).is_err());
        assert_eq!(
            AdapterId::from_names(&[]).unwrap(),
            vec![AdapterId::OpenCode]
        );
    }

    #[test]
    fn capabilities_do_not_claim_parity() {
        assert!(capabilities(AdapterId::OpenCode).commands);
        assert!(capabilities(AdapterId::Claude).commands);
        assert!(!capabilities(AdapterId::Codex).commands);
        assert!(!capabilities(AdapterId::Codex).hooks);
        assert!(!capabilities(AdapterId::Claude).model_config);
        for adapter in AdapterId::ALL {
            assert!(capabilities(adapter).project_instructions);
            assert!(!capabilities(adapter).automated_execution);
        }
    }

    #[test]
    fn every_id_resolves_to_one_implementation() {
        for id in AdapterId::ALL {
            let implementation = implementation(id);
            assert_eq!(implementation.id(), id);
            assert!(implementation.capabilities().project_instructions);
        }
        assert_eq!(implementations().len(), AdapterId::ALL.len());
        let unique: std::collections::BTreeSet<_> = implementations()
            .iter()
            .map(|adapter| adapter.id())
            .collect();
        assert_eq!(unique.len(), implementations().len());
    }

    #[test]
    fn command_surfaces_match_capabilities() {
        for id in AdapterId::ALL {
            let adapter = implementation(id);
            assert_eq!(
                adapter.commands_dir().is_some(),
                adapter.capabilities().commands,
                "{}",
                id.id()
            );
            assert_eq!(
                adapter.instruction_file().is_some(),
                adapter.capabilities().project_instructions,
                "{}",
                id.id()
            );
        }
    }
}
