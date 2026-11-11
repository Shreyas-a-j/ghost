use crate::answers::{
    parse_local_datetime, slugify, Answers, CheckpointFreq, Mode, RiskArea, Stack, TeamSize,
    TestLevel, DEADLINE_FORMAT,
};
use crate::opencode;
use anyhow::{Context, Result};
use chrono::{Duration, Local};
use console::style;
use dialoguer::{Confirm, Input, MultiSelect, Select};
use std::collections::HashSet;
use std::path::Path;

pub fn run(dir: &Path) -> Result<Answers> {
    println!();
    println!("{}", style("ghost new").bold());
    println!("A short interview. Answers are saved to .ghost/answers.json.");
    println!();

    let default_name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty() && *n != ".")
        .unwrap_or("project")
        .to_string();

    let project_name = required(&format!("Project name [{default_name}]"), Some(&default_name))?;
    let pitch = required("Pitch (one sentence)", None)?;

    let mut demo_steps = Vec::new();
    for n in 1..=3 {
        demo_steps.push(required(&format!("Demo step {n} (what a judge sees)"), None)?);
    }

    let event_name = required("Event name", Some("Hackathon"))?;
    let deadline = ask_deadline()?;

    let team_size = match Select::new()
        .with_prompt("Team size")
        .items(["Solo", "2-3", "4+"])
        .default(0)
        .interact()?
    {
        0 => TeamSize::Solo,
        1 => TeamSize::Small,
        _ => TeamSize::Large,
    };

    let mode = match Select::new()
        .with_prompt("Mode")
        .items(["Hackathon sprint", "Normal project"])
        .default(0)
        .interact()?
    {
        0 => Mode::Hackathon,
        _ => Mode::Normal,
    };

    let stack = match Select::new()
        .with_prompt("Stack")
        .items(["Rust", "Python", "TypeScript-Node", "Other"])
        .default(0)
        .interact()?
    {
        0 => Stack::Rust,
        1 => Stack::Python,
        2 => Stack::TypeScriptNode,
        _ => Stack::Other,
    };

    let stack_details = optional("Stack details: libraries and frameworks with versions, comma-separated")?;
    let run_cmd = optional("Dev server command (blank if none)")?;

    let must_haves = ask_features("Must-have feature", true)?;
    let nice_to_haves = ask_features("Nice-to-have feature", false)?;

    let existing_code = Confirm::new()
        .with_prompt("Does this directory already contain code?")
        .default(false)
        .interact()?;

    let risky_areas = ask_risky_areas()?;

    let test_level = match Select::new()
        .with_prompt("Test level")
        .items(["None", "Smoke tests only", "Tests per feature"])
        .default(1)
        .interact()?
    {
        0 => TestLevel::None,
        1 => TestLevel::Smoke,
        _ => TestLevel::PerFeature,
    };

    let model_override = {
        let value = optional(
            "Model override (blank inherits the global model; example format provider/model)",
        )?;
        if value.trim().is_empty() {
            None
        } else {
            Some(value.trim().to_string())
        }
    };

    let skills = ask_skills()?;

    let checkpoint_freq = match Select::new()
        .with_prompt("Checkpoint frequency")
        .items([
            "Before every /build",
            "Before risky edits only",
            "Manual",
        ])
        .default(0)
        .interact()?
    {
        0 => CheckpointFreq::BeforeEvery,
        1 => CheckpointFreq::BeforeRisky,
        _ => CheckpointFreq::Manual,
    };

    let answers = Answers {
        project_name: project_name.trim().to_string(),
        pitch: pitch.trim().to_string(),
        demo_steps,
        event_name: event_name.trim().to_string(),
        deadline,
        team_size,
        mode,
        stack,
        stack_details,
        run_cmd,
        must_haves,
        nice_to_haves,
        existing_code,
        risky_areas,
        test_level,
        model_override,
        skills,
        checkpoint_freq,
        created_at: Local::now(),
    };
    answers.validate_structure()?;
    Ok(answers)
}

fn required(prompt: &str, default: Option<&str>) -> Result<String> {
    let mut input = Input::<String>::new().with_prompt(prompt);
    if let Some(default) = default {
        input = input.default(default.to_string());
    }
    loop {
        let value = input.clone().interact_text().context("input cancelled")?;
        if !value.trim().is_empty() {
            return Ok(value.trim().to_string());
        }
        eprintln!("please enter a value");
    }
}

fn optional(prompt: &str) -> Result<String> {
    Ok(Input::<String>::new()
        .with_prompt(prompt)
        .allow_empty(true)
        .interact_text()
        .context("input cancelled")?
        .trim()
        .to_string())
}

fn ask_deadline() -> Result<String> {
    let default = (Local::now() + Duration::days(7))
        .format(DEADLINE_FORMAT)
        .to_string();
    loop {
        let value = Input::<String>::new()
            .with_prompt(format!("Deadline ({DEADLINE_FORMAT}, local time)"))
            .default(default.clone())
            .interact_text()
            .context("input cancelled")?;
        match parse_local_datetime(&value) {
            Ok(deadline) if deadline > Local::now() => return Ok(value),
            Ok(_) => eprintln!("deadline must be in the future"),
            Err(err) => eprintln!("{err}"),
        }
    }
}

fn ask_features(label: &str, required: bool) -> Result<Vec<String>> {
    let mut features = Vec::new();
    let mut slugs = HashSet::new();
    while features.len() < 6 {
        let prompt = if features.is_empty() && required {
            format!("{label} 1 (required)")
        } else {
            format!("{label} {} (blank to finish)", features.len() + 1)
        };
        let value = optional(&prompt).context("input cancelled")?;
        if value.is_empty() {
            if features.is_empty() && required {
                eprintln!("add at least one must-have feature");
                continue;
            }
            break;
        }
        if !slugs.insert(slugify(&value)) {
            eprintln!("that name duplicates an earlier feature; try a different name");
            continue;
        }
        features.push(value);
    }
    Ok(features)
}

fn ask_risky_areas() -> Result<Vec<RiskArea>> {
    let labels = [
        "auth",
        "payments",
        "user data",
        "external APIs/keys",
        "file uploads",
        "none",
    ];
    let chosen = MultiSelect::new()
        .with_prompt("Risky areas (space to select, enter to confirm)")
        .items(labels)
        .interact()?;
    let mut areas: Vec<RiskArea> = chosen
        .into_iter()
        .map(|idx| match idx {
            0 => RiskArea::Auth,
            1 => RiskArea::Payments,
            2 => RiskArea::UserData,
            3 => RiskArea::ExternalApis,
            4 => RiskArea::FileUploads,
            _ => RiskArea::None,
        })
        .collect();
    if areas.len() > 1 {
        areas.retain(|a| *a != RiskArea::None);
    }
    if areas.is_empty() {
        areas.push(RiskArea::None);
    }
    Ok(areas)
}

fn ask_skills() -> Result<Vec<String>> {
    let available = opencode::discover_skills();
    if available.is_empty() {
        return Ok(Vec::new());
    }
    loop {
        let selected = MultiSelect::new()
            .with_prompt("Relevant skills (up to 5; load only when needed)")
            .items(&available)
            .interact()?;
        if selected.len() > 5 {
            eprintln!("select at most 5 skills");
            continue;
        }
        return Ok(selected
            .into_iter()
            .map(|idx| available[idx].clone())
            .collect());
    }
}
