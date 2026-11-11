#![allow(dead_code)]

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::PathBuf;

/// Project command directory used by OpenCode (verified in the OpenCode docs).
pub const COMMANDS_DIR: &str = ".opencode/commands";
/// Project configuration file written only when the interview sets a model.
pub const PROJECT_CONFIG: &str = "opencode.json";
pub const CONFIG_SCHEMA: &str = "https://opencode.ai/config.json";

pub const ENV_OPENCODE_DIR: &str = "GHOST_OPENCODE_DIR";
pub const ENV_AGENTS_DIR: &str = "GHOST_AGENTS_DIR";

/// Command name to preferred agent name. The first name is the command.
pub const COMMAND_AGENTS: &[(&str, &str)] = &[
    ("plan", "build"),
    ("build", "build"),
    ("fix", "build"),
    ("handoff", "build"),
    ("test", "tester"),
    ("review", "reviewer"),
    ("checkpoint", "build"),
];

pub fn config_dir() -> PathBuf {
    if let Ok(dir) = env::var(ENV_OPENCODE_DIR) {
        return PathBuf::from(dir);
    }
    home_dir().join(".config/opencode")
}

pub fn agents_dir() -> PathBuf {
    config_dir().join("agents")
}

pub fn skill_dirs() -> Vec<PathBuf> {
    let agents_skills = env::var(ENV_AGENTS_DIR)
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".agents/skills"));
    vec![agents_skills, config_dir().join("skills")]
}

pub fn home_dir() -> PathBuf {
    env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

pub fn detected_agents() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    if let Ok(entries) = fs::read_dir(agents_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.insert(stem.to_string());
                }
            }
        }
    }
    names
}

pub fn agent_for(command: &str, available: &BTreeSet<String>) -> Option<String> {
    let expected = COMMAND_AGENTS
        .iter()
        .find(|(name, _)| *name == command)
        .map(|(_, agent)| *agent)?;
    available.contains(expected).then(|| expected.to_string())
}

/// List directory names only. No skill files are read.
pub fn discover_skills() -> Vec<String> {
    let mut names = BTreeSet::new();
    for dir in skill_dirs() {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    if let Some(name) = entry.file_name().to_str() {
                        names.insert(name.to_string());
                    }
                }
            }
        }
    }
    names.into_iter().collect()
}
