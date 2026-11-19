mod adapter;
mod answers;
mod cmds;
mod events;
mod handoff;
mod interview;
mod opencode;
mod persist;
mod project;
mod render;
mod state;
mod status;
mod tasks;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(
    name = "ghost",
    version,
    about = "A local-first workflow and context layer for coding agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Interview, then generate project files
    New(NewArgs),
    /// Initialize a new or existing repository without hackathon assumptions
    Init(InitArgs),
    /// Re-render generated files from .ghost/answers.json
    Regen {
        /// Overwrite files changed since the last generation
        #[arg(long)]
        force: bool,
        /// Adapter to generate for; defaults to the saved selection. Repeatable.
        #[arg(long = "adapter", value_name = "NAME", action = clap::ArgAction::Append)]
        adapters: Vec<String>,
    },
    /// Show a compact project dashboard
    Status,
    /// Show coding-agent adapters, detection, and capabilities
    Adapters {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Run scripts/check.sh with a timeout
    Check {
        /// Timeout in seconds
        #[arg(long, default_value_t = 300)]
        timeout: u64,
    },
    /// Stop the dev server tracked by scripts/dev.sh
    Down,
    /// Create and manage structured tasks
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    /// Persist a cross-agent handoff snapshot
    Handoff {
        /// Short note included in the snapshot
        #[arg(long)]
        note: Option<String>,
    },
    /// Show the last handoff and flag stale context
    Resume,
    /// Make a local git checkpoint commit
    Checkpoint {
        /// Checkpoint message
        msg: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
enum TaskCommand {
    /// Create a task in backlog
    Add {
        /// Task title
        title: Vec<String>,
        /// Agent that owns the task
        #[arg(long)]
        owner: Option<String>,
        /// File or path the task depends on; repeatable
        #[arg(long = "ref")]
        references: Vec<String>,
        /// Acceptance check; repeatable
        #[arg(long = "check")]
        checks: Vec<String>,
        /// Print the task as JSON
        #[arg(long)]
        json: bool,
    },
    /// Move a task to in_progress and record its base revision
    Start {
        /// Task id such as TASK-0001
        id: String,
        /// Agent that owns the task
        #[arg(long)]
        owner: Option<String>,
    },
    /// Move a task to another state
    Move {
        /// Task id such as TASK-0001
        id: String,
        /// Target state: ready, in_progress, verification, done, blocked, cancelled
        status: String,
    },
    /// List tasks
    List {
        /// Print tasks as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show one task
    Show {
        /// Task id such as TASK-0001
        id: String,
        /// Print the task as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(clap::Args, Debug)]
struct NewArgs {
    /// Target directory
    #[arg(long, default_value = ".")]
    dir: PathBuf,
    /// Overwrite generated files
    #[arg(long)]
    force: bool,
    /// Read answers from JSON instead of prompting
    #[arg(long)]
    answers: Option<PathBuf>,
    /// Adapter to generate for (opencode, claude, codex). Repeatable.
    #[arg(long = "adapter", value_name = "NAME", action = clap::ArgAction::Append)]
    adapters: Vec<String>,
}

#[derive(clap::Args, Debug)]
struct InitArgs {
    /// Target directory
    #[arg(long, default_value = ".")]
    dir: PathBuf,
    /// Overwrite generated files that changed or were not created by ghost
    #[arg(long)]
    force: bool,
    /// Workflow profile
    #[arg(long, value_enum, default_value = "normal")]
    profile: ProfileArg,
    /// Override the detected project name
    #[arg(long)]
    name: Option<String>,
    /// Read answers from JSON instead of using detection
    #[arg(long)]
    answers: Option<PathBuf>,
    /// Adapter to generate for (opencode, claude, codex). Repeatable.
    #[arg(long = "adapter", value_name = "NAME", action = clap::ArgAction::Append)]
    adapters: Vec<String>,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum ProfileArg {
    Normal,
    Hackathon,
}

impl From<ProfileArg> for answers::Mode {
    fn from(profile: ProfileArg) -> Self {
        match profile {
            ProfileArg::Normal => answers::Mode::Normal,
            ProfileArg::Hackathon => answers::Mode::Hackathon,
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(err) => {
            eprintln!("ghost: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<i32> {
    let cli = Cli::parse();
    match cli.command {
        Command::New(args) => cmds::new_project(
            &args.dir,
            args.force,
            args.answers.as_deref(),
            &args.adapters,
        ),
        Command::Init(args) => cmds::init_project(
            &args.dir,
            args.force,
            args.profile.into(),
            args.name.as_deref(),
            args.answers.as_deref(),
            &args.adapters,
        ),
        Command::Regen { force, adapters } => cmds::regen(force, &adapters),
        Command::Status => cmds::status(),
        Command::Adapters { json } => cmds::adapters(json),
        Command::Check { timeout } => cmds::check(timeout),
        Command::Down => cmds::down(),
        Command::Task { command } => match command {
            TaskCommand::Add {
                title,
                owner,
                references,
                checks,
                json,
            } => cmds::task_add(&title, owner.as_deref(), &references, &checks, json),
            TaskCommand::Start { id, owner } => cmds::task_start(&id, owner.as_deref()),
            TaskCommand::Move { id, status } => cmds::task_move(&id, &status),
            TaskCommand::List { json } => cmds::task_list(json),
            TaskCommand::Show { id, json } => cmds::task_show(&id, json),
        },
        Command::Handoff { note } => cmds::handoff(note.as_deref()),
        Command::Resume => cmds::resume(),
        Command::Checkpoint { msg } => cmds::checkpoint(&msg),
    }
}
