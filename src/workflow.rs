use crate::adapters::{self, Agent};
use anyhow::{bail, ensure, Context, Result};
use clap::{Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Subcommand)]
pub enum Action {
    /// Initialize versioned workflow state in an existing project.
    Init {
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        #[arg(long, value_enum, value_delimiter = ',', default_value = "opencode,claude,codex")]
        agents: Vec<Agent>,
    },
    /// Inspect agent executables and supported project instructions.
    Doctor,
    /// Preview adapter files. Use --apply to create missing files.
    Sync { #[arg(long)] apply: bool },
    /// Manage canonical tasks.
    Task { #[command(subcommand)] command: TaskCommand },
    /// Freeze the active task and agent-reported progress for another agent.
    Handoff {
        id: u64,
        #[arg(long)] to: Agent,
        #[arg(long)] completed: String,
        #[arg(long)] remaining: String,
        #[arg(long)] blocker: Vec<String>,
        #[arg(long)] assumption: Vec<String>,
    },
    /// Read the latest handoff and check its revision and task freshness.
    Resume { #[arg(long)] accept_as: Option<Agent> },
    /// Save a named copy of the current canonical state.
    Snapshot { name: String },
    /// Show the ordered journal of workflow changes.
    Journal,
}

#[derive(Debug, Subcommand)]
pub enum TaskCommand {
    Add {
        title: String,
        #[arg(long)] after: Vec<u64>,
        #[arg(long)] accept: Vec<String>,
        #[arg(long)] reference: Vec<String>,
    },
    Ready { id: u64 },
    Start { id: u64, #[arg(long)] agent: Agent },
    Set { id: u64, status: Status },
    List,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Status { Backlog, Ready, InProgress, Verification, Done, Blocked, Cancelled }

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Task {
    id: u64, title: String, status: Status, owner: Option<Agent>,
    after: Vec<u64>, acceptance: Vec<String>, references: Vec<String>, generation: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct Handoff {
    task: Task, to: Agent, completed: String, remaining: String,
    blockers: Vec<String>, assumptions: Vec<String>, revision: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct State {
    schema_version: u32, stacks: Vec<String>, agents: Vec<Agent>,
    legacy_handoff: Option<String>, tasks: Vec<Task>, events: Vec<String>, handoff: Option<Handoff>,
}

const STATE: &str = ".ghost/state.json";

pub(super) fn safe_path(root: &Path, path: &Path) -> Result<()> {
    let mut current = root.to_owned();
    for component in path.strip_prefix(root)?.components() {
        let std::path::Component::Normal(name) = component else { bail!("invalid project path") };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(m) => ensure!(!m.is_symlink(), "refusing symlink {}", current.display()),
            Err(e) if e.kind() == ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

pub(super) fn write_atomic(path: &Path, bytes: &[u8], replace: bool) -> Result<()> {
    let parent = path.parent().context("missing parent")?;
    let (temp, mut file) = (0u64..).find_map(|i| {
        let temp = parent.join(format!(".ghost-tmp-{}-{i}", std::process::id()));
        match File::create_new(&temp) {
            Ok(file) => Some(Ok((temp, file))),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => None,
            Err(e) => Some(Err(e)),
        }
    }).unwrap()?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        if replace { fs::rename(&temp, path)?; } else { fs::hard_link(&temp, path)?; }
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if temp.exists() { fs::remove_file(temp)?; }
    result
}

fn load(root: &Path) -> Result<State> {
    let path = root.join(STATE);
    safe_path(root, &path)?;
    let value: serde_json::Value = serde_json::from_slice(&fs::read(path).context("run ghost init first")?)?;
    ensure!(value["schema_version"].as_u64() == Some(1), "unsupported workflow schema");
    Ok(serde_json::from_value(value)?)
}

fn lock(root: &Path, init: bool) -> Result<File> {
    let path = root.join(".ghost/workflow.lock");
    safe_path(root, &path)?;
    if init { fs::create_dir_all(root.join(".ghost"))?; }
    let file = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(path)?;
    file.lock()?;
    Ok(file)
}

fn save(root: &Path, state: &State, replace: bool) -> Result<()> {
    let path = root.join(STATE);
    safe_path(root, &path)?;
    let mut content = serde_json::to_vec_pretty(state)?;
    content.push(b'\n');
    write_atomic(&path, &content, replace)
}

fn init(root: &Path, agents: Vec<Agent>) -> Result<()> {
    let _guard = lock(root, true)?;
    if root.join(STATE).try_exists()? {
        load(root)?;
        println!("already initialized");
        return Ok(());
    }
    let mut stacks = Vec::new();
    for (name, markers) in [("rust", &["Cargo.toml"][..]), ("python", &["pyproject.toml", "requirements.txt", "setup.py"]), ("node", &["package.json"])] {
        if markers.iter().any(|p| root.join(p).is_file()) { stacks.push(name.to_owned()); }
    }
    let legacy = root.join("STATE.md");
    safe_path(root, &legacy)?;
    let legacy_handoff = match fs::read_to_string(legacy) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let state = State { schema_version: 1, stacks, agents, legacy_handoff, tasks: vec![], events: vec!["initialized workflow".into()], handoff: None };
    save(root, &state, false)?;
    println!("initialized; preview agent instructions with ghost sync");
    Ok(())
}

fn revision(root: &Path) -> Option<String> {
    let output = Command::new("git").current_dir(root).args(["rev-parse", "--verify", "HEAD"]).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn lookup(state: &State, id: u64) -> Result<usize> {
    state.tasks.iter().position(|t| t.id == id).context("unknown task ID")
}

fn transition(state: &mut State, id: u64, next: Status, agent: Option<Agent>) -> Result<()> {
    use Status::*;
    let i = lookup(state, id)?;
    let old = state.tasks[i].status;
    ensure!(matches!((old,next), (Backlog,Ready) | (Ready,InProgress) | (InProgress,Verification) | (Verification,Done) | (Verification,InProgress) | (Blocked,Ready))
        || (matches!(next,Blocked | Cancelled) && !matches!(old,Done | Cancelled) && old != next), "invalid transition {old:?} -> {next:?}");
    if matches!(next,Ready | InProgress) {
        for dep in &state.tasks[i].after {
            ensure!(state.tasks[lookup(state,*dep)?].status == Done, "unfinished dependency {dep}");
        }
    }
    if next == InProgress {
        let owner = agent.or(state.tasks[i].owner).context("use task start with --agent")?;
        ensure!(state.agents.contains(&owner), "agent is not configured");
        state.tasks[i].owner = Some(owner);
    }
    state.tasks[i].status = next;
    state.tasks[i].generation += 1;
    state.events.push(format!("task {id}: {old:?} -> {next:?}"));
    Ok(())
}

fn list(state: &State) {
    println!("stacks: {}", state.stacks.join(", "));
    for t in &state.tasks { println!("{} {:?} {}", t.id, t.status, t.title); }
}

pub fn initialized() -> bool { Path::new(STATE).exists() }
pub fn dashboard() -> Result<i32> { list(&load(Path::new("."))?); Ok(0) }

fn execute(root: &Path, action: Action) -> Result<()> {
    if let Action::Init { dir, agents } = action { return init(&root.join(dir).canonicalize()?, agents); }
    let _guard = lock(root, false).context("opening workflow; run ghost init in the project directory")?;
    let mut state = load(root)?;
    match action {
        Action::Init { .. } => unreachable!(),
        Action::Doctor => { adapters::doctor(&state.agents); return Ok(()); }
        Action::Sync { apply } => return adapters::sync(root, &state.agents, apply),
        Action::Journal => { for event in &state.events { println!("{event}"); } return Ok(()); }
        Action::Task { command } => match command {
            TaskCommand::List => { list(&state); return Ok(()); }
            TaskCommand::Add { title, after, accept, reference } => {
                ensure!(!title.trim().is_empty(), "task title is empty");
                for dep in &after { lookup(&state, *dep)?; }
                for path in &reference { safe_path(root, &root.join(path))?; }
                let id = state.tasks.last().map_or(1, |t| t.id + 1);
                state.tasks.push(Task { id, title, status: Status::Backlog, owner: None, after, acceptance: accept, references: reference, generation: 0 });
                state.events.push(format!("task {id}: created"));
            }
            TaskCommand::Ready { id } => transition(&mut state, id, Status::Ready, None)?,
            TaskCommand::Start { id, agent } => transition(&mut state, id, Status::InProgress, Some(agent))?,
            TaskCommand::Set { id, status } => transition(&mut state, id, status, None)?,
        },
        Action::Handoff { id, to, completed, remaining, blocker, assumption } => {
            let task = state.tasks[lookup(&state,id)?].clone();
            ensure!(state.agents.contains(&to), "recipient is not configured");
            ensure!(task.owner.is_some() && !matches!(task.status, Status::Done | Status::Cancelled), "task needs an active owner");
            ensure!(!completed.trim().is_empty() && !remaining.trim().is_empty(), "supply completed and remaining work");
            state.events.push(format!("task {id}: handoff to {}", to.name()));
            state.handoff = Some(Handoff { task, to, completed, remaining, blockers: blocker, assumptions: assumption, revision: revision(root) });
        }
        Action::Snapshot { name } => {
            ensure!(!name.is_empty() && name.len() <= 80 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'), "invalid snapshot name");
            let target = root.join(".ghost").join(format!("checkpoint-{name}.json"));
            safe_path(root, &target)?;
            write_atomic(&target, &serde_json::to_vec_pretty(&state)?, false)?;
            println!("saved {}", target.display());
            return Ok(());
        }
        Action::Resume { accept_as } => {
            if let Some(handoff) = &state.handoff {
                println!("Agent-reported handoff:\n{}", serde_json::to_string_pretty(handoff)?);
                let head = revision(root);
                if head.is_none() || handoff.revision.is_none() { println!("revision unavailable"); }
                else if head != handoff.revision { println!("stale: Git revision changed"); }
                if state.tasks[lookup(&state,handoff.task.id)?].generation != handoff.task.generation { println!("stale: task changed"); }
            } else if let Some(text) = &state.legacy_handoff { println!("Imported STATE.md:\n{text}"); }
            else { bail!("no saved handoff"); }
            if let Some(agent) = accept_as {
                let handoff = state.handoff.as_ref().context("no structured handoff")?;
                ensure!(agent == handoff.to, "recipient differs from handoff");
                ensure!(revision(root) == handoff.revision, "revision changed; refresh handoff");
                let i = lookup(&state, handoff.task.id)?;
                ensure!(state.tasks[i].generation == handoff.task.generation, "task changed; refresh handoff");
                state.tasks[i].owner = Some(agent);
                state.tasks[i].generation += 1;
                state.events.push(format!("task {}: accepted by {}", state.tasks[i].id, agent.name()));
            } else { return Ok(()); }
        }
    }
    save(root, &state, true)?;
    println!("{}", state.events.last().unwrap());
    Ok(())
}

pub fn run(action: Action) -> Result<i32> { execute(&std::env::current_dir()?, action)?; Ok(0) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_handoff_and_checks_dependencies_after_reopen() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        fs::write(root.join("STATE.md"), "pending review")?;
        init(root, vec![Agent::Codex, Agent::Claude])?;
        assert_eq!(load(root)?.legacy_handoff.as_deref(), Some("pending review"));
        let task = |command| execute(root, Action::Task { command });
        for (title, after) in [("first", vec![]), ("second", vec![1])] {
            task(TaskCommand::Add { title: title.into(), after, accept: vec![], reference: vec![] })?;
        }
        assert!(task(TaskCommand::Ready { id: 2 }).is_err());
        task(TaskCommand::Ready { id: 1 })?;
        task(TaskCommand::Start { id: 1, agent: Agent::Codex })?;
        execute(root, Action::Handoff { id: 1, to: Agent::Claude, completed: "design".into(),
            remaining: "review".into(), blocker: vec!["approval".into()], assumption: vec![] })?;
        assert!(execute(root, Action::Resume { accept_as: Some(Agent::Codex) }).is_err());
        execute(root, Action::Resume { accept_as: Some(Agent::Claude) })?;
        assert_eq!(load(root)?.tasks[0].owner, Some(Agent::Claude));
        assert!(execute(root, Action::Resume { accept_as: Some(Agent::Claude) }).is_err());
        for status in [Status::Verification, Status::Done] {
            task(TaskCommand::Set { id: 1, status })?;
        }
        task(TaskCommand::Ready { id: 2 })?;
        execute(root, Action::Snapshot { name: "reviewed".into() })?;
        assert!(execute(root, Action::Snapshot { name: "reviewed".into() }).is_err());
        Ok(())
    }
}
