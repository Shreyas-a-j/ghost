use anyhow::{ensure, Context, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Agent { Opencode, Claude, Codex }

impl Agent {
    pub fn name(self) -> &'static str {
        match self { Self::Opencode => "opencode", Self::Claude => "claude", Self::Codex => "codex" }
    }

    fn guidance(self) -> &'static str {
        match self { Self::Claude => "CLAUDE.md", Self::Codex | Self::Opencode => "AGENTS.md" }
    }
}

const GUIDE: &str = "# Ghost workflow\n\nRun `ghost status` to inspect tasks. Run `ghost resume` to read the latest handoff. Treat reported progress as an agent claim until independently verified.\n";
const COMMAND: &str = "---\ndescription: Resume the latest Ghost handoff\n---\nRun `ghost resume` and review its task, blockers, remaining work and revision warning before continuing.\n";

fn files(agents: &[Agent]) -> BTreeMap<&'static str, &'static str> {
    let mut result = BTreeMap::new();
    for agent in agents {
        result.insert(agent.guidance(), GUIDE);
        if *agent == Agent::Opencode {
            result.insert(".opencode/commands/ghost-resume.md", COMMAND);
        }
    }
    result
}

pub fn sync(root: &Path, agents: &[Agent], apply: bool) -> Result<()> {
    let mut missing = Vec::new();
    let mut conflicts = Vec::new();
    for (relative, body) in files(agents) {
        let path = root.join(relative);
        super::workflow::safe_path(root, &path)?;
        match fs::read(&path) {
            Ok(bytes) if bytes == body.as_bytes() => println!("current {relative}"),
            Ok(_) => conflicts.push(relative),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                println!("create {relative}");
                missing.push((path, body));
            }
            Err(e) => return Err(e).context("reading adapter target"),
        }
    }
    ensure!(conflicts.is_empty(), "existing instructions differ: {}. Review these files before syncing.", conflicts.join(", "));
    if apply {
        for (path, body) in missing {
            fs::create_dir_all(path.parent().unwrap())?;
            super::workflow::write_atomic(&path, body.as_bytes(), false)?;
        }
    }
    Ok(())
}

pub fn doctor(agents: &[Agent]) {
    for agent in agents {
        let binary = agent.name();
        let available = std::env::var_os("PATH").into_iter()
            .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
            .any(|dir| {
                use std::os::unix::fs::PermissionsExt;
                fs::metadata(dir.join(binary)).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            });
        println!("{binary}: executable on PATH={available}, instructions={}, project commands={}", agent.guidance(), *agent == Agent::Opencode);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adapters_share_guidance_and_preserve_user_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let agents = [Agent::Codex, Agent::Claude, Agent::Opencode];
        sync(root, &agents, false).unwrap();
        assert_eq!(fs::read_dir(root).unwrap().count(), 0);
        fs::write(root.join("CLAUDE.md"), "team rules").unwrap();
        assert!(sync(root, &agents, true).is_err());
        assert!(!root.join("AGENTS.md").exists());
        assert_eq!(fs::read_to_string(root.join("CLAUDE.md")).unwrap(), "team rules");
        fs::remove_file(root.join("CLAUDE.md")).unwrap();
        sync(root, &agents, true).unwrap();
        sync(root, &agents, true).unwrap();
        for (name, content) in files(&agents) {
            assert_eq!(fs::read_to_string(root.join(name)).unwrap(), content);
        }
    }
}
