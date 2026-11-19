# v1 inventory

This is the Phase 0 inventory of the Ghost 0.1.0 surface. It records what exists
before adapters and the task engine, so later changes have a compatibility
baseline.

## CLI commands

| Command | Flags | Behavior | Exit |
| --- | --- | --- | --- |
| `ghost new` | `--dir <path>` (default `.`), `--force`, `--answers <file>` | Interviews the user or loads answers, validates a future deadline, renders the OpenCode workflow, appends `.gitignore` entries. Refuses any existing generated target unless `--force`. | `0` success, `1` error |
| `ghost init` | `--dir <path>`, `--force`, `--profile normal\|hackathon`, `--name <name>`, `--answers <file>` | Detects an existing repository and initializes it without hackathon fields. Reuses stored answers. Refuses changed or unowned files unless `--force`. | `0` success, `1` error |
| `ghost regen` | `--force` | Re-renders generated views from `.ghost/answers.json`, refreshes canonical metadata, and appends `.gitignore` entries. Refuses edited generated files unless `--force`. | `0` success, `1` error |
| `ghost status` | none | Prints a compact dashboard from `.ghost/state.json` or, for v1 projects, `STATE.md`. | `0` success, `1` missing answers |
| `ghost check` | `--timeout <seconds>` (default `300`) | Runs `scripts/check.sh` in its own process group; kills the group on timeout. | Child exit code, `124` on timeout |
| `ghost down` | none | Runs `scripts/dev.sh stop`, which stops only the recorded process group. | Child exit code |
| `ghost checkpoint` | `[message...]` | Refuses `.env*`, `*.pem`, `*.key`, `id_rsa*`; otherwise creates a local commit. Never pushes. | `0` success, `1` refused/error |

Every command except `new` and `init` exits `1` with `run \`ghost new\` first` when
`.ghost/answers.json` is missing.

## Generated files and templates

| Output | Template | Owner | Notes |
| --- | --- | --- | --- |
| `AGENTS.md` | `templates/AGENTS.md.j2` | generated view | Capped at 40 lines; project facts only |
| `STATE.md` | `templates/STATE.md.j2` | generated view | Public handoff view with section markers |
| `specs/00-overview.md` | `templates/overview.md.j2` | generated view | Profile-aware demo/primary-flow heading |
| `specs/feature-<slug>.md` | `templates/feature.md.j2` | generated view + user edits | One per must-have |
| `specs/schema.yaml` | `templates/schema.yaml.j2` | user-owned scaffold | Flat notes, TODO placeholders |
| `scripts/check.sh` | `templates/check.sh.j2` | generated view | Stack-specific stages; short failure output |
| `scripts/dev.sh` | `templates/dev.sh.j2` | generated view | `start`, `stop`, `status`; PID-tracked |
| `.opencode/commands/*.md` | `templates/commands/*.j2` | generated view | `plan`, `build`, `fix`, `test`, `review`, `handoff`, `checkpoint` |
| `opencode.json` | inline JSON | generated view | Only when `model_override` is set |
| `.ghost/answers.json` | serde JSON | user-editable source | Interview result |
| `.ghost/config.toml` | inline | generated metadata | Profile and adapter selection |
| `.ghost/project.json` | serde JSON | generated metadata | Detected stacks, manifests, Git facts |
| `.ghost/state.json` | serde JSON | canonical workflow state | `STATE.md` is its view |
| `.ghost/generated-manifest.json` | serde JSON | generated metadata | SHA-256 per generated view |

`.gitignore` receives `.env`, `.env.*`, `.ghost/dev.pid`, `.ghost/dev.log` once
each. Existing content is preserved.

## `answers.json` fields

Required for v1: `project_name`, `pitch`, `demo_steps`, `event_name`, `deadline`,
`team_size`, `mode`, `stack`, `must_haves`, `existing_code`, `risky_areas`,
`test_level`, `skills`, `checkpoint_freq`, `created_at`.

Optional: `nice_to_haves`, `model_override`, `run_cmd`, `stack_details`, and now
`deadline` for normal profiles.

Enumerations: `mode` (`hackathon`, `normal`), `team_size` (`solo`, `small`,
`large`), `stack` (`rust`, `python`, `typescript_node`, `other`), `test_level`
(`none`, `smoke`, `per_feature`), `checkpoint_freq` (`before_every`,
`before_risky`, `manual`), `risky_areas` (`auth`, `payments`, `user_data`,
`external_apis`, `file_uploads`, `none`).

`deadline` uses local time `YYYY-MM-DD HH:MM`; `created_at` is an RFC 3339
timestamp.

## `STATE.md` markers and semantics

Markers split the file into machine-readable sections:

```text
<!-- ghost:focus -->
<!-- ghost:next -->
<!-- ghost:done -->
<!-- ghost:decisions -->
<!-- ghost:blockers -->
```

Guidance encoded in the template: focus 1-3 lines, next at most 5 checkboxes,
done the last 10 items, decisions 10 one-line entries, blockers explicit. The
`/handoff` command rewrites the view for a fresh session.

## Hackathon-specific assumptions in v1

These are preserved for the `hackathon` profile and must not leak into normal
initialization:

- The interview asks for event name, deadline, three demo steps, and 1-6
  must-haves.
- `validate_structure` required exactly three demo steps and at least one
  must-have.
- Every answer set required a future deadline.
- `status` always rendered a countdown and event name.
- `specs/00-overview.md` always used a "Demo script" heading.
- Feature specs described each feature by its demo step.
- The default interview mode was `hackathon`.
- README positioning and examples assumed a hackathon.

## Environment overrides

| Variable | Purpose |
| --- | --- |
| `GHOST_OPENCODE_DIR` | Replace `~/.config/opencode` for agent/skill discovery |
| `GHOST_AGENTS_DIR` | Replace `~/.agents/skills` for skill discovery |

## Baseline evidence

- `tests/cli.rs`: end-to-end generation, overwrite refusal, status, dev server,
  check timeout/exit codes, checkpoint secret refusal.
- `tests/phase_zero.rs`: Rust/Python/Node fixtures keep source files and produce
  repeatable `STATE.md` after `new` and `regen`.
- `tests/phase_one.rs`: generic `init`, idempotence, conflict refusal, v1
  migration, symlink refusal.
- Unit tests cover serialization, slugging, deadlines, rendering, hashing,
  atomic writes, locking, and path validation.
