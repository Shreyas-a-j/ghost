# ADR 0003: Context packets and verification evidence

Status: accepted (Phase 4)
Date: 2026-11-19

## Context

Phase 3 gave tasks identity and history, but a fresh session still had to
decide which files to read. Reading everything is expensive and noisy; reading
too little misses the code a task actually touches. Verification had the same
problem in reverse: `ghost check` proved a moment passed, but nothing recorded
what ran, at which revision, or whether the result still applied.

The blueprint asks for deterministic context selection before embeddings, a
token budget with space left for reasoning, approved command execution, and
stale-evidence detection.

## Decision

### Deterministic selection, no retrieval model

`ghost context` ranks sources with simple, explainable rules:

1. The selected task's objective, status, owner, base revision, and references.
2. Acceptance criteria.
3. Blockers and decisions from canonical state.
4. `git diff --stat HEAD` as a metadata-only summary of change.
5. Recent evidence for the task, with stale flags.
6. File excerpts from task references, then Git-changed files, then manifests.

Candidates are sorted and deduplicated, so the same repository state produces
the same packet. No embeddings, no network, no model call. If these heuristics
later prove insufficient, the omission list in the packet is the measurement
hook for deciding whether to add retrieval.

### Budget with explicit uncertainty

The estimate is `characters / 4`, labeled as conservative and uncertain by
roughly 15 percent. Fixed sections are included first. Optional sections are
added only while they fit. File excerpts are truncated at a line boundary to
the remaining budget, and omitted files are named.

This is intentionally not a tokenizer-accurate count. A tokenizer dependency
would add weight and still vary by model. The packet reports what it did so the
receiving agent can budget the remainder for reasoning.

### Approval before execution

`ghost verify` runs a command only when one of these is true:

- `--approve` was passed for this run, or
- the command exactly matches an entry in `.ghost/verify.json`.

The allowlist is a user-owned file Ghost reads but never writes. Exact matching
keeps the rule auditable; prefix or glob matching would make an approval broader
than what the user wrote.

`verify --task <id>` with no command runs that task's acceptance checks, but each
check still needs the same approval. Task files are repository content, so they
are not treated as consent by themselves.

### Evidence is a bounded record, not a log

Each run writes one immutable `.ghost/evidence/<run-id>.json`:

- command, working directory, start/end time, duration;
- exit code and timeout flag;
- repository revision and dirty flag;
- redaction policy;
- SHA-256 hashes of relevant sources;
- the last 40 redacted output lines.

Raw output goes to a temporary file under `.ghost/`, which is deleted after the
tail is copied. Evidence ids are appended to the task, and a `verification`
event is added to the journal.

Staleness is hash-based, not revision-based: a run is stale when a recorded file
is missing or changed. A new commit that does not touch the recorded files does
not invalidate the evidence, which matches the blueprint's "relevant source
files or dependencies" wording. `verify --list` reports the flag; stale records
are never deleted automatically.

### Evidence failure does not silently pass

If the command fails or times out, evidence is still written and the exit code
is propagated (`124` for timeouts). The journal records `exit_code` and an
optional `timed_out` flag. This keeps failed runs visible instead of erasing
them.

## Consequences

- Agents get one deterministic, budgeted read instead of a repository dump.
- Every verification is attributable to a command, a revision, and a set of
  source hashes.
- Stale checks are visible without rerunning anything.
- The allowlist is a convenience, not a sandbox; approved commands have the
  user's full shell access.
- Output tails are redacted but not secret-scanned; users should avoid printing
  secrets in checks.
- Evidence grows without automatic retention. Pruning is a manual or future
  policy decision, deliberately not hidden in Phase 4.

## Alternatives considered

- **Embedding retrieval**: rejected for Phase 4. It adds a model, network, and
  nondeterminism before simple heuristics have been measured.
- **One combined `context.json` with full file contents**: rejected; the packet
  is a prompt, not a cache, and the budget must be visible in the output.
- **Implicit approval from `answers.json` or task checks**: rejected. Repository
  content is not user consent for arbitrary command execution.
- **Storing full stdout/stderr**: rejected for size and secret exposure. The
  bounded, redacted tail plus source hashes is enough to judge freshness.
- **Revision-only staleness**: rejected. Unrelated commits would invalidate
  every run and train users to ignore the flag.
