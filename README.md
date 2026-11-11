# ghost

`ghost` generates a small, project-level workflow for [OpenCode](https://opencode.ai).
It exists to stop two common failures in long agent sessions: spending tokens on
context the agent does not need, and losing track of what was done and what comes next.

One short interview writes the project facts, a disk-backed `STATE.md`, one spec per
feature, deterministic check/dev scripts, and seven project slash commands. `ghost`
makes no LLM calls and has no network access. State lives in files, so a fresh session
can pick up where the last one stopped.

## Install

### From crates.io

```bash
cargo install ghost-workflow --locked
```

The package is named `ghost-workflow`; the installed binary is `ghost`.

### From GitHub Releases

Download the archive for your platform from the repository's Releases page:

- `ghost-x86_64-unknown-linux-gnu.tar.gz`
- `ghost-aarch64-apple-darwin.tar.gz`
- `ghost-x86_64-apple-darwin.tar.gz`

Then unpack it and put `ghost` on your `PATH`:

```bash
tar -xzf ghost-x86_64-unknown-linux-gnu.tar.gz
install -m 755 ghost ~/.local/bin/ghost
```

### From source

```bash
cargo install --path . --locked
```

If crates.io is unreachable, use the local crate cache:

```bash
cargo install --path . --locked --offline
```

Requires stable Rust and a Unix-like system (Ubuntu is the tested target).

## Quick start

```bash
cd my-project
ghost new
ghost status
opencode          # then run /plan, /build <slug>, ...
```

`ghost new --answers answers.json` skips the interview, which is useful for scripts
and tests.

## Commands

| Command | What it does | Why it helps |
| --- | --- | --- |
| `ghost new [--dir .] [--force] [--answers file]` | Interviews you and generates project files. Refuses to overwrite existing files unless `--force`. | Keeps generated context small and project-specific. |
| `ghost regen` | Re-renders generated files from `.ghost/answers.json`. | Recovers after editing answers or changing templates without re-interviewing. |
| `ghost status` | Prints a compact dashboard (countdown, focus, next, blockers, dev server, state size). | One cheap glance instead of loading the whole project into chat. |
| `ghost check [--timeout 300]` | Runs `scripts/check.sh` in its own process group and kills the group on timeout. | Prevents full logs in chat and leaves no orphan processes. |
| `ghost down` | Runs `scripts/dev.sh stop`. | Stops only the server ghost started, by its recorded process group. |
| `ghost checkpoint [msg]` | Makes a local git commit. Refuses `.env*`, `*.pem`, `*.key`, and `id_rsa*` files. | A safe restore point before a risky change; never pushes. |

Every command except `new` exits 1 with a friendly message when `.ghost/answers.json`
is missing.

## Generated files

```text
AGENTS.md                       # <= 40 lines, project-specific only
STATE.md                        # focus, next, done, decisions, blockers
specs/00-overview.md            # pitch, demo script, feature list
specs/feature-<slug>.md         # one spec per must-have, Gherkin skeleton
specs/schema.yaml               # flat schema notes, mostly TODO
scripts/check.sh                # short stage summary, last 40 lines on failure
scripts/dev.sh                  # start | stop | status, PID-tracked
.opencode/commands/plan.md      # plus build, fix, test, review, handoff, checkpoint
.ghost/answers.json             # the interview result
opencode.json                   # only when a model override was entered
```

`ghost new` also appends these lines to `.gitignore` without overwriting existing
content and without duplicating them:

```text
.env
.env.*
.ghost/dev.pid
.ghost/dev.log
```

## How it saves tokens and prevents lost work

- `AGENTS.md` is capped at 40 lines and contains project facts, not a second copy of
  the global workflow.
- `STATE.md` is the handoff point. `/handoff` rewrites it and tells you it is safe to
  start a fresh session.
- Features are split into independent specs. `/build <slug>` reads only that spec.
- `scripts/check.sh` prints `ok <stage>` or a failure summary, never a wall of logs.
- `scripts/dev.sh` records a PID and kills only that process group, so servers do not
  survive a forgotten session.
- `ghost checkpoint` refuses likely secrets before creating a local commit.

## `answers.json`

The file is pretty-printed JSON. All fields are required except `nice_to_haves`,
`model_override`, `run_cmd`, and `stack_details`. Example:

```json
{
  "project_name": "Orbit",
  "pitch": "A tiny dashboard for demo day.",
  "demo_steps": ["Open the dashboard", "Add a task", "See progress"],
  "event_name": "HackMIT 2026",
  "deadline": "2099-11-14 10:00",
  "team_size": "solo",
  "mode": "hackathon",
  "stack": "rust",
  "stack_details": "axum 0.8, serde 1",
  "run_cmd": "cargo run",
  "must_haves": ["Sign in", "Task board", "Export"],
  "nice_to_haves": ["Dark mode"],
  "existing_code": false,
  "risky_areas": ["auth"],
  "test_level": "smoke",
  "model_override": null,
  "skills": ["tdd"],
  "checkpoint_freq": "before_every",
  "created_at": "2026-01-01T09:00:00+00:00"
}
```

Deadlines are local time in `YYYY-MM-DD HH:MM` and must be in the future when running
`ghost new`. `created_at` is the progress start for `ghost status`. Enumerations use
snake_case (`hackathon`, `typescript_node`, `per_feature`, etc.).

## Relationship to the global OpenCode config

`ghost` writes project files only. It never reads or edits anything under
`~/.config/opencode/` beyond listing agent and skill directory names.

- Project and global `AGENTS.md` are both loaded by OpenCode.
- Commands are project commands and intentionally do not copy global rules.
- Command frontmatter selects an existing agent only when that agent is found in
  `~/.config/opencode/agents/`. Missing agents fall back to OpenCode's default.
- With an empty `model_override`, nothing generated sets a model. You type a model
  only if you want one; the recommended choice is to inherit the global model.
- `opencode.json` is written only when `model_override` is set, and contains only
  `$schema` and `model`.

## Assumptions

Verified in the OpenCode docs (commands, rules, agents, config):

- Project commands live in `.opencode/commands/<name>.md`.
- Command frontmatter keys are `description`, `agent`, and `model`; the body is the
  prompt and `$ARGUMENTS` expands to the full argument string.
- Project and global `AGENTS.md` are both applied.
- Agents live in `~/.config/opencode/agents/<name>.md`, and `opencode.json` is a valid
  project config name.

Chosen without stronger evidence:

- The agent names are conventional: `build`, `tester`, `reviewer`. If a name is not
  present, its command omits the `agent:` line.
- Skill discovery lists directory names under `~/.agents/skills/` and
  `~/.config/opencode/skills/`; `GHOST_OPENCODE_DIR` and `GHOST_AGENTS_DIR` override
  those locations for tests.
- Hackathon ordering uses the interview's must-have order as the demo order.
- `run_cmd` is executed with `bash -c` inside `setsid`; logs go to `.ghost/dev.log`.
- `opencode.json` is overwritten by `regen` when a model override is set, because it is
  treated as a generated file.
- `STATE.md` pre-fills `/plan` plus one `/build` per must-have. With five or six
  must-haves this exceeds the stated five-checkbox guidance until `/handoff` trims it.
- Quality checks are best-effort: missing tools and missing npm scripts are skipped with
  a message rather than failing the run.

## Known limitations

- Unix-only. Windows is not supported.
- One project per directory; no multi-project view.
- State is a single local JSON file. If teammates commit it, pull before editing and
  resolve conflicts by keeping one coherent state.
- `regen` overwrites generated files but does not delete specs for features removed
  from `answers.json`.
- `ghost checkpoint` assumes `git` is installed and the project is already a repository.
- No model validation: a typed `provider/model` is passed through to OpenCode.
- No LLM calls, networking, notifications, themes, plugins, or mouse support.

## Publishing

GitHub Releases are built by `.github/workflows/release.yml` when a `v*` tag is
pushed:

```bash
git tag v0.1.0
git push origin master --tags
```

Publishing to crates.io is free. The package name is `ghost-workflow` because the
name `ghost` is already owned by another crate. To publish a new version:

```bash
cargo login
cargo publish --locked
```

You can validate the package first with `cargo publish --dry-run --locked`.

## License

MIT. See [LICENSE](LICENSE).

