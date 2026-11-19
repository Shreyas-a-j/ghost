# ghost

`ghost` is a local-first workflow and context layer for coding agents. It generates
a small, project-level workflow for OpenCode, Claude Code, and Codex without
pretending the agents are interchangeable.

It exists to stop two common failures in long agent sessions: spending tokens on
context the agent does not need, and losing track of what was done and what comes next.

One short interview, or one detection pass over an existing repository, writes the
project facts, a disk-backed `STATE.md` plus canonical `.ghost/state.json`, one spec
per feature, deterministic check/dev scripts, and project commands where the agent
supports them. `ghost` makes no LLM calls and has no network access. State lives in
files, so a fresh session can pick up where the last one stopped.

Ghost 2.0 is being delivered in phases. See [docs/PHASE-STATUS.md](docs/PHASE-STATUS.md)
for what is implemented and what is still proposed.

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

For an existing repository, use detection:

```bash
cd my-project
ghost init                          # OpenCode (default)
ghost init --adapter claude         # or: --adapter codex, repeatable
ghost status
opencode          # then run /plan, /build <slug>, ...
```

For a hackathon or a new project, use the interview:

```bash
cd my-project
ghost new
ghost status
```

`ghost new --answers answers.json` skips the interview, which is useful for scripts
and tests. `ghost init --profile hackathon` uses the interview with the hackathon
profile, while `ghost init` never requires an event, deadline, or demo script.

Adapter selection is saved in `.ghost/config.toml` and reused by `ghost regen`.
Use `ghost adapters` to see what is installed and which surfaces each adapter
declares.

## Profiles

| Profile | Entry point | Required inputs |
| --- | --- | --- |
| `normal` | `ghost init` | Project name and pitch derived from the repository; no deadline or features required |
| `hackathon` | `ghost new`, `ghost init --profile hackathon` | Event, future deadline, three demo steps, 1-6 must-haves |

The normal profile keeps the same generated workflow but drops hackathon wording
from the overview, feature specs, and status dashboard.

## Adapters

| Adapter | Instructions | Commands | Model config | Notes |
| --- | --- | --- | --- | --- |
| `opencode` | `AGENTS.md` | `.opencode/commands/*.md` | `opencode.json` when a model override is set | Agent lines only for agents found locally |
| `claude` | `CLAUDE.md` | `.claude/commands/*.md` | none | Frontmatter uses `description` and `argument-hint` |
| `codex` | `AGENTS.md` | none | none | File-based next steps instead of `/plan` |

Capabilities are declared per adapter rather than assumed to be equal. Ghost does
not write hooks, Claude settings, or Codex project commands. `ghost adapters --json`
prints the same declarations plus local detection results.

```bash
ghost init --adapter opencode --adapter claude
ghost regen --adapter codex        # replace the saved selection
```

Changing the selection does not delete the previous adapter's directory; those
files may contain local edits.

## Commands

| Command | What it does | Why it helps |
| --- | --- | --- |
| `ghost new [--dir .] [--force] [--answers file] [--adapter name]` | Interviews you and generates project files. Refuses to overwrite existing files unless `--force`. | Keeps generated context small and project-specific. |
| `ghost init [--dir .] [--force] [--profile normal\|hackathon] [--name name] [--answers file] [--adapter name]` | Detects Rust, Python, or Node, then generates the workflow without hackathon assumptions. Reuses stored answers; refuses changed files unless `--force`. | Initializes an existing repository without re-interviewing or losing edits. |
| `ghost regen [--force] [--adapter name]` | Re-renders generated files from `.ghost/answers.json` and refreshes canonical metadata. Refuses files changed since generation unless `--force`. | Recovers after editing answers or changing templates. |
| `ghost status` | Prints a compact dashboard (countdown or no-deadline header, focus, next, blockers, dev server, state size). | One cheap glance instead of loading the whole project into chat. |
| `ghost adapters [--json]` | Shows adapter detection, declared capabilities, and install hints. | Distinguishes "not installed" from "not supported" instead of guessing. |
| `ghost check [--timeout 300]` | Runs `scripts/check.sh` in its own process group and kills the group on timeout. | Prevents full logs in chat and leaves no orphan processes. |
| `ghost down` | Runs `scripts/dev.sh stop`. | Stops only the server ghost started, by its recorded process group. |
| `ghost task add <title> [--owner a] [--ref path] [--check cmd]` | Creates a `TASK-####` record in `.ghost/tasks/` with `backlog` status. | Gives work a durable identity across sessions and agents. |
| `ghost task start <id> [--owner a]` | Moves a task to `in_progress` and records its Git base revision. | Makes the active task explicit and comparable later. |
| `ghost task move <id> <status>` | Enforces the documented state machine (`ready`, `in_progress`, `verification`, `done`, `blocked`, `cancelled`). | Rejects skips such as backlog to done. |
| `ghost task list [--json]` / `ghost task show <id> [--json]` | Lists or prints task records. | Machine-readable state for scripts and agents. |
| `ghost handoff [--note text]` | Freezes revision, branch, state, tasks, and evidence ids into `.ghost/handoff.json`. | Lets another agent resume without re-reading everything. |
| `ghost resume` | Prints the last handoff beside the current revision; exits 2 when stale. | Flags changed files instead of silently trusting an old summary. |
| `ghost checkpoint [msg]` | Makes a local git commit and records a checkpoint event. Refuses `.env*`, `*.pem`, `*.key`, and `id_rsa*` files. | A safe restore point before a risky change; never pushes. |
| `ghost context [--task id] [--budget n] [--json]` | Builds a deterministic packet: objective, acceptance checks, blockers, decisions, recent changes, verification history, and budgeted file excerpts. | One focused read instead of loading the repository. |
| `ghost verify [--task id] [--approve] [--timeout n] [--list] [-- cmd]` | Runs an approved command, saves exit code, timings, revision, output tail, and source hashes to `.ghost/evidence/`. | Makes checks evidence-backed and stale results visible. |

`--adapter` is repeatable. With no flag, Ghost reuses the saved selection or
defaults to OpenCode. Every command except `new`, `init`, and `adapters` exits 1
with a friendly message when `.ghost/answers.json` is missing.

## Generated files

```text
AGENTS.md                       # <= 40 lines; OpenCode and/or Codex
CLAUDE.md                       # <= 40 lines; Claude Code only
STATE.md                        # human-readable view of canonical state
specs/00-overview.md            # profile-aware flow, feature list
specs/feature-<slug>.md         # one spec per must-have, Gherkin skeleton
specs/schema.yaml               # flat schema notes, mostly TODO
scripts/check.sh                # short stage summary, last 40 lines on failure
scripts/dev.sh                  # start | stop | status, PID-tracked
.opencode/commands/plan.md      # OpenCode only; plus build, fix, test, review, handoff, checkpoint
.claude/commands/plan.md        # Claude Code only; same seven commands
.ghost/answers.json             # interview result / editable input
.ghost/config.toml              # schema version, profile, adapter list
.ghost/project.json             # detected stacks, manifests, Git facts
.ghost/state.json               # canonical workflow state
.ghost/generated-manifest.json  # SHA-256 for conflict detection
.ghost/tasks/TASK-0001.json     # structured task records
.ghost/events.jsonl             # append-only, redacted journal
.ghost/handoff.json             # last cross-agent snapshot
.ghost/evidence/<run-id>.json   # verification evidence
.ghost/verify.json              # optional command allowlist; user-owned
opencode.json                   # only when a model override was entered
```

`STATE.md` is a view over `.ghost/state.json`. Generated files are written through
a temporary file and atomic rename, guarded by `.ghost/.lock`. Before replacing a
generated file, ghost compares its SHA-256 with `.ghost/generated-manifest.json`.
If the file changed since generation, or was not generated by ghost, the command
stops and asks for `--force`. Symlinks are refused even with `--force`.

When upgrading a v1 project, `ghost regen` imports the existing `STATE.md` sections
into `.ghost/state.json` before replacing the view.

`ghost new` also appends these lines to `.gitignore` without overwriting existing
content and without duplicating them:

```text
.env
.env.*
.ghost/.lock
.ghost/dev.pid
.ghost/dev.log
```

## How it saves tokens and prevents lost work

- `AGENTS.md` / `CLAUDE.md` are capped at 40 lines and contain project facts, not a
  second copy of the global workflow.
- `STATE.md` is the handoff point. `/handoff` rewrites it and tells you it is safe to
  start a fresh session. `.ghost/state.json` is the canonical copy that `ghost status`
  prefers.
- Features are split into independent specs. `/build <slug>` reads only that spec.
  Codex, which has no project commands, gets direct `specs/feature-<slug>.md` next steps.
- `scripts/check.sh` prints `ok <stage>` or a failure summary, never a wall of logs.
- `scripts/dev.sh` records a PID and kills only that process group, so servers do not
  survive a forgotten session.
- `ghost status` also reports task counts and the current `in_progress` or
  `verification` task when one exists.
- `ghost handoff` / `ghost resume` replace guessing about the previous session:
  the snapshot names the revision it was taken at and resume flags a mismatch.
- Every task transition and checkpoint appends one redacted line to
  `.ghost/events.jsonl`, so history survives without storing prompts or logs.
- `ghost context` selects files deterministically from task references, Git
  changes, and manifests, then truncates to a token budget instead of dumping
  the repository.
- `ghost verify` only runs an approved command and stores a short output tail
  plus source hashes; `--list` marks evidence stale when those files change.
- `ghost checkpoint` refuses likely secrets before creating a local commit.
- The generated-file manifest prevents a re-render from silently discarding an edit
  you made to a spec, command, or script.

## `answers.json`

The file is pretty-printed JSON. Hackathon answers require every field except
`nice_to_haves`, `model_override`, `run_cmd`, and `stack_details`. Normal-profile
answers may also omit `deadline`, `demo_steps`, and `must_haves`. Example:

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
`ghost new` or a hackathon initialization. `created_at` is the progress start for
`ghost status`. Enumerations use snake_case (`hackathon`, `typescript_node`,
`per_feature`, etc.). A v1 answers file with a deadline string still parses.

## Relationship to global agent configuration

`ghost` writes project files only. It never reads or edits anything under
`~/.config/opencode/`, `~/.claude/`, or `~/.codex/` beyond checking whether the
directory exists and, for OpenCode, listing agent and skill directory names.

- Project and global `AGENTS.md` are both loaded by OpenCode; Claude Code loads
  `CLAUDE.md` with its own global memory.
- Commands are project commands and intentionally do not copy global rules.
- OpenCode command frontmatter selects an existing agent only when that agent is found
  in `~/.config/opencode/agents/`. Missing agents fall back to OpenCode's default.
- With an empty `model_override`, nothing generated sets a model. You type a model
  only if you want one; the recommended choice is to inherit the global model.
- `opencode.json` is written only when `model_override` is set, and contains only
  `$schema` and `model`. Claude Code and Codex receive no model configuration.

## Assumptions

Verified in the OpenCode docs (commands, rules, agents, config):

- Project commands live in `.opencode/commands/<name>.md`.
- Command frontmatter keys are `description`, `agent`, and `model`; the body is the
  prompt and `$ARGUMENTS` expands to the full argument string.
- Project and global `AGENTS.md` are both applied.
- Agents live in `~/.config/opencode/agents/<name>.md`, and `opencode.json` is a valid
  project config name.

Verified in the Claude Code docs:

- Project memory is `CLAUDE.md`.
- Project command files under `.claude/commands/<name>.md` remain supported after
  custom commands were merged into skills.
- Command frontmatter supports `description` and `argument-hint`, and `$ARGUMENTS`
  expands in the body.

Verified in the Codex docs:

- Project instructions are `AGENTS.md`.
- There is no project-scoped command file surface Ghost can rely on, so command
  generation is declared unavailable rather than emulated.

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
- `STATE.md` pre-fills `/plan` plus one `/build` per must-have when a command-capable
  adapter is selected. Codex instead gets `specs/feature-<slug>.md` next steps.
- Quality checks are best-effort: missing tools and missing npm scripts are skipped with
  a message rather than failing the run.
- A normal-profile project has no deadline, so `ghost status` prints a `no deadline`
  header instead of a countdown.

## Known limitations

- Unix-only. Windows is not supported.
- OpenCode and Claude Code get the full command surface. Codex gets instructions only.
- Adding or removing an adapter does not clean up the previous adapter's directory.
- Task records, the event journal, handoff snapshots, context packets, and
  verification evidence are local files under `.ghost/`; canonical workflow state
  stays in `.ghost/state.json`.
- No benchmark yet: token savings and context relevance are not measured.
- `ghost verify` has no sandbox; approval is a user promise.
- Context selection uses references, Git changes, and manifests, not embeddings.
- Evidence is unsigned and never pruned automatically.
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

