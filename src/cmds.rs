use crate::adapter::AdapterId;
use crate::answers::{Answers, Mode};
use crate::persist::FileLock;
use anyhow::{bail, Context, Result};
use chrono::Local;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const ANSWERS_PATH: &str = ".ghost/answers.json";

pub fn new_project(
    dir: &Path,
    force: bool,
    answers_path: Option<&Path>,
    requested_adapters: &[String],
) -> Result<i32> {
    fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    let lock = FileLock::acquire(dir)?;
    let answers = match answers_path {
        Some(path) => Answers::load(path)?,
        None => crate::interview::run(dir)?,
    };
    answers.validate_for_new(Local::now())?;

    let facts = crate::project::discover(dir)?;
    let adapters = resolve_adapters(dir, requested_adapters)?;
    let agents = crate::opencode::detected_agents();
    let files = crate::render::generate_for(&adapters, &answers, &agents)?;
    let target = dir.join(ANSWERS_PATH);
    let mut existing = crate::render::existing_targets(dir, &files);
    if target.exists() {
        existing.push(ANSWERS_PATH.to_string());
    }
    existing.sort();
    existing.dedup();
    if !force && !existing.is_empty() {
        bail!(
            "refusing to overwrite existing files:\n  {}\npass --force to overwrite generated files",
            existing.join("\n  ")
        );
    }

    write_all(dir, &lock, &answers, Some(&facts), &adapters, &files)?;
    println!(
        "generated {} files in {}",
        files.len(),
        dir.canonicalize()
            .unwrap_or_else(|_| dir.to_path_buf())
            .display()
    );
    println!("{}", next_hint(&adapters));
    Ok(0)
}

/// Initialize a repository without hackathon assumptions.
///
/// Existing answers and generated files are preserved. Files changed since the
/// last generation, or files ghost did not generate, require `--force`.
pub fn init_project(
    dir: &Path,
    force: bool,
    profile: Mode,
    name: Option<&str>,
    answers_path: Option<&Path>,
    requested_adapters: &[String],
) -> Result<i32> {
    fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    let lock = FileLock::acquire(dir)?;
    let facts = crate::project::discover(dir)?;
    let stored_answers = dir.join(ANSWERS_PATH);

    let mut answers = if let Some(path) = answers_path {
        Answers::load(path)?
    } else if stored_answers.exists() {
        Answers::load(&stored_answers)?
    } else if profile == Mode::Hackathon {
        crate::interview::run(dir)?
    } else {
        crate::project::generic_answers(&facts, name)
    };
    if let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) {
        answers.project_name = name.to_string();
    }
    answers.validate_structure()?;

    let adapters = resolve_adapters(dir, requested_adapters)?;
    let agents = crate::opencode::detected_agents();
    let files = crate::render::generate_for(&adapters, &answers, &agents)?;
    let entries = crate::render::manifest_entries(&files);
    let manifest = crate::persist::load_manifest(dir)?;
    let conflicts = crate::persist::conflicts(dir, &entries, manifest.as_ref())?;
    if !force && !conflicts.is_empty() {
        bail!(
            "refusing to overwrite files that ghost did not generate or that changed since generation:\n  {}\npass --force to overwrite generated files",
            conflicts.join("\n  ")
        );
    }

    write_all(dir, &lock, &answers, Some(&facts), &adapters, &files)?;
    println!(
        "initialized {} project in {}",
        facts.primary_stack().label(),
        dir.canonicalize()
            .unwrap_or_else(|_| dir.to_path_buf())
            .display()
    );
    if !facts.manifests.is_empty() {
        println!("detected: {}", facts.manifests.join(", "));
    }
    println!(
        "generated {} files; canonical state is in .ghost/",
        files.len()
    );
    println!("{}", next_hint(&adapters));
    Ok(0)
}

pub fn regen(force: bool, requested_adapters: &[String]) -> Result<i32> {
    let root = Path::new(".");
    let lock = FileLock::acquire(root)?;
    let answers = load_required(root)?;
    answers.validate_structure()?;
    let facts = crate::project::discover(root)?;
    let adapters = resolve_adapters(root, requested_adapters)?;
    let agents = crate::opencode::detected_agents();
    let files = crate::render::generate_for(&adapters, &answers, &agents)?;
    let entries = crate::render::manifest_entries(&files);
    let manifest = crate::persist::load_manifest(root)?;
    let conflicts = crate::persist::conflicts(root, &entries, manifest.as_ref())?;
    if !force && !conflicts.is_empty() {
        bail!(
            "refusing to overwrite files that ghost did not generate or that changed since generation:\n  {}\npass --force to overwrite generated files",
            conflicts.join("\n  ")
        );
    }

    write_all(root, &lock, &answers, Some(&facts), &adapters, &files)?;
    println!("re-rendered {} files", files.len());
    Ok(0)
}

/// Resolve the adapter selection: an explicit flag wins, then the saved
/// `.ghost/config.toml`, then the OpenCode default.
fn resolve_adapters(root: &Path, requested: &[String]) -> Result<Vec<AdapterId>> {
    if !requested.is_empty() {
        return AdapterId::from_names(requested);
    }
    let stored = crate::state::load_config_adapters(root)?;
    if stored.is_empty() {
        Ok(vec![AdapterId::OpenCode])
    } else {
        AdapterId::from_names(&stored)
    }
}

fn next_hint(adapters: &[AdapterId]) -> &'static str {
    if adapters.iter().any(|adapter| adapter.supports_commands()) {
        "next: run `ghost status`, then open your agent and use `/plan`"
    } else {
        "next: run `ghost status`, then open Codex and read `specs/00-overview.md`"
    }
}

fn write_all(
    root: &Path,
    lock: &FileLock,
    answers: &Answers,
    facts: Option<&crate::project::ProjectFacts>,
    adapters: &[AdapterId],
    files: &[crate::render::GeneratedFile],
) -> Result<()> {
    answers.save(&root.join(ANSWERS_PATH))?;
    // Migrate an existing STATE.md before the generated view replaces it.
    let has_commands = adapters.iter().any(|adapter| adapter.supports_commands());
    let _ = crate::state::ensure_state(root, lock, answers, has_commands)?;
    crate::render::write_files(root, files)?;
    let entries = crate::render::manifest_entries(files);
    crate::persist::write_manifest(root, &entries, lock)?;
    crate::state::write_config(
        root,
        lock,
        crate::state::profile_name(answers.mode),
        &AdapterId::names(adapters),
    )?;
    crate::state::write_project(root, lock, answers, facts)?;
    crate::render::append_gitignore(root)?;
    Ok(())
}

pub fn load_required(root: &Path) -> Result<Answers> {
    let path = root.join(ANSWERS_PATH);
    if !path.exists() {
        bail!(
            "run `ghost init` or `ghost new` first (missing {})",
            path.display()
        );
    }
    Answers::load(&path)
}

pub fn status() -> Result<i32> {
    crate::status::run(Path::new("."))
}

/// Show adapter detection and declared capabilities.
pub fn adapters(json: bool) -> Result<i32> {
    let info = crate::adapter::detect_all(Path::new("."));
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&info).context("could not serialize adapter info")?
        );
        return Ok(0);
    }
    for adapter in info {
        println!("{} ({})", adapter.id, adapter.label);
        println!(
            "  capabilities: {}",
            adapter.capabilities.summary().join(", ")
        );
        println!("  {}", adapter.detection.detail);
    }
    Ok(0)
}

pub fn check(timeout: u64) -> Result<i32> {
    let root = Path::new(".");
    load_required(root)?;
    let script = root.join("scripts/check.sh");
    if !script.exists() {
        bail!("missing scripts/check.sh; run `ghost regen` first");
    }
    let mut child = Command::new("setsid")
        .arg("bash")
        .arg(&script)
        .current_dir(root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("could not start scripts/check.sh (is setsid installed?)")?;
    let pid = child.id();
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.code().unwrap_or(1));
        }
        if timeout > 0 && started.elapsed() >= Duration::from_secs(timeout) {
            kill_group(pid, "-TERM");
            std::thread::sleep(Duration::from_millis(300));
            kill_group(pid, "-KILL");
            let _ = child.wait();
            println!("check timed out after {timeout}s");
            return Ok(124);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn kill_group(pid: u32, signal: &str) {
    let _ = Command::new("kill")
        .arg(signal)
        .arg("--")
        .arg(format!("-{pid}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

pub fn down() -> Result<i32> {
    let root = Path::new(".");
    load_required(root)?;
    let script = root.join("scripts/dev.sh");
    if !script.exists() {
        bail!("missing scripts/dev.sh; run `ghost regen` first");
    }
    let status = Command::new("bash")
        .arg(&script)
        .arg("stop")
        .current_dir(root)
        .status()
        .context("could not run scripts/dev.sh stop")?;
    Ok(status.code().unwrap_or(1))
}

pub fn checkpoint(args: &[String]) -> Result<i32> {
    let root = Path::new(".");
    load_required(root)?;

    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .context("could not run git status; is this a git repository?")?;
    if !output.status.success() {
        bail!("git status failed; is this a git repository?");
    }
    let status = String::from_utf8_lossy(&output.stdout);
    if status.trim().is_empty() {
        println!("nothing to commit");
        return Ok(0);
    }

    for path in changed_paths(&status) {
        if sensitive_path(&path) {
            eprintln!("refusing checkpoint: sensitive file {path}");
            return Ok(1);
        }
    }

    let message = args.join(" ").trim().to_string();
    let message = if message.is_empty() {
        Local::now().format("%Y-%m-%d %H:%M").to_string()
    } else {
        message
    };
    let commit_message = format!("checkpoint: {message}");

    let add = Command::new("git")
        .args(["add", "-A"])
        .current_dir(root)
        .status()
        .context("could not run git add")?;
    if !add.success() {
        return Ok(add.code().unwrap_or(1));
    }
    let commit = Command::new("git")
        .args(["commit", "-m", &commit_message])
        .current_dir(root)
        .status()
        .context("could not run git commit")?;
    Ok(commit.code().unwrap_or(1))
}

fn changed_paths(status: &str) -> Vec<String> {
    status
        .lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }
            let path = line[3..].split(" -> ").last().unwrap_or("").trim();
            let path = path.trim_matches('"');
            (!path.is_empty()).then(|| path.to_string())
        })
        .collect()
}

fn sensitive_path(path: &str) -> bool {
    let name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path);
    name.starts_with(".env")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.starts_with("id_rsa")
}
