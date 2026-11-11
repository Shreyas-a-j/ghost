use crate::answers::Answers;
use anyhow::Result;
use chrono::{DateTime, Local};
use console::{style, Color, Term};
use std::fs;
use std::path::Path;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct StateSections {
    pub focus: Vec<String>,
    pub next: Vec<String>,
    pub done: Vec<String>,
    pub decisions: Vec<String>,
    pub blockers: Vec<String>,
}

#[derive(Clone, Copy)]
enum Section {
    Focus,
    Next,
    Done,
    Decisions,
    Blockers,
}

fn marker(line: &str) -> Option<Section> {
    match line.trim() {
        "<!-- ghost:focus -->" => Some(Section::Focus),
        "<!-- ghost:next -->" => Some(Section::Next),
        "<!-- ghost:done -->" => Some(Section::Done),
        "<!-- ghost:decisions -->" => Some(Section::Decisions),
        "<!-- ghost:blockers -->" => Some(Section::Blockers),
        _ => None,
    }
}

pub fn parse_state(text: &str) -> StateSections {
    let mut sections = StateSections::default();
    let mut current = None;
    for line in text.lines() {
        if let Some(section) = marker(line) {
            current = Some(section);
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("## ") {
            continue;
        }
        match current {
            Some(Section::Focus) => sections.focus.push(trimmed.to_string()),
            Some(Section::Next) => sections.next.push(trimmed.to_string()),
            Some(Section::Done) => sections.done.push(trimmed.to_string()),
            Some(Section::Decisions) => sections.decisions.push(trimmed.to_string()),
            Some(Section::Blockers) => sections.blockers.push(trimmed.to_string()),
            None => {}
        }
    }
    sections
}

pub fn format_remaining(deadline: DateTime<Local>, now: DateTime<Local>) -> (String, bool) {
    let seconds = (deadline - now).num_seconds();
    if seconds <= 0 {
        return ("TIME'S UP".to_string(), true);
    }
    format_seconds(seconds)
}

fn format_seconds(seconds: i64) -> (String, bool) {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let secs = seconds % 60;
    if days > 0 {
        (
            format!("{days}d {hours:02}:{minutes:02}:{secs:02} left"),
            false,
        )
    } else {
        (format!("{hours:02}:{minutes:02}:{secs:02} left"), false)
    }
}

pub fn progress_fraction(answers: &Answers, now: DateTime<Local>) -> f64 {
    let Ok(deadline) = answers.deadline_at() else {
        return 0.0;
    };
    let total = (deadline - answers.created_at).num_seconds();
    if total <= 0 {
        return 1.0;
    }
    let elapsed = (now - answers.created_at).num_seconds();
    (elapsed as f64 / total as f64).clamp(0.0, 1.0)
}

fn gauge(frac: f64, width: usize) -> String {
    let filled = ((frac * width as f64).round() as usize).min(width);
    format!("{}{}", "#".repeat(filled), "-".repeat(width - filled))
}

fn color_name(seconds_left: i64, expired: bool) -> &'static str {
    if expired || seconds_left <= 2 * 3_600 {
        "red"
    } else if seconds_left <= 6 * 3_600 {
        "yellow"
    } else {
        "green"
    }
}

pub fn render(answers: &Answers, state_text: &str, now: DateTime<Local>, dev: &str) -> String {
    let deadline = answers.deadline_at().unwrap_or(now);
    let seconds_left = (deadline - now).num_seconds();
    let (remaining, expired) = format_remaining(deadline, now);
    let frac = progress_fraction(answers, now);
    let percent = (frac * 100.0).round() as u32;
    let color = color_name(seconds_left, expired);
    let tty = Term::stdout().is_term();

    let painted_time = if tty {
        let styled = match color {
            "red" => style(&remaining).fg(Color::Red),
            "yellow" => style(&remaining).fg(Color::Yellow),
            _ => style(&remaining).fg(Color::Green),
        };
        if expired {
            styled.bold().to_string()
        } else {
            styled.to_string()
        }
    } else {
        remaining.clone()
    };

    let sections = parse_state(state_text);
    let focus = sections
        .focus
        .first()
        .map(|line| strip_dash(line))
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| "not set".to_string());
    let next: Vec<String> = sections
        .next
        .iter()
        .filter_map(|line| {
            line.strip_prefix("- [ ]")
                .map(|rest| rest.trim().to_string())
        })
        .take(3)
        .collect();
    let next = if next.is_empty() {
        "none".to_string()
    } else {
        next.join("; ")
    };
    let blockers = sections
        .blockers
        .first()
        .map(|line| strip_dash(line))
        .filter(|line| !line.is_empty() && !line.eq_ignore_ascii_case("none."))
        .unwrap_or_else(|| "none".to_string());

    let mut lines = vec![
        format!(
            "{}   {}   [{}] {}%   ({color})",
            answers.event_name,
            painted_time,
            gauge(frac, 20),
            percent
        ),
        format!("Focus:  {focus}"),
        format!("Next:   {next}"),
        format!("Blockers: {blockers}"),
        format!("Dev:    {dev}"),
    ];
    let state_lines = state_text.lines().count();
    let state_bytes = state_text.len();
    let mut state_line = format!("STATE.md: {state_lines} lines");
    if state_lines > 80 || state_bytes > 4096 {
        state_line.push_str("  (consider /handoff)");
    }
    lines.push(state_line);
    lines.join("\n")
}

fn strip_dash(line: &str) -> String {
    line.trim()
        .strip_prefix("- ")
        .unwrap_or(line.trim())
        .trim()
        .to_string()
}

pub fn dev_status(root: &Path) -> String {
    let path = root.join(".ghost/dev.pid");
    let Ok(raw) = fs::read_to_string(&path) else {
        return "stopped".to_string();
    };
    let Ok(pid) = raw.trim().parse::<u32>() else {
        return "stopped (unreadable pid file)".to_string();
    };
    if process_alive(pid) {
        format!("running (pid {pid})")
    } else {
        "stopped".to_string()
    }
}

pub fn process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub fn run(root: &Path) -> Result<i32> {
    let answers = crate::cmds::load_required(root)?;
    let state_path = root.join("STATE.md");
    let state_text = fs::read_to_string(&state_path).unwrap_or_default();
    let dev = dev_status(root);
    println!("{}", render(&answers, &state_text, Local::now(), &dev));
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_markers_and_sections() {
        let text = "\
# State
<!-- ghost:focus -->
## Current focus
- Building the board
<!-- ghost:next -->
## Next steps
- [ ] `/plan`
- [ ] `/build board`
- [x] done item
<!-- ghost:done -->
## Done
- First task
<!-- ghost:decisions -->
## Decisions
- Use axum because it is small.
<!-- ghost:blockers -->
## Blockers
- None.
";
        let sections = parse_state(text);
        assert_eq!(sections.focus, vec!["- Building the board"]);
        assert_eq!(sections.next.len(), 3);
        assert_eq!(sections.done, vec!["- First task"]);
        assert_eq!(sections.blockers, vec!["- None."]);
    }

    #[test]
    fn empty_and_missing_markers_degrade() {
        assert_eq!(parse_state(""), StateSections::default());
        assert_eq!(
            parse_state("# State\n\nno markers here"),
            StateSections::default()
        );
        let sections = parse_state("<!-- ghost:focus -->\n## Current focus\n");
        assert!(sections.focus.is_empty());
    }

    #[test]
    fn formats_remaining_time() {
        let now = Local::now();
        assert_eq!(
            format_remaining(
                now + chrono::Duration::seconds(14 * 3600 + 32 * 60 + 8),
                now
            )
            .0,
            "14:32:08 left"
        );
        let (text, expired) = format_remaining(now - chrono::Duration::seconds(1), now);
        assert_eq!(text, "TIME'S UP");
        assert!(expired);
    }

    #[test]
    fn gauge_clamps() {
        assert_eq!(gauge(-1.0, 4), "----");
        assert_eq!(gauge(2.0, 4), "####");
        assert_eq!(gauge(0.5, 4), "##--");
    }
}
