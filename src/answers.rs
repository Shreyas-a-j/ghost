use anyhow::{ensure, Context, Result};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub const DEADLINE_FORMAT: &str = "%Y-%m-%d %H:%M";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Hackathon,
    Normal,
}

impl Mode {
    pub fn line(self) -> &'static str {
        match self {
            Mode::Hackathon => {
                "A working demo comes first. Smoke tests cover the demo path. Skip brainstorming \
                 and specialist delegation unless blocked. Do one review pass before submission."
            }
            Mode::Normal => "Follow the global workflow.",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Hackathon => "Hackathon sprint",
            Mode::Normal => "Normal project",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TeamSize {
    Solo,
    Small,
    Large,
}

impl TeamSize {
    pub fn label(self) -> &'static str {
        match self {
            TeamSize::Solo => "Solo",
            TeamSize::Small => "2-3",
            TeamSize::Large => "4+",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stack {
    Rust,
    Python,
    #[serde(rename = "typescript_node")]
    TypeScriptNode,
    Other,
}

impl Stack {
    pub fn label(self) -> &'static str {
        match self {
            Stack::Rust => "Rust",
            Stack::Python => "Python",
            Stack::TypeScriptNode => "TypeScript / Node",
            Stack::Other => "Other",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestLevel {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "smoke")]
    Smoke,
    #[serde(rename = "per_feature")]
    PerFeature,
}

impl TestLevel {
    pub fn label(self) -> &'static str {
        match self {
            TestLevel::None => "None",
            TestLevel::Smoke => "Smoke tests only",
            TestLevel::PerFeature => "Tests per feature",
        }
    }

    pub fn runs_tests(self) -> bool {
        !matches!(self, TestLevel::None)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckpointFreq {
    #[serde(rename = "before_every")]
    BeforeEvery,
    #[serde(rename = "before_risky")]
    BeforeRisky,
    #[serde(rename = "manual")]
    Manual,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskArea {
    Auth,
    Payments,
    UserData,
    ExternalApis,
    FileUploads,
    None,
}

impl RiskArea {
    pub fn label(self) -> &'static str {
        match self {
            RiskArea::Auth => "auth",
            RiskArea::Payments => "payments",
            RiskArea::UserData => "user data",
            RiskArea::ExternalApis => "external APIs/keys",
            RiskArea::FileUploads => "file uploads",
            RiskArea::None => "none",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Answers {
    pub project_name: String,
    pub pitch: String,
    pub demo_steps: Vec<String>,
    pub event_name: String,
    pub deadline: String,
    pub team_size: TeamSize,
    pub mode: Mode,
    pub stack: Stack,
    pub stack_details: String,
    pub run_cmd: String,
    pub must_haves: Vec<String>,
    pub nice_to_haves: Vec<String>,
    pub existing_code: bool,
    pub risky_areas: Vec<RiskArea>,
    pub test_level: TestLevel,
    pub model_override: Option<String>,
    pub skills: Vec<String>,
    pub checkpoint_freq: CheckpointFreq,
    pub created_at: DateTime<Local>,
}

impl Answers {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("could not read answers file {}", path.display()))?;
        let answers: Answers = serde_json::from_str(&raw)
            .with_context(|| format!("could not parse answers file {}", path.display()))?;
        Ok(answers)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
        let raw = serde_json::to_string_pretty(self).context("could not serialize answers")?;
        fs::write(path, format!("{raw}\n"))
            .with_context(|| format!("could not write {}", path.display()))?;
        Ok(())
    }

    pub fn validate_for_new(&self, now: DateTime<Local>) -> Result<()> {
        self.validate_structure()?;
        let deadline = self.deadline_at()?;
        ensure!(
            deadline > now,
            "deadline {} must be in the future",
            self.deadline
        );
        Ok(())
    }

    pub fn validate_structure(&self) -> Result<()> {
        ensure!(
            !self.project_name.trim().is_empty(),
            "project_name must not be empty"
        );
        ensure!(!self.pitch.trim().is_empty(), "pitch must not be empty");
        ensure!(
            self.demo_steps.len() == 3,
            "demo_steps must contain exactly 3 steps"
        );
        ensure!(
            self.demo_steps.iter().all(|s| !s.trim().is_empty()),
            "demo_steps must not contain empty steps"
        );
        ensure!(
            (1..=6).contains(&self.must_haves.len()),
            "must_haves must contain 1 to 6 features"
        );
        ensure!(
            self.must_haves.iter().all(|s| !s.trim().is_empty()),
            "must_haves must not contain empty names"
        );
        ensure!(
            self.nice_to_haves.len() <= 6,
            "nice_to_haves must contain at most 6 features"
        );
        ensure!(
            self.nice_to_haves.iter().all(|s| !s.trim().is_empty()),
            "nice_to_haves must not contain empty names"
        );
        ensure!(
            self.skills.len() <= 5,
            "at most 5 skills may be selected"
        );
        if let Some(model) = &self.model_override {
            ensure!(
                !model.trim().is_empty(),
                "model_override must be absent or non-empty"
            );
        }
        let mut seen = HashSet::new();
        for feature in &self.must_haves {
            ensure!(
                seen.insert(slugify(feature)),
                "must-have features must have unique slugs (duplicate: {feature})"
            );
        }
        self.deadline_at()?;
        Ok(())
    }

    pub fn deadline_at(&self) -> Result<DateTime<Local>> {
        parse_local_datetime(&self.deadline)
    }

    pub fn risky_label(&self) -> String {
        let areas: Vec<&str> = self
            .risky_areas
            .iter()
            .filter(|a| **a != RiskArea::None)
            .map(|a| a.label())
            .collect();
        if areas.is_empty() {
            "none".to_string()
        } else {
            areas.join(", ")
        }
    }

    pub fn skills_label(&self) -> String {
        self.skills
            .iter()
            .map(|s| format!("`{s}`"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn stack_line(&self) -> String {
        if self.stack_details.trim().is_empty() {
            self.stack.label().to_string()
        } else {
            format!("{} ({})", self.stack.label(), self.stack_details.trim())
        }
    }

    pub fn model_line(&self) -> Option<&str> {
        self.model_override
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
    }
}

pub fn parse_local_datetime(input: &str) -> Result<DateTime<Local>> {
    let naive = NaiveDateTime::parse_from_str(input.trim(), DEADLINE_FORMAT)
        .with_context(|| format!("expected local time formatted YYYY-MM-DD HH:MM, got {input:?}"))?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .with_context(|| format!("local time {input:?} does not exist in this timezone"))
}

pub fn slugify(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "feature".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Answers {
        Answers {
            project_name: "Demo".into(),
            pitch: "A tiny demo.".into(),
            demo_steps: vec!["first".into(), "second".into(), "third".into()],
            event_name: "HackMIT".into(),
            deadline: "2099-01-01 10:00".into(),
            team_size: TeamSize::Solo,
            mode: Mode::Hackathon,
            stack: Stack::Rust,
            stack_details: "axum 0.8".into(),
            run_cmd: String::new(),
            must_haves: vec!["Auth & login".into()],
            nice_to_haves: vec![],
            existing_code: false,
            risky_areas: vec![RiskArea::Auth],
            test_level: TestLevel::Smoke,
            model_override: None,
            skills: vec![],
            checkpoint_freq: CheckpointFreq::Manual,
            created_at: Local.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap(),
        }
    }

    #[test]
    fn slugify_edge_cases() {
        assert_eq!(slugify("Auth & Login!"), "auth-login");
        assert_eq!(slugify("--Hello__World--"), "hello-world");
        assert_eq!(slugify("   "), "feature");
        assert_eq!(slugify("Café"), "caf");
        assert_eq!(slugify("App Store!! (v2)"), "app-store-v2");
    }

    #[test]
    fn deadline_validation() {
        let mut answers = sample();
        let now = Local.with_ymd_and_hms(2026, 1, 1, 10, 0, 0).unwrap();
        assert!(answers.validate_for_new(now).is_ok());

        answers.deadline = "2020-01-01 10:00".into();
        let err = answers.validate_for_new(now).unwrap_err().to_string();
        assert!(err.contains("future"), "{err}");

        answers.deadline = "not a date".into();
        assert!(answers.validate_for_new(now).is_err());

        answers.deadline = "2099-01-01 10:00".into();
        answers.must_haves = vec!["Same Name".into(), "same-name".into()];
        assert!(answers.validate_structure().is_err());
    }

    #[test]
    fn parses_local_datetime() {
        let parsed = parse_local_datetime("2099-11-14 10:00").unwrap();
        assert_eq!(parsed.format("%Y-%m-%d %H:%M").to_string(), "2099-11-14 10:00");
        assert!(parse_local_datetime("2099-11-14T10:00").is_err());
    }

    #[test]
    fn answers_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".ghost/answers.json");
        let answers = sample();
        answers.save(&path).unwrap();
        let loaded = Answers::load(&path).unwrap();
        assert_eq!(loaded, answers);
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\n  \"project_name\""));
    }

    #[test]
    fn rejects_too_many_skills() {
        let mut answers = sample();
        answers.skills = (0..6).map(|n| format!("skill-{n}")).collect();
        assert!(answers.validate_structure().is_err());
    }
}
