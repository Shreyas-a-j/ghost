# Threat model

Ghost is local-first today: no network calls, no LLM calls, no cloud upload. The
threat model therefore focuses on local file integrity, command execution, and
accidental secret exposure.

## Assets

- User source code and uncommitted changes.
- `.ghost/answers.json` and canonical workflow state.
- Generated instructions and commands consumed by an agent.
- Local Git history.
- Secrets present in the working tree or environment.

## Trust boundaries

| Boundary | Trust |
| --- | --- |
| Ghost binary | Trusted |
| Project directory contents | Untrusted input |
| `answers.json` | User-controlled input; validated |
| Generated Markdown | Passed to a third-party agent; treat as untrusted by that agent |
| `scripts/check.sh` / `scripts/dev.sh` | User-reviewable shell; executed only by explicit `ghost check` or `ghost down` |
| Agent CLI discovery under `~/.config/opencode` | Read-only names; no file contents |
| Git | Invoked locally; no push |

## Threats and mitigations

### Path traversal or symlink escape

A generated path could contain `..` or a directory component could be a symlink
to a location outside the project.

Mitigations:

- Reject absolute paths and any non-`Normal` path component.
- Walk existing components with `symlink_metadata` and refuse symlinks.
- `--force` does not bypass the symlink check.
- Paths are generated internally; user input never becomes a write path.

### Silent overwrite of user work

A re-render could destroy a hand-edited `AGENTS.md` or `STATE.md`.

Mitigations:

- Record SHA-256 hashes in `.ghost/generated-manifest.json`.
- Refuse changed files and files not created by Ghost unless `--force`.
- `new` keeps its v1 refusal of any existing generated target.
- Canonical `state.json` is never replaced by `new`, `init`, or `regen`.
- Migration imports `STATE.md` before the view is overwritten.

### Concurrent writers

Two Ghost processes could interleave writes.

Mitigations:

- `.ghost/.lock` uses `create_new` for exclusivity.
- A lock owned by a dead process is reclaimed; a live owner produces an error.
- Writes use temporary files plus atomic rename.

### Command execution

`ghost check`, `ghost verify`, and `ghost down` execute project commands.

Mitigations:

- Never executed automatically by `new`, `init`, `regen`, or `status`.
- `check` runs synchronously with a timeout and kills the process group.
- `down` stops only the PID recorded by `dev.sh`.
- `checkpoint` refuses common secret file names and never pushes.
- `check` and `down` compare the script against its SHA-256 in
  `.ghost/generated-manifest.json` and refuse to run a changed script.
  `GHOST_ALLOW_MODIFIED_SCRIPTS=1` is an explicit opt-out for deliberate local
  edits, and `ghost regen` rewrites the script and its hash.
- Acceptance checks referenced by a task are always treated as untrusted
  repository data: they require an explicit `--approve`, even if the same
  repository ships a matching `.ghost/verify.json` entry.

Residual risk: the generated script content is only as safe as the user's review.
Ghost does not sandbox scripts. An approved command runs with the user's shell
access.

### Secret exposure

Generated context could include secrets; checkpoint could commit them.

Mitigations:

- `.env`, `.env.*`, `.ghost/dev.pid`, and `.ghost/dev.log` are ignored.
- `checkpoint` enumerates every real path with
  `git status --porcelain=v1 -z --untracked-files=all --no-renames`, so a
  collapsed untracked directory cannot hide a secret file.
- `checkpoint` and `context` share a name filter covering `.env*`, private keys
  (`*.pem`, `*.key`, `*.p12`, `.ssh/`, `.aws/`, `id_rsa*`, `id_ed25519*`), and
  common credential files (`.netrc`, `.npmrc`, `.pypirc`, `.pgpass`).
- `context` excludes `.ghost/`, `.git/`, `target/`, and `node_modules/` even when
  a task references them explicitly.
- Journal and evidence metadata mask common credential keys before storage.
- Loaded state, task, handoff, and evidence text is stripped of terminal control
  characters before printing.
- Ghost reads skill and agent *directory names* only, never their contents.
- No network or model calls, so repository content is not uploaded.

Residual risk: the name-based filter is incomplete. A secret with an arbitrary
filename can still be committed or included in a context packet. Review a
packet before pasting it into a remote model.

### Journal and handoff

`ghost task`, `ghost handoff`, and `ghost resume` read and write local state only.

Mitigations:

- Task ids are validated as `TASK-` plus digits; arbitrary paths cannot become task files.
- Journal fields are redacted and capped before append.
- Handoff notes pass through the same redaction.
- `resume` never writes canonical state; it reports staleness and exits non-zero.
- All journal and task writes happen under the existing `.ghost/.lock`.

Residual risk: redaction is heuristic. A secret typed into a task title or note
can still be stored; keep prompts and credentials out of both.

### Verification execution and evidence

`ghost verify` runs project commands and stores a short output tail.

Mitigations:

- A command runs only with `--approve` or an exact match in `.ghost/verify.json`.
- Acceptance checks taken from a task file always require `--approve`; a
  committed allowlist cannot approve them.
- Commands run in their own process group with a timeout; the group is killed.
- Only the last 40 lines are stored, each redacted and capped at 200 characters.
- Evidence records a redaction policy, revision, and source hashes, not full logs.
- Raw output is written to a private (`0600`) log created with `create_new` and a
  randomized name, so a pre-planted symlink cannot redirect or truncate another
  file. The log is deleted after use.
- Every canonical document checks `schema_version` before use.
- Ghost never pushes, deploys, or uploads anything.

Residual risk: the allowlist is a promise by the user, not a sandbox. An
approved command has the same access as the user's shell. Evidence is local and
unsigned.

### Context packet disclosure

`ghost context` prints file excerpts to stdout.

Mitigations:

- Only relative paths inside the project are read; symlinks, binaries, and files
  over 256 KiB are skipped.
- `.ghost/`, `target/`, and `node_modules/` are excluded from Git-derived picks.
- The packet is bounded by `--budget`, so a large file cannot flood the session.
- Exclusion is name- and size-based, not secret detection. Review before pasting
  a packet into a remote model.

### Corrupt or incompatible state

A partial write or future schema could break a project.

Mitigations:

- Atomic writes prevent partial targets.
- JSON parsing failures include the file path and stop the command.
- `schema_version` is present in every canonical document.
- Unknown future versions are treated as errors, not guessed.
- Migration policy requires a backup before rewriting documents.

Residual risk: no automatic backup directory exists yet; that lands with the
first real schema migration.

### Supply chain

Dependencies could introduce vulnerabilities.

Mitigations:

- Small dependency set: `anyhow`, `chrono`, `clap`, `console`, `dialoguer`,
  `minijinja`, `serde`, `serde_json`; `tempfile` for tests.
- SHA-256 is implemented in-tree to avoid a hashing dependency.
- `Cargo.lock` is committed and installs use `--locked`.
- Release artifacts are built from tags by CI.
- CI runs `cargo audit` against the committed lockfile.

Gaps: no signed artifacts yet. Release packages ship SHA-256 checksum files.
`cargo audit` reports one warning: `chrono 0.4.45` is marked unmaintained
(RUSTSEC-2026-0543) with no known vulnerability and no fixed upgrade. It is
tracked as a migration item, not treated as a release blocker.

### Agent adapter drift

OpenCode, Claude Code, or Codex may change command or instruction conventions.

Mitigations:

- Compatibility assumptions are documented in `docs/COMPATIBILITY.md`.
- Pinned fixtures in `tests/fixtures/phase_two/` assert each adapter's files,
  frontmatter, and forbidden surfaces.
- Capability declarations prevent claiming parity where an agent lacks a feature.

## Out of scope for the first beta

- Autonomous agent orchestration.
- Cloud sync, accounts, billing, and telemetry.
- Sandboxing project scripts.
- Guaranteeing that an agent consumes generated context.
- Windows support.
