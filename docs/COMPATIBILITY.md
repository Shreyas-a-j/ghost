# Compatibility

This document records what Ghost actually supports today and what it only plans
to support. It is not a promise of feature parity across agents.

## Runtime matrix

| Item | Supported | Evidence |
| --- | --- | --- |
| Linux (Ubuntu) | Yes | CI target and local test suite |
| macOS | Build target only | Release workflow builds `x86_64` and `aarch64`; not exercised in CI |
| Windows | No | Uses `setsid`, Unix signals, and Unix permissions |
| Stable Rust | Yes | `cargo test --locked` |
| Rust 2021 edition | Yes | `Cargo.toml` |

## Agent adapters

| Agent | Status | Surface |
| --- | --- | --- |
| OpenCode | Supported | `AGENTS.md`, `.opencode/commands/*.md`, `opencode.json` model override, agent discovery under `~/.config/opencode/agents/` |
| Claude Code | Supported | `CLAUDE.md`, `.claude/commands/*.md` with `description` and `argument-hint` frontmatter |
| Codex | Supported (instructions only) | `AGENTS.md`; no project command files are claimed |

Capabilities are explicit in `src/adapter.rs` and visible with `ghost adapters`.
No adapter claims hooks, skill files that Ghost writes, or automated execution.

OpenCode assumptions verified against its documentation:

- Project commands live in `.opencode/commands/<name>.md`.
- Frontmatter keys are `description`, `agent`, and `model`.
- `$ARGUMENTS` expands to the full argument string in command bodies.
- Project and global `AGENTS.md` are both applied.
- Agent files live in `~/.config/opencode/agents/<name>.md`.
- `opencode.json` is a valid project config name.

Conventional names (`build`, `tester`, `reviewer`) are used only when the agent
file exists. Otherwise the `agent:` line is omitted and OpenCode's default agent
is used.

Claude Code assumptions:

- Project memory is `CLAUDE.md`; project command files remain supported under
  `.claude/commands/<name>.md` even after custom commands were merged into skills.
- Command frontmatter supports `description` and `argument-hint`.
- `$ARGUMENTS` expands in command bodies.
- Ghost writes no Claude settings, hooks, or model configuration.

Codex assumptions:

- Project instructions are `AGENTS.md`.
- Codex does not expose an equivalent project-scoped command file surface that
  Ghost can rely on, so commands are not generated and the capability is declared
  as unavailable. Generated state uses file-based next steps instead.

## CLI compatibility

Ghost 0.1.0 commands keep working:

- `ghost new`, `regen`, `status`, `check`, `down`, `checkpoint`.

Additions:

- `ghost init` for normal repositories.
- `ghost regen --force` for conscious overwrite of changed generated files.
- `--adapter <opencode|claude|codex>` on `new`, `init`, and `regen`; repeatable.
- `ghost adapters [--json]` for detection and capability reporting.

No v1 command has been removed. `ghost new` remains the hackathon-oriented
interview path; `ghost init` is the generic path. With no `--adapter` flag,
Ghost reuses the saved selection or defaults to OpenCode.

## Input compatibility

v1 `answers.json` files remain valid. A deadline string is parsed into the new
optional deadline field. Normal profiles may omit a deadline, demo steps, and
must-haves; hackathon profiles keep the v1 requirements.

An older `config.toml` with a single `adapter = "..."` line is not read; Ghost
falls back to OpenCode and rewrites the file as an `adapters` array on the next
generation.

## Generated-file compatibility

- v1 projects without a manifest are adopted when generated files still match
  the current template output.
- Changed files produce a conflict instead of a silent overwrite.
- `STATE.md` is imported into `.ghost/state.json` before a view is replaced.
- `.gitignore` entries remain idempotent.
- Adding an adapter writes that adapter's files; removing one does not delete the
  old directory.

## Known limitations

- Codex receives instructions and shared views but no project command files.
- Claude Code receives no model or settings configuration.
- No task state machine, event journal, context packet, or verification evidence
  yet; those are Phases 3 and 4.
- No model validation; a typed `provider/model` is passed through to OpenCode.
- `regen` does not delete specs for removed features or directories for removed
  adapters.
- `ghost checkpoint` assumes Git is installed and the project is a repository.
- Platform behavior for screen readers, terminal color, and non-UTF-8 paths is
  not validated.
