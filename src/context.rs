use crate::events;
use crate::persist;
use crate::project;
use crate::state::{self, WorkflowState};
use crate::tasks::{self, Task};
use crate::verify::{self, Evidence};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::process::Command;

pub const CONTEXT_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_BUDGET: usize = 4000;
const CHARS_PER_TOKEN: usize = 4;
const MAX_FILES: usize = 12;
const MAX_FILE_LINES: usize = 120;
const MAX_FILE_CHARS: usize = 8000;
const MAX_FILE_BYTES: u64 = 262_144;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileExcerpt {
    pub path: String,
    pub lines: usize,
    pub truncated: bool,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Packet {
    pub schema_version: u32,
    pub task_id: Option<String>,
    pub budget_tokens: usize,
    pub estimated_tokens: usize,
    pub uncertain: bool,
    pub sections: Vec<Section>,
    pub files: Vec<FileExcerpt>,
    pub omitted: Vec<String>,
}

impl Packet {
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("# Ghost context packet\n\n");
        for section in &self.sections {
            out.push_str(&format!(
                "## {}\n{}\n\n",
                section.name,
                section.content.trim_end()
            ));
        }
        if !self.files.is_empty() {
            out.push_str("## Relevant files\n\n");
            for file in &self.files {
                out.push_str(&format!(
                    "### {}{}\n```{}\n{}\n```\n\n",
                    file.path,
                    if file.truncated {
                        format!(" (first {} lines)", file.lines)
                    } else {
                        String::new()
                    },
                    language(&file.path),
                    file.content.trim_end()
                ));
            }
        }
        if !self.omitted.is_empty() {
            out.push_str("## Omitted for budget\n");
            for item in &self.omitted {
                out.push_str(&format!("- {item}\n"));
            }
            out.push('\n');
        }
        out.push_str(&format!(
            "_Estimated {} tokens (chars/{CHARS_PER_TOKEN}, conservative and uncertain by roughly 15%); budget {}._\n",
            self.estimated_tokens, self.budget_tokens
        ));
        out
    }
}

fn language(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("rs") => "rust",
        Some("py") => "python",
        Some("js" | "jsx" | "mjs" | "cjs") => "javascript",
        Some("ts" | "tsx") => "typescript",
        Some("toml") => "toml",
        Some("json") => "json",
        Some("md") => "markdown",
        Some("sh" | "bash") => "bash",
        Some("yaml" | "yml") => "yaml",
        _ => "text",
    }
}

fn tokens(chars: usize) -> usize {
    chars.div_ceil(CHARS_PER_TOKEN)
}

/// Build a deterministic packet: fixed sections first, then optional sections
/// in priority order while the conservative budget allows.
pub fn build(root: &Path, task_id: Option<&str>, budget: usize) -> Result<Packet> {
    let budget = budget.max(200);
    let task = match task_id {
        Some(id) => Some(tasks::load(root, id)?),
        None => None,
    };
    let workflow = load_workflow(root)?;
    let evidence = verify::list(root)?;

    let mut sections = Vec::new();
    let mut omitted = Vec::new();

    sections.push(Section {
        name: "Task".to_string(),
        content: task_section(root, task.as_ref(), &workflow)?,
    });

    let acceptance = acceptance_section(task.as_ref());
    if !acceptance.is_empty() {
        sections.push(Section {
            name: "Acceptance criteria".to_string(),
            content: acceptance,
        });
    }

    if !workflow.blockers.is_empty() {
        sections.push(Section {
            name: "Blockers".to_string(),
            content: workflow
                .blockers
                .iter()
                .map(|line| format!("- {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        });
    }

    let mut estimated = estimate(&sections, &[]);

    if !workflow.decisions.is_empty() {
        let content = workflow
            .decisions
            .iter()
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        estimated = push_optional(
            &mut sections,
            &mut omitted,
            estimated,
            budget,
            "Decisions",
            content,
        );
    }

    let changes = latest_changes(root)?;
    if !changes.is_empty() {
        estimated = push_optional(
            &mut sections,
            &mut omitted,
            estimated,
            budget,
            "Latest changes",
            changes,
        );
    }

    let history = verification_section(root, &evidence, task.as_ref());
    if !history.is_empty() {
        estimated = push_optional(
            &mut sections,
            &mut omitted,
            estimated,
            budget,
            "Verification history",
            history,
        );
    }

    let candidates = candidate_files(root, task.as_ref())?;
    let mut files = Vec::new();
    for path in candidates {
        if files.len() >= MAX_FILES {
            omitted.push(format!("{path} (file limit)"));
            continue;
        }
        let remaining = budget.saturating_sub(estimated);
        if remaining < 40 {
            omitted.push(format!("{path} (budget)"));
            continue;
        }
        let Some(mut excerpt) = read_excerpt(root, &path)? else {
            omitted.push(format!("{path} (missing, binary, or too large)"));
            continue;
        };
        let allowed = remaining * CHARS_PER_TOKEN - path.len() - 16;
        truncate_excerpt(&mut excerpt, allowed);
        if excerpt.content.trim().is_empty() {
            omitted.push(format!("{path} (budget)"));
            continue;
        }
        let cost = tokens(excerpt.content.len() + path.len() + 16);
        estimated += cost;
        files.push(excerpt);
    }

    Ok(Packet {
        schema_version: CONTEXT_SCHEMA_VERSION,
        task_id: task_id.map(str::to_string),
        budget_tokens: budget,
        estimated_tokens: estimated,
        uncertain: true,
        sections,
        files,
        omitted,
    })
}

fn push_optional(
    sections: &mut Vec<Section>,
    omitted: &mut Vec<String>,
    estimated: usize,
    budget: usize,
    name: &str,
    content: String,
) -> usize {
    let cost = tokens(content.len() + name.len());
    if estimated + cost > budget {
        omitted.push(format!("{name} (budget)"));
        return estimated;
    }
    sections.push(Section {
        name: name.to_string(),
        content,
    });
    estimated + cost
}

fn estimate(sections: &[Section], files: &[FileExcerpt]) -> usize {
    let mut chars = 0;
    for section in sections {
        chars += section.name.len() + section.content.len();
    }
    for file in files {
        chars += file.path.len() + file.content.len();
    }
    tokens(chars)
}

fn load_workflow(root: &Path) -> Result<WorkflowState> {
    if let Some(state) = state::load_state(root)? {
        return Ok(state);
    }
    let text = fs::read_to_string(root.join("STATE.md")).unwrap_or_default();
    Ok(WorkflowState::from_markdown(&text))
}

fn task_section(root: &Path, task: Option<&Task>, workflow: &WorkflowState) -> Result<String> {
    let mut lines = Vec::new();
    if let Some(task) = task {
        lines.push(format!("- Objective: {} ({})", task.title, task.id));
        lines.push(format!("- Status: {}", task.status.as_str()));
        if let Some(owner) = &task.owner_agent {
            lines.push(format!("- Owner: {owner}"));
        }
        if let Some(revision) = &task.base_revision {
            lines.push(format!("- Base revision: {}", short(revision)));
        }
        if !task.references.is_empty() {
            lines.push(format!("- References: {}", task.references.join(", ")));
        }
    } else {
        lines.push(format!(
            "- Project: {}",
            project::discover(root)
                .map(|facts| facts.name)
                .unwrap_or_else(|_| "unknown".to_string())
        ));
        if let Some(focus) = workflow.focus.first() {
            lines.push(format!("- Focus: {focus}"));
        }
        if !workflow.next.is_empty() {
            lines.push(format!("- Next: {}", workflow.next.join("; ")));
        }
    }
    Ok(lines.join("\n"))
}

fn acceptance_section(task: Option<&Task>) -> String {
    let Some(task) = task else {
        return String::new();
    };
    task.acceptance_checks
        .iter()
        .map(|check| format!("- [ ] {check}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn latest_changes(root: &Path) -> Result<String> {
    let output = match Command::new("git")
        .args(["diff", "--stat", "HEAD"])
        .current_dir(root)
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return Ok(String::new()),
    };
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(events::redact(&text))
}

fn verification_section(root: &Path, evidence: &[Evidence], task: Option<&Task>) -> String {
    let mut lines = Vec::new();
    for record in evidence {
        if let Some(task) = task {
            if record.task_id.as_deref() != Some(task.id.as_str()) {
                continue;
            }
        }
        let stale = verify::is_stale(root, record);
        lines.push(format!(
            "- {} {} -> exit {}{}{}",
            record.id,
            record.command,
            record.exit_code,
            if record.timed_out { " (timed out)" } else { "" },
            if stale {
                " [stale: sources changed]"
            } else {
                ""
            }
        ));
        if lines.len() >= 8 {
            break;
        }
    }
    lines.join("\n")
}

fn candidate_files(root: &Path, task: Option<&Task>) -> Result<Vec<String>> {
    let mut paths = Vec::new();
    if let Some(task) = task {
        paths.extend(task.references.iter().cloned());
    }
    for path in git_paths(root) {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    if let Ok(facts) = project::discover(root) {
        for manifest in facts.manifests {
            if !paths.contains(&manifest) {
                paths.push(manifest);
            }
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn git_paths(root: &Path) -> Vec<String> {
    let output = match Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.get(3..))
        .map(|path| path.trim().trim_matches('"').to_string())
        .filter(|path| !path.is_empty())
        .filter(|path| {
            !path.starts_with(".ghost/")
                && !path.starts_with("target/")
                && !path.starts_with("node_modules/")
                && !path.contains(" -> ")
        })
        .collect()
}

fn read_excerpt(root: &Path, relative: &str) -> Result<Option<FileExcerpt>> {
    let relative_path = Path::new(relative);
    if persist::validate_relative(relative_path).is_err() {
        return Ok(None);
    }
    let path = root.join(relative_path);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(_) => return Ok(None),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_FILE_BYTES {
        return Ok(None);
    }
    let bytes = fs::read(&path).with_context(|| format!("could not read {}", path.display()))?;
    if bytes.iter().take(1024).any(|byte| *byte == 0) {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut content = String::new();
    let mut lines = 0;
    let mut truncated = false;
    for line in text.lines() {
        if lines >= MAX_FILE_LINES || content.len() + line.len() > MAX_FILE_CHARS {
            truncated = true;
            break;
        }
        content.push_str(line);
        content.push('\n');
        lines += 1;
    }
    Ok(Some(FileExcerpt {
        path: relative.to_string(),
        lines,
        truncated,
        content,
    }))
}

fn short(revision: &str) -> &str {
    &revision[..revision.len().min(8)]
}

/// Trim an excerpt to a character budget at a line boundary when possible.
fn truncate_excerpt(excerpt: &mut FileExcerpt, max_chars: usize) {
    if excerpt.content.len() <= max_chars {
        return;
    }
    let cut = excerpt
        .content
        .char_indices()
        .take_while(|(index, _)| *index < max_chars)
        .map(|(index, ch)| index + ch.len_utf8())
        .last()
        .unwrap_or(0);
    let mut truncated = excerpt.content[..cut].to_string();
    if let Some(index) = truncated.rfind('\n') {
        truncated.truncate(index + 1);
    }
    excerpt.lines = truncated.lines().count();
    excerpt.truncated = true;
    excerpt.content = truncated;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(root: &Path) -> Task {
        let lock = persist::FileLock::acquire(root).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
        )
        .unwrap();
        tasks::add(
            root,
            "Add addition",
            Some("opencode"),
            &["src/lib.rs".to_string()],
            &["cargo test".to_string()],
            &lock,
        )
        .unwrap()
    }

    #[test]
    fn packet_contains_objective_acceptance_and_reference() {
        let dir = tempfile::tempdir().unwrap();
        let task = sample(dir.path());
        let packet = build(dir.path(), Some(&task.id), DEFAULT_BUDGET).unwrap();
        let rendered = packet.render();
        assert!(rendered.contains("Add addition"));
        assert!(rendered.contains("cargo test"));
        assert!(rendered.contains("src/lib.rs"));
        assert!(rendered.contains("Estimated"));
        assert!(packet.estimated_tokens <= packet.budget_tokens);
    }

    #[test]
    fn small_budget_truncates_deterministically() {
        let dir = tempfile::tempdir().unwrap();
        let task = sample(dir.path());
        fs::write(
            dir.path().join("src/lib.rs"),
            "pub fn wide() -> String { String::from(\"padding padding padding padding\") }\n"
                .repeat(200),
        )
        .unwrap();
        let packet = build(dir.path(), Some(&task.id), 400).unwrap();
        assert!(
            packet.estimated_tokens <= 400,
            "{}",
            packet.estimated_tokens
        );
        assert!(
            packet.files.iter().any(|file| file.truncated) || !packet.omitted.is_empty(),
            "expected truncation or omission"
        );
        let again = build(dir.path(), Some(&task.id), 400).unwrap();
        assert_eq!(packet.omitted, again.omitted);
        assert_eq!(packet.files.len(), again.files.len());
    }

    #[test]
    fn packet_without_task_summarizes_project() {
        let dir = tempfile::tempdir().unwrap();
        sample(dir.path());
        let packet = build(dir.path(), None, DEFAULT_BUDGET).unwrap();
        assert!(packet.task_id.is_none());
        assert!(packet.render().contains("Project"));
    }
}
