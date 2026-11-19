# Changelog

## Unreleased

Ghost 2.0 implementation, developed in phases:

- Generic `ghost init` with Rust, Python, and Node detection.
- Versioned canonical state (`.ghost/config.toml`, `project.json`, `state.json`)
  with migration from `STATE.md`.
- Atomic writes, single-writer lock, generated-file manifest, and conflict
  consent.
- Adapter trait with OpenCode, Claude Code, and Codex surfaces plus
  `ghost adapters`.
- Task records and state machine (`ghost task`).
- Append-only, redacted journal (`.ghost/events.jsonl`).
- Handoff snapshots and revision-aware `ghost resume`.
- Checkpoint metadata recorded with the local commit.
- Budgeted, deterministic context packets (`ghost context`).
- Approved verification with evidence and stale detection (`ghost verify`).
- CI, release packaging with SHA-256 checksums, beta documentation, and
  fault-injection tests.

## 0.1.0

- Initial v1: hackathon interview, generated OpenCode workflow, `status`,
  `check`, `down`, and `checkpoint`.
