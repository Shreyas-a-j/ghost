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

pub fn task_add(
    title: &[String],
    owner: Option<&str>,
    references: &[String],
    checks: &[String],
    json: bool,
) -> Result<i32> {
    let root = Path::new(".");
    load_required(root)?;
    let title = title.join(" ");
    let lock = FileLock::acquire(root)?;
    let task = crate::tasks::add(root, &title, owner, references, checks, &lock)?;
    crate::events::append(
        root,
        &crate::events::Event::task_created(&task.id, &task.title, task.owner_agent.as_deref()),
        &lock,
    )?;
    print_task(&task, json)?;
    Ok(0)
}

pub fn task_start(id: &str, owner: Option<&str>) -> Result<i32> {
    let root = Path::new(".");
    load_required(root)?;
    let lock = FileLock::acquire(root)?;
    let (task, previous) = crate::tasks::start(root, id, owner, &lock)?;
    if let Some(previous) = previous {
        crate::events::append(
            root,
            &crate::events::Event::task_transition(
                &task.id,
                previous.as_str(),
                task.status.as_str(),
                task.owner_agent.as_deref(),
            ),
            &lock,
        )?;
    }
    print_task(&task, false)?;
    Ok(0)
}

pub fn task_move(id: &str, status: &str) -> Result<i32> {
    let root = Path::new(".");
    load_required(root)?;
    let target = crate::tasks::TaskStatus::parse(status)?;
    let lock = FileLock::acquire(root)?;
    let (task, previous) = crate::tasks::transition(root, id, target, &lock)?;
    crate::events::append(
        root,
        &crate::events::Event::task_transition(
            &task.id,
            previous.as_str(),
            task.status.as_str(),
            task.owner_agent.as_deref(),
        ),
        &lock,
    )?;
    print_task(&task, false)?;
    Ok(0)
}

pub fn task_list(json: bool) -> Result<i32> {
    let root = Path::new(".");
    load_required(root)?;
    let tasks = crate::tasks::list(root)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&tasks).context("could not serialize tasks")?
        );
        return Ok(0);
    }
    if tasks.is_empty() {
        println!("no tasks");
        return Ok(0);
    }
    for task in tasks {
        println!(
            "{:<10} {:<12} {}",
            task.id,
            task.status.as_str(),
            task.title
        );
    }
    Ok(0)
}

pub fn task_show(id: &str, json: bool) -> Result<i32> {
    let root = Path::new(".");
    load_required(root)?;
    let task = crate::tasks::load(root, id)?;
    print_task(&task, json)?;
    Ok(0)
}

fn print_task(task: &crate::tasks::Task, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(task).context("could not serialize task")?
        );
    } else {
        println!("{} {}: {}", task.id, task.status.as_str(), task.title);
    }
    Ok(())
}

pub fn handoff(note: Option<&str>) -> Result<i32> {
    let root = Path::new(".");
    let answers = load_required(root)?;
    let lock = FileLock::acquire(root)?;
    let has_commands = resolve_adapters(root, &[])?
        .iter()
        .any(|adapter| adapter.supports_commands());
    let state = crate::state::ensure_state(root, &lock, &answers, has_commands)?;
    let snapshot = crate::handoff::capture(root, &state, note, &lock)?;
    println!(
        "handoff saved: {} tasks, revision {}",
        snapshot.tasks.len(),
        snapshot
            .revision
            .as_deref()
            .map(|revision| &revision[..revision.len().min(8)])
            .unwrap_or("none")
    );
    Ok(0)
}

pub fn resume() -> Result<i32> {
    let root = Path::new(".");
    load_required(root)?;
    let lock = FileLock::acquire(root)?;
    let Some(report) = crate::handoff::resume(root, &lock)? else {
        bail!("no handoff snapshot; run `ghost handoff` first");
    };
    println!("{}", report.render());
    Ok(if report.stale { 2 } else { 0 })
}

fn record_checkpoint(root: &Path, message: &str) -> Result<()> {
    let lock = FileLock::acquire(root)?;
    let revision = crate::project::discover(root)?
        .git
        .and_then(|git| git.revision);
    crate::events::append(
        root,
        &crate::events::Event::checkpoint(revision.as_deref(), message),
        &lock,
    )
}

pub fn context(task: Option<&str>, budget: usize, json: bool) -> Result<i32> {
    let root = Path::new(".");
    load_required(root)?;
    let packet = crate::context::build(root, task, budget)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&packet).context("could not serialize context packet")?
        );
    } else {
        print!("{}", packet.render());
    }
    Ok(0)
}

pub fn verify(
    task: Option<&str>,
    approve: bool,
    timeout: u64,
    list: bool,
    command: &[String],
) -> Result<i32> {
    let root = Path::new(".");
    load_required(root)?;

    if list {
        let evidence = crate::verify::list(root)?;
        if evidence.is_empty() {
            println!("no verification evidence");
            return Ok(0);
        }
        for record in evidence {
            let stale = crate::verify::is_stale(root, &record);
            let owner = record
                .task_id
                .as_deref()
                .map(|task| format!(" [{task}]"))
                .unwrap_or_default();
            println!(
                "{}{owner}{}",
                record.summary(),
                if stale { " [stale]" } else { "" }
            );
        }
        return Ok(0);
    }

    let commands: Vec<String> = if !command.is_empty() {
        vec![command.join(" ")]
    } else if let Some(id) = task {
        let task = crate::tasks::load(root, id)?;
        if task.acceptance_checks.is_empty() {
            bail!("{id} has no acceptance checks; pass a command after --");
        }
        task.acceptance_checks
    } else {
        bail!("pass a command after -- or select --task");
    };

    for candidate in &commands {
        if !crate::verify::is_allowed(root, candidate, approve)? {
            bail!(
                "command not approved: {candidate}\npass --approve or add it to .ghost/verify.json (exact match)"
            );
        }
    }

    let lock = FileLock::acquire(root)?;
    let mut code = 0;
    for candidate in commands {
        let evidence = crate::verify::run(root, task, &candidate, timeout, &lock)?;
        println!("{}", evidence.summary());
        for line in &evidence.output_tail {
            println!("  {line}");
        }
        if evidence.exit_code != 0 {
            code = evidence.exit_code;
        }
    }
    Ok(code)
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

    // Record the event before staging so the checkpoint commit includes it.
    // The revision names the parent commit the checkpoint was built on.
    if let Err(err) = record_checkpoint(root, &message) {
        eprintln!("ghost: warning: could not record checkpoint metadata: {err:#}");
    }
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
