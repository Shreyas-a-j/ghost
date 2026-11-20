# Ghost beta

Ghost is a local-first workflow layer for coding agents. This page states what
the beta supports, what it does not, and how to report a problem.

## Status

Ghost 2.0 is in source beta. The CLI, schemas, and generated surfaces are
covered by automated tests; there has been no external tester program and no
comparative benchmark yet. Treat claims about token savings as design intent,
not measured results.

## Supported environments

| Item | Status |
| --- | --- |
| Linux x86_64 | Built and tested in CI |
| macOS aarch64 | Release build in CI; not covered by the test suite |
| macOS x86_64 | Release build in CI; not covered by the test suite |
| Windows | Not supported; Ghost uses Unix process groups, signals, and permissions |
| Stable Rust | Required when building from source |

## Supported agents

| Agent | Surface |
| --- | --- |
| OpenCode | `AGENTS.md`, `.opencode/commands/*.md`, optional `opencode.json` model override |
| Claude Code | `CLAUDE.md`, `.claude/commands/*.md` |
| Codex | `AGENTS.md` only; no project command files |

Capabilities are declared per adapter and shown by `ghost adapters`. Ghost does
not claim feature parity across agents.

## Install

```bash
cargo install --path .
# or use a release tarball; each tarball has a .sha256 file
sha256sum -c ghost-x86_64-unknown-linux-gnu.tar.gz.sha256
```

## Try it

```bash
ghost init                     # detect an existing Rust, Python, or Node project
ghost task add "First task" --check "cargo test"
ghost task start TASK-0001
ghost context --task TASK-0001
ghost verify --task TASK-0001 --approve
ghost handoff --note "where I stopped"
ghost resume
```

`ghost new` remains the hackathon-oriented interview path.

## What is verified

- Unit and integration tests for schemas, migrations, adapters, task
  transitions, journal redaction, handoff staleness, evidence staleness,
  approval gating, and conflict handling.
- Fault-injection tests for corrupt state, corrupt task records, corrupt
  manifests, live and dead locks, and leftover temporary files.
- CI runs `cargo fmt --check`, Clippy with `-D warnings`, and the full test
  suite on Linux.

## Known limitations

- No sandbox. `ghost verify` approval is a user promise; an approved command
  runs with the user's shell access. Task-sourced acceptance checks always
  require an explicit `--approve`, even when a repository ships a matching
  allowlist.
- Evidence is unsigned and local. It is not a tamper-proof attestation.
- Context selection is heuristic. There is no embedding retrieval and no
  comparative benchmark.
- Redaction is pattern- and control-character-based. It is not secret scanning.
- Changing adapter selection does not delete the previous adapter directory.
- `regen` does not delete specs for removed features.
- Windows is unsupported.

## Reporting

Open an issue with:

- `ghost --version` and the operating system;
- the command and its exit code;
- the relevant `.ghost/` file names, with secrets removed;
- whether the project was created by `ghost new` or `ghost init`.

Do not paste `.env` contents or private keys into an issue.
