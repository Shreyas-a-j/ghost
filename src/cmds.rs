use crate::answers::Answers;
use anyhow::{bail, Context, Result};
use chrono::Local;
use std::fs;
use std::path::Path;

pub const ANSWERS_PATH: &str = ".ghost/answers.json";

pub fn new_project(dir: &Path, force: bool, answers_path: Option<&Path>) -> Result<i32> {
    fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    let answers = match answers_path {
        Some(path) => Answers::load(path)?,
        None => crate::interview::run(dir)?,
    };
    answers.validate_for_new(Local::now())?;
    let target = dir.join(ANSWERS_PATH);
    if target.exists() && !force {
        bail!(
            "{} already exists; pass --force to overwrite generated files",
            target.display()
        );
    }
    answers.save(&target)?;
    println!("saved answers to {}", target.display());
    Ok(0)
}

pub fn not_ready() -> Result<i32> {
    bail!("this command is implemented in a later milestone")
}
