mod answers;
mod cmds;
mod interview;
mod opencode;
mod render;
mod status;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(
    name = "ghost",
    version,
    about = "Generate a lean, project-level OpenCode workflow"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Interview, then generate project files
    New(NewArgs),
    /// Re-render generated files from .ghost/answers.json
    Regen,
    /// Show a compact project dashboard
    Status,
    /// Run scripts/check.sh with a timeout
    Check {
        /// Timeout in seconds
        #[arg(long, default_value_t = 300)]
        timeout: u64,
    },
    /// Stop the dev server tracked by scripts/dev.sh
    Down,
    /// Make a local git checkpoint commit
    Checkpoint {
        /// Checkpoint message
        msg: Vec<String>,
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
        Command::New(args) => cmds::new_project(&args.dir, args.force, args.answers.as_deref()),
        Command::Regen => cmds::regen(),
        Command::Status => cmds::status(),
        Command::Check { timeout } => cmds::check(timeout),
        Command::Down => cmds::down(),
        Command::Checkpoint { msg } => cmds::checkpoint(&msg),
    }
}
