# ADR 0002: Task records, journal, and handoff snapshots

Status: accepted (Phase 3)
Date: 2026-11-19

## Context

Phase 1 made canonical workflow state durable, but work itself still lived in
prose. `STATE.md` could say "build the board" without an id, an owner, a base
revision, or a history. Two agents could not tell whether they were continuing
the same piece of work, and a fresh session had no reliable way to know whether
the last summary still matched the repository.

The blueprint requires a small task state machine, an append-only journal, a
handoff snapshot with revision checks, and checkpoint metadata, all local-first
and metadata-only.

## Decision

### One file per task

Tasks live in `.ghost/tasks/TASK-0001.json` rather than a single `tasks.json`.
Each record is written atomically under `.ghost/.lock`.

- A single file means a corrupt task cannot take down the whole list.
- Ids are allocated from the highest existing number and never reused.
- `status` is the only field whose transition is gated; everything else is
  descriptive. The table is in `docs/SCHEMA-v1.md` and in
  `TaskStatus::can_transition_to`.

`base_revision` is captured from Git HEAD when a task first enters
`in_progress`. It is informational: Ghost reports drift, it does not rebase or
block on it.

### Append-only metadata journal

`.ghost/events.jsonl` receives one JSON object per line for task creation,
transitions, handoff, resume, and checkpoint. The journal is written with an
append handle and an `fsync`, never rewritten.

Fields pass through `events::redact`, which strips control characters,
collapses whitespace, and caps length. The journal is deliberately unable to
hold raw prompts, command output, or file contents. Readers skip malformed
lines so one bad line does not make `ghost resume` unusable.

### Handoff is a snapshot, not a merge

`ghost handoff` writes `.ghost/handoff.json` with revision, branch, dirty flag,
workflow state, task summaries, evidence ids, and an optional redacted note.
`ghost resume` compares the snapshot revision with the current HEAD and exits
`2` on a mismatch.

Ghost never rewrites canonical state during resume. A stale snapshot is a
signal for the agent to re-read changed files, not an invitation for Ghost to
guess which state is newer. This keeps conflict resolution with the agent and
the user.

### Checkpoint metadata precedes the commit

`checkpoint` appends its event before `git add -A`, so the resulting commit
contains the journal entry that describes it. The event records the parent
revision and the redacted message. A failure to write metadata warns but does
not abort an otherwise valid commit, because losing a commit is worse than
losing one journal line.

### Status stays a view

`ghost status` gains one task summary line derived from the task files. It does
not become a scheduler, and it never mutates tasks. `STATE.md` remains the
human view.

## Consequences

- Work has stable ids that survive across sessions and agents.
- History is queryable without storing sensitive text.
- Staleness is explicit and machine-detectable through the exit code.
- The journal is not a full audit log: it intentionally omits command output
  and diffs. That belongs to Phase 4 evidence files.
- Task files are not rendered by `new`/`init`/`regen`; a re-render cannot
  overwrite a task.
- A user can still store a secret in a title or note. Redaction reduces
  accidental leakage, it does not classify secrets.

## Alternatives considered

- **Single `tasks.json`**: simpler, but one corrupt write or merge conflict
  affects every task, and concurrent agents would contend on the whole list.
- **Tasks embedded in `state.json`**: fewer files, but mixes a human-editable
  view with machine state and makes per-task atomic updates impossible.
- **Auto-merging on resume**: rejected. Without a three-way merge of intent,
  Ghost would silently pick a winner.
- **Recording full command output in events**: rejected for size and secret
  exposure. Evidence files in Phase 4 are the right place, with explicit
  redaction and retention rules.
