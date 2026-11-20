# ADR 0001: Keep core and adapter boundaries inside one crate

- Status: accepted
- Date: 2026-11-19
- Phase: 0/1; Phase 2 follow-up applied

## Context

Ghost 0.1.0 is a single Rust binary that renders an OpenCode workflow. Ghost 2.0
must support OpenCode first, then Claude Code and Codex, without pretending the
three agents have identical capabilities.

The blueprint proposes `ghost-cli`, `ghost-core`, `ghost-project`, `ghost-state`,
`ghost-workflow`, `ghost-context`, `ghost-verify`, and `ghost-adapters` modules,
but also says to split into a Cargo workspace only after public interfaces are
stable. Splitting now would add crate-boundary ceremony before the types have
been exercised by real adapters.

## Decision

Keep one crate with explicit module boundaries:

| Module | Responsibility |
| --- | --- |
| `main` | Argument parsing and command dispatch |
| `cmds` | Command orchestration, lock lifetime, output |
| `answers` | Input model and validation |
| `interview` | Interactive prompts |
| `project` | Repository discovery and facts |
| `persist` | Atomic writes, locking, path safety, manifest, hashing |
| `state` | Versioned canonical metadata and workflow state |
| `adapter` | Adapter trait, implementations, capabilities, detection |
| `render` | Template environment, shared views, command bodies |
| `opencode` | OpenCode detection and command conventions |
| `status` | Dashboard rendering |

The rule that matters is directional: **canonical types never depend on an
adapter**. `state`, `project`, `persist`, and `answers` do not import `adapter`,
`render`, or `opencode`. Adapters consume canonical facts and emit
`GeneratedFile` values; shared views are rendered once for any adapter set.

## Consequences

Positive:

- Existing tests and CLI aliases keep working during the migration.
- Interfaces can change without semver or workspace churn.
- A future `ghost-adapters` crate has a clear extraction seam: `GeneratedFile`
  plus a project snapshot.

Negative:

- Module discipline is enforced by review, not the compiler.
- Command bodies are shared templates, so adapter-specific variation is limited
  to frontmatter and destination paths.
- Binary size is monolithic until the workspace split.

## Follow-up

Phase 2 landed `Adapter` in `src/adapter.rs` with `id`, `capabilities`,
`detect`, `instruction_file`, `commands_dir`, `command_header`, and
`config_files`. `OpenCodeAdapter`, `ClaudeAdapter`, and `CodexAdapter` implement
it; `render::generate_for` depends only on the trait. The crate split waits until
the task engine and context packet interfaces stabilize, so that `ghost-core`,
`ghost-state`, and `ghost-adapters` can be separated once rather than twice.
