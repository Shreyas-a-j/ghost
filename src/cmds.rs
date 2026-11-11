use crate::answers::Answers;
use anyhow::{bail, Context, Result};
use chrono::Local;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const ANSWERS_PATH: &str = ".ghost/answers.json";

pub fn new_project(dir: &Path, force: bool, answers_path: Option<&Path>) -> Result<i32> {
    fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    let answers = match answers_path {
        Some(path) => Answers::load(path)?,
        None => crate::interview::run(dir)?,
    };
    answers.validate_for_new(Local::now())?;

    let agents = crate::opencode::detected_agents();
    let files = crate::render::generate(&answers, &agents)?;
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

    answers.save(&target)?;
    crate::render::write_files(dir, &files)?;
    crate::render::append_gitignore(dir)?;
    println!(
        "generated {} files in {}",
        files.len(),
        dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf()).display()
    );
    println!("next: run `ghost status`, then open OpenCode and use `/plan`");
    Ok(0)
}

pub fn regen() -> Result<i32> {
    let root = Path::new(".");
    let answers = load_required(root)?;
    answers.validate_structure()?;
    let agents = crate::opencode::detected_agents();
    let files = crate::render::generate(&answers, &agents)?;
    crate::render::write_files(root, &files)?;
    crate::render::append_gitignore(root)?;
    println!("re-rendered {} files", files.len());
    Ok(0)
}

pub fn load_required(root: &Path) -> Result<Answers> {
    let path = root.join(ANSWERS_PATH);
    if !path.exists() {
        bail!(
            "run `ghost new` first (missing {})",
            path.display()
        );
    }
    Answers::load(&path)
}

pub fn status() -> Result<i32> {
    crate::status::run(Path::new("."))
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
