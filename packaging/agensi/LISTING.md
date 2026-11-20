# Agensi submission copy

Fill the bracketed values before submitting. The package itself is
`ghost.zip`; Agensi expects `SKILL.md` at the zip root, which this build
provides.

## Listing

- **Name:** Ghost
- **Skill id:** `ghost`
- **Category:** Developer tools / Coding agents
- **Price:** Free (non-exclusive listing)
- **Repository:** https://github.com/Shreyas-a-j/ghost
- **Support:** [your support email]
- **Author:** [your name or handle]
- **Platforms:** Linux x86_64, macOS arm64, macOS x86_64

## Short description

> A local-first task, context, and verification workflow for coding agents.
> Ghost keeps one source of truth in `.ghost/`, builds deterministic context
> packets, records approved verification evidence, and hands off cleanly between
> sessions.

## Long description

Ghost gives a coding agent a small, durable workflow inside a repository.

Most agent failures are not model failures. They are workflow failures: the
agent loses the thread between sessions, reads too much of the repository, claims
a task is finished without evidence, or rewrites state it does not own.

Ghost fixes the workflow, not the model:

- **Canonical state.** `.ghost/state.json` holds focus, next, done, decisions,
  and blockers. `STATE.md` is a generated view.
- **Tasks with acceptance checks.** `ghost task add` creates `TASK-####` records
  with references, checks, and a base revision. Transitions are enforced.
- **Deterministic context packets.** `ghost context --task TASK-0001 --budget 1200`
  selects task metadata, acceptance checks, referenced files, recent evidence, and
  state. Explicit references come first. Output is stable for the same repo state.
- **Approval-gated verification.** `ghost verify` runs a command only with
  `--approve` or an exact allowlist match, saves bounded evidence with source
  hashes, and marks results stale when a dependency changes.
- **Handoffs that do not lie.** `ghost handoff` writes a snapshot; `ghost resume`
  exits `2` when the revision moved. It never merges.
- **Local-first.** No network calls, no model calls, no push, no cloud.

Ghost supports existing Rust, Python, and Node repositories through `ghost init`,
and adapters for OpenCode, Claude Code, and Codex.

### What is not claimed

There is no benchmark yet: token savings and context relevance are not measured.
Verification is not sandboxed. Evidence is local and unsigned. Windows is not
supported. The beta documentation states these limits explicitly.

## Suggested tags

coding-agent, workflow, cli, rust, context-engineering, tasks, verification,
local-first, opencode, claude-code, codex

## Requirements

- Linux x86_64 or macOS (arm64/x86_64)
- `git`
- `bash` and `setsid` (Linux) for command execution
- A Rust toolchain only when building from source
