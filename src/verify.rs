use crate::events::{self, Event};
use crate::persist::{self, FileLock};
use crate::project;
use crate::tasks;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const EVIDENCE_SCHEMA_VERSION: u32 = 1;
pub const EVIDENCE_DIR: &str = ".ghost/evidence";
pub const VERIFY_CONFIG_PATH: &str = ".ghost/verify.json";
pub const MAX_OUTPUT_LINES: usize = 40;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationConfig {
    pub schema_version: u32,
    #[serde(default)]
    pub allow: Vec<String>,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            allow: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub schema_version: u32,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub command: String,
    pub cwd: String,
    pub started_at: DateTime<Local>,
    pub ended_at: DateTime<Local>,
    pub duration_ms: u64,
    pub exit_code: i32,
    pub timed_out: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    pub dirty: bool,
    pub redaction: String,
    #[serde(default)]
    pub source_hashes: BTreeMap<String, String>,
    #[serde(default)]
    pub output_tail: Vec<String>,
}

impl Evidence {
    pub fn summary(&self) -> String {
        format!(
            "{} {} exit={} ({}ms){}",
            self.id,
            self.command,
            self.exit_code,
            self.duration_ms,
            if self.timed_out { " timed out" } else { "" }
        )
    }
}

pub fn evidence_dir(root: &Path) -> PathBuf {
    root.join(EVIDENCE_DIR)
}

pub fn evidence_path(root: &Path, id: &str) -> PathBuf {
    evidence_dir(root).join(format!("{id}.json"))
}

pub fn load_config(root: &Path) -> Result<VerificationConfig> {
    let path = root.join(VERIFY_CONFIG_PATH);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(VerificationConfig::default())
        }
        Err(err) => return Err(err).with_context(|| format!("could not read {}", path.display())),
    };
    let config: VerificationConfig = serde_json::from_str(&raw)
        .with_context(|| format!("could not parse {}", path.display()))?;
    if config.schema_version != EVIDENCE_SCHEMA_VERSION {
        bail!(
            "unsupported schema_version {} in {}; expected {}",
            config.schema_version,
            path.display(),
            EVIDENCE_SCHEMA_VERSION
        );
    }
    Ok(config)
}

pub fn is_allowed(root: &Path, command: &str, approve: bool) -> Result<bool> {
    if approve {
        return Ok(true);
    }
    let config = load_config(root)?;
    Ok(config
        .allow
        .iter()
        .any(|entry| entry.trim() == command.trim()))
}

/// Execute one approved command and store its evidence.
///
/// Output is captured to a temporary file, summarized, and deleted. Only a
/// redacted tail reaches the evidence record.
pub fn run(
    root: &Path,
    task_id: Option<&str>,
    command: &str,
    timeout: u64,
    lock: &FileLock,
) -> Result<Evidence> {
    let command = command.trim();
    if command.is_empty() {
        bail!("verify needs a command");
    }
    if let Some(id) = task_id {
        tasks::validate_id(id)?;
    }

    let facts = project::discover(root)?;
    let revision = facts.git.as_ref().and_then(|git| git.revision.clone());
    let dirty = facts.git.as_ref().map(|git| git.dirty).unwrap_or(false);
    let source_hashes = source_hashes(root, task_id, &facts.manifests)?;

    let id = next_run_id(root);
    let (log_path, log) = create_log(root)?;
    let log_err = log.try_clone()?;

    let started_at = Local::now();
    let started = Instant::now();
    let mut child = match Command::new("setsid")
        .arg("bash")
        .arg("-c")
        .arg(command)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            let _ = fs::remove_file(&log_path);
            return Err(err).context("could not start verification command (is setsid installed?)");
        }
    };
    let pid = child.id();
    let mut timed_out = false;
    let exit_code = loop {
        if let Some(status) = child.try_wait()? {
            break status.code().unwrap_or(1);
        }
        if timeout > 0 && started.elapsed() >= Duration::from_secs(timeout) {
            timed_out = true;
            kill_group(pid, "-TERM");
            std::thread::sleep(Duration::from_millis(300));
            kill_group(pid, "-KILL");
            let _ = child.wait();
            break 124;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let duration_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    let ended_at = Local::now();

    let output_tail = tail_lines(&log_path, MAX_OUTPUT_LINES)?;
    let _ = fs::remove_file(&log_path);

    let evidence = Evidence {
        schema_version: EVIDENCE_SCHEMA_VERSION,
        id,
        task_id: task_id.map(str::to_string),
        command: command.to_string(),
        cwd: ".".to_string(),
        started_at,
        ended_at,
        duration_ms,
        exit_code,
        timed_out,
        revision,
        dirty,
        redaction: "control characters stripped; 40-line tail; 200 characters per line".to_string(),
        source_hashes,
        output_tail,
    };
    save(root, &evidence)?;

    if let Some(task_id) = task_id {
        let mut task = tasks::load(root, task_id)?;
        if !task.evidence_ids.iter().any(|id| id == &evidence.id) {
            task.evidence_ids.push(evidence.id.clone());
            task.updated_at = Local::now();
            tasks::save(root, &task, lock)?;
        }
    }
    events::append(
        root,
        &Event::verification(
            task_id,
            &evidence.id,
            command,
            exit_code,
            timed_out,
            evidence.revision.as_deref(),
        ),
        lock,
    )?;
    Ok(evidence)
}

/// Create a unique, private log file with `create_new`, which refuses to follow
/// a pre-planted symlink. Never derive the name from a predictable timestamp
/// alone.
fn create_log(root: &Path) -> Result<(PathBuf, fs::File)> {
    let dir = root.join(".ghost");
    fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    let pid = std::process::id();
    for attempt in 0..64 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path = dir.join(format!(".verify-{pid}-{nanos}-{attempt}.log"));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(err).with_context(|| format!("could not create {}", path.display()))
            }
        }
    }
    bail!(
        "could not create a private verification log in {}",
        dir.display()
    )
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

fn tail_lines(path: &Path, limit: usize) -> Result<Vec<String>> {
    let mut raw = String::new();
    match fs::File::open(path) {
        Ok(mut file) => {
            file.read_to_string(&mut raw)
                .with_context(|| format!("could not read {}", path.display()))?;
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("could not read {}", path.display())),
    }
    let lines: Vec<&str> = raw.lines().collect();
    let start = lines.len().saturating_sub(limit);
    Ok(lines[start..]
        .iter()
        .map(|line| events::redact(line))
        .collect())
}

fn next_run_id(root: &Path) -> String {
    let base = Local::now().format("RUN-%Y%m%dT%H%M%S");
    let mut id = base.to_string();
    let mut counter = 1;
    while evidence_path(root, &id).exists() {
        id = format!("{base}-{counter:02}");
        counter += 1;
    }
    id
}

pub fn save(root: &Path, evidence: &Evidence) -> Result<()> {
    let target = persist::safe_target(
        root,
        &Path::new(EVIDENCE_DIR).join(format!("{}.json", evidence.id)),
    )?;
    let raw = serde_json::to_string_pretty(evidence).context("could not serialize evidence")?;
    persist::atomic_write(&target, format!("{raw}\n").as_bytes(), false)
        .with_context(|| format!("could not write {}", target.display()))
}

pub fn list(root: &Path) -> Result<Vec<Evidence>> {
    let directory = evidence_dir(root);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err).with_context(|| format!("could not read {}", directory.display()))
        }
    };
    let mut evidence = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("could not read {}", path.display()))?;
        let mut record: Evidence = serde_json::from_str(&raw)
            .with_context(|| format!("could not parse {}", path.display()))?;
        anyhow::ensure!(
            record.schema_version == EVIDENCE_SCHEMA_VERSION,
            "{} uses unsupported schema_version {} (expected {})",
            path.display(),
            record.schema_version,
            EVIDENCE_SCHEMA_VERSION
        );
        record.command = crate::events::terminal_safe(&record.command);
        record.task_id = record.task_id.as_deref().map(crate::events::terminal_safe);
        evidence.push(record);
    }
    evidence.sort_by_key(|record| std::cmp::Reverse(record.started_at));
    Ok(evidence)
}

/// A run is stale when any file it depended on is missing or changed, or when
/// it was recorded without a revision in a repository that now has one.
pub fn is_stale(root: &Path, evidence: &Evidence) -> bool {
    if evidence.source_hashes.is_empty() {
        return false;
    }
    for (relative, expected) in &evidence.source_hashes {
        let path = root.join(relative);
        match fs::read(&path) {
            Ok(bytes) if persist::sha256_hex(&bytes) == *expected => {}
            Ok(_) => return true,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return true,
            Err(_) => return true,
        }
    }
    false
}

fn source_hashes(
    root: &Path,
    task_id: Option<&str>,
    manifests: &[String],
) -> Result<BTreeMap<String, String>> {
    let mut paths: Vec<String> = manifests.to_vec();
    for candidate in [
        "Cargo.lock",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "poetry.lock",
        "requirements.txt",
    ] {
        if root.join(candidate).exists() {
            paths.push(candidate.to_string());
        }
    }
    if let Some(task_id) = task_id {
        let task = tasks::load(root, task_id)?;
        paths.extend(task.references);
    } else {
        paths.extend(git_changed_paths(root)?);
    }
    paths.sort();
    paths.dedup();

    let mut hashes = BTreeMap::new();
    for relative in paths {
        let relative_path = Path::new(&relative);
        if persist::validate_relative(relative_path).is_err() {
            continue;
        }
        let path = root.join(relative_path);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            continue;
        }
        if let Ok(bytes) = fs::read(&path) {
            hashes.insert(relative, persist::sha256_hex(&bytes));
        }
    }
    Ok(hashes)
}

fn git_changed_paths(root: &Path) -> Result<Vec<String>> {
    let output = match Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return Ok(Vec::new()),
    };
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.get(3..))
        .map(|path| path.trim().trim_matches('"').to_string())
        .filter(|path| !path.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_matches_exact_commands() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".ghost")).unwrap();
        fs::write(
            dir.path().join(VERIFY_CONFIG_PATH),
            "{\"schema_version\":1,\"allow\":[\"cargo test\"]}",
        )
        .unwrap();
        assert!(is_allowed(dir.path(), "cargo test", false).unwrap());
        assert!(!is_allowed(dir.path(), "cargo test --all", false).unwrap());
        assert!(is_allowed(dir.path(), "rm -rf /", true).unwrap());
    }

    #[test]
    fn evidence_round_trips_and_detects_staleness() {
        let dir = tempfile::tempdir().unwrap();
        let lock = FileLock::acquire(dir.path()).unwrap();
        fs::write(dir.path().join("input.txt"), "one").unwrap();
        let mut hashes = BTreeMap::new();
        hashes.insert("input.txt".to_string(), persist::sha256_hex(b"one"));
        let now = Local::now();
        let evidence = Evidence {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            id: "RUN-test".to_string(),
            task_id: None,
            command: "true".to_string(),
            cwd: ".".to_string(),
            started_at: now,
            ended_at: now,
            duration_ms: 1,
            exit_code: 0,
            timed_out: false,
            revision: None,
            dirty: false,
            redaction: "none".to_string(),
            source_hashes: hashes,
            output_tail: vec!["ok".to_string()],
        };
        save(dir.path(), &evidence).unwrap();
        assert_eq!(list(dir.path()).unwrap().len(), 1);
        assert!(!is_stale(dir.path(), &evidence));
        fs::write(dir.path().join("input.txt"), "two").unwrap();
        assert!(is_stale(dir.path(), &evidence));
        drop(lock);
    }

    #[test]
    fn run_captures_exit_code_output_and_journal() {
        let dir = tempfile::tempdir().unwrap();
        let lock = FileLock::acquire(dir.path()).unwrap();
        let evidence = run(dir.path(), None, "echo hello; exit 3", 10, &lock).unwrap();
        assert_eq!(evidence.exit_code, 3);
        assert_eq!(evidence.output_tail, vec!["hello".to_string()]);
        assert!(!is_stale(dir.path(), &evidence));
        let entries = events::load(dir.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, events::EventKind::Verification);
    }
}
