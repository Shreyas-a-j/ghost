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

pub fn not_ready() -> Result<i32> {
    bail!("this command is implemented in a later milestone")
}
