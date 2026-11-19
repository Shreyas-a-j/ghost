# Ghost 2.0 phase status

Source: `Blueprint.pdf`, 18 November 2026. Phase gates are advanced only when
their evidence is present.

## Phase 0: inventory and baseline

| Backlog item | Status | Evidence |
| --- | --- | --- |
| Inventory CLI, templates, state fields, hackathon assumptions | Done | `docs/INVENTORY-v1.md` |
| Golden-output fixtures for representative repositories | Done | `tests/phase_zero.rs` covers Rust, Python, Node |
| Unit tests for serialization, idempotence, paths, migration | Done | `answers`, `persist`, `state`, `project`, `render` unit tests |
| Schema v1 and upgrade path from `STATE.md` | Done | `docs/SCHEMA-v1.md`, `state::ensure_state`, migration test |
| Generated-file ownership and conflict behavior | Done | `docs/GENERATED-FILES.md` |
| ADR for core/adapter boundaries | Done | `docs/adr/0001-core-adapter-boundaries.md` |
| Formatting, Clippy, tests, reproducible CI | Done | `.github/workflows/ci.yml` |
| Compatibility and threat-model documents | Done | `docs/COMPATIBILITY.md`, `docs/THREAT-MODEL.md` |

Exit gate: existing commands and generated files are reproducible. **Passed.**

## Phase 1: generic profiles and repository detection

| Item | Status | Evidence |
| --- | --- | --- |
| `ghost init` without hackathon assumptions | Done | `cmds::init_project` |
| Rust/Python/Node detection | Done | `project::discover` |
| Versioned state schemas | Done | `.ghost/config.toml`, `project.json`, `state.json` |
| Atomic writes and locking | Done | `persist::atomic_write`, `FileLock` |
| Overwrite manifest and conflict consent | Done | `generated-manifest.json`, `persist::conflicts` |
| Existing repositories initialize without data loss | Done | `tests/phase_one.rs` |
| Keep hackathon as opt-in profile | Done | `--profile hackathon`, `ghost new` |

Exit gate: existing Rust/Python/JS repositories initialize without data loss.
**Passed by tests.**

## Phase 2: adapters

| Item | Status | Evidence |
| --- | --- | --- |
| Extract OpenCode behind an in-crate adapter boundary | Done | `src/adapter.rs`, `render::generate_for` |
| Add Claude Code renderer and capability declaration | Done | `CLAUDE.md`, `.claude/commands/*.md`, `tests/fixtures/phase_two/claude.json` |
| Add Codex renderer and capability declaration | Done | `AGENTS.md`, no project commands, `tests/fixtures/phase_two/codex.json` |
| Pinned compatibility fixtures for all three adapters | Done | `tests/phase_two.rs`, `tests/fixtures/phase_two/` |
| Adapter detection and honest capability reporting | Done | `ghost adapters [--json]` |

Exit gate: three adapters pass compatibility fixtures. **Passed by tests.**

Adapter selections are recorded as an array in `.ghost/config.toml` and reused by
`ghost regen`. Capabilities are declared per adapter; Ghost does not claim that a
surface exists on every agent. Codex has instructions but no project command
files, so its generated state uses file-based next steps instead of `/plan`.

## Phase 3: task engine and journal

Not started.

Planned:

- `tasks/<id>.json` with the documented transitions.
- `events.jsonl` with redacted metadata only.
- Handoff/resume snapshots with revision checks.
- Checkpoint metadata.

Exit gate: interrupted and cross-agent sessions recover consistently.

## Phase 4: context and verification

Not started.

Planned:

- Deterministic context packet selection with a token budget.
- Approved command execution and evidence capture.
- Stale-evidence detection when source or dependencies change.

Exit gate: benchmarks and stale-evidence tests pass.

## Phase 5: hardening and beta

Not started.

Planned:

- Fault-injection tests for interrupted writes and corrupt state.
- Signed/tagged release artifacts with checksums.
- Beta documentation, tester program, and measured claims.

Exit gate: documented beta on supported operating systems.
