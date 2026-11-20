# Agensi submission copy

Fill the bracketed values before submitting. The package itself is
`ghost.zip`; Agensi expects `SKILL.md` at the zip root, which this build
provides.

## Listing

- **Name:** Ghost 2.0
- **Skill id:** `ghost`
- **Category:** Developer tools / Coding agents
- **Price:** $5 USD (one-time purchase; non-exclusive listing)
- **Repository:** https://github.com/Shreyas-a-j/ghost
- **Support:** [your support email]
- **Author:** [your name or handle]
- **Platforms:** Linux x86_64, macOS arm64, macOS x86_64

## Short description

> Ghost 2.0 is a local-first workflow engine for OpenCode, Claude Code, and Codex. Keep project state, tasks, context packets, verification evidence, and handoffs organized across coding sessions.

## Long description

Ghost 2.0 gives supported AI coding agents a shared, durable project workflow inside an existing repository. The core Rust CLI remains free and MIT-licensed on GitHub; this $5 listing provides the packaged agent skill, setup guidance, and included reference templates. Buying the package does not grant exclusive access to the CLI.

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
supported. The beta documentation states these limits explicitly. An externally signed distribution ZIP does not make internal verification evidence cryptographically signed.

## Purchase and delivery

- **Price:** $5 USD, one-time (subject to Agensi listing options).
- **Deliverable:** Agent skill ZIP with `SKILL.md` at its root, installer instructions, scripts, references, and templates.
- **Core CLI:** Free, open source (MIT), installed separately from the GitHub release or crates.io.
- **Security:** Package signatures/checksums should be published and verifiable independently; signatures do not imply Agensi verifies them.

## Suggested tags

coding-agent, workflow, cli, rust, context-engineering, tasks, verification,
local-first, opencode, claude-code, codex

## Requirements

- Linux x86_64 or macOS (arm64/x86_64)
- `git`
- `bash` and `setsid` (Linux) for command execution
- A Rust toolchain only when building from source
