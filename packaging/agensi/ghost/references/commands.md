# Command reference

All commands run in the current project root and use `.ghost/` for canonical
state.

## Setup

| Command | Purpose |
| --- | --- |
| `ghost init [--force] [--adapter NAME]...` | Adopt an existing Rust, Python, or Node repository without hackathon assumptions. Idempotent when content matches. |
| `ghost regen [--force] [--adapter NAME]...` | Re-render generated files from `.ghost/answers.json`. |
| `ghost new` | Interactive hackathon interview. Use only when the user asks for it. |
| `ghost adapters [--json]` | Show supported agents, detection, and capabilities. |
| `ghost status` | Compact dashboard: deadline, focus, next step, blockers, dev server, task counts. |

## Tasks

| Command | Purpose |
| --- | --- |
| `ghost task add TITLE [--ref PATH]... [--check CMD]... [--owner NAME]` | Create `TASK-####`. |
| `ghost task start ID` | Move a task to `in_progress` and record the base revision. |
| `ghost task move ID STATUS` | Move through the allowed state machine (`backlog`, `in_progress`, `blocked`, `done`, `cancelled`). |
| `ghost task list [--json]` | List task records. |
| `ghost task show ID [--json]` | Show one task. |

## Context

| Command | Purpose |
| --- | --- |
| `ghost context [--task ID] [--budget N] [--json]` | Deterministic packet with task, acceptance checks, relevant excerpts, evidence, and state. Default budget is 1200 tokens. |

## Verification

| Command | Purpose |
| --- | --- |
| `ghost verify -- <command>` | Run an approved command and save evidence. |
| `ghost verify --task ID --approve` | Run a task's acceptance checks after explicit approval. |
| `ghost verify --list` | List evidence, exit codes, task ids, and `[stale]` markers. |
| `ghost verify --timeout N ...` | Per-command timeout in seconds (default 300). |

A command is allowed only with `--approve` or an exact match in
`.ghost/verify.json`. Task-sourced checks always require `--approve`.

## Session continuity

| Command | Purpose |
| --- | --- |
| `ghost handoff [--note TEXT]` | Write `.ghost/handoff.json` with state, tasks, evidence ids, and revision. |
| `ghost resume` | Print the last snapshot; exit `2` when the revision is stale. Never merges. |

## Local checks and Git

| Command | Purpose |
| --- | --- |
| `ghost check [--timeout N]` | Run `scripts/check.sh`. Refuses a script whose hash no longer matches `.ghost/generated-manifest.json`. |
| `ghost down` | Run `scripts/dev.sh stop`. |
| `ghost checkpoint [MESSAGE]` | Stage and commit locally after refusing common credential file names. Never pushes. |

If the user deliberately edits a generated script, set
`GHOST_ALLOW_MODIFIED_SCRIPTS=1` for that command or run `ghost regen`.

## Files

| Path | Meaning |
| --- | --- |
| `.ghost/config.toml` | Project metadata and selected adapters. |
| `.ghost/project.json` | Detected stack and commands. |
| `.ghost/state.json` | Canonical workflow state. |
| `.ghost/tasks/TASK-####.json` | One task per file. |
| `.ghost/events.jsonl` | Append-only, redacted metadata journal. |
| `.ghost/handoff.json` | Last handoff snapshot. |
| `.ghost/evidence/RUN-*.json` | Verification evidence. |
| `.ghost/generated-manifest.json` | SHA-256 hashes of generated files. |
| `.ghost/verify.json` | Optional user-owned command allowlist. Ghost never writes it. |
