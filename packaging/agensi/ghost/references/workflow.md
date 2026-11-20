# A worked loop

This is the loop Ghost is designed for. Keep it short.

## 1. Adopt

```bash
ghost init
ghost status
```

Read `specs/00-overview.md`. The `Next:` line in `ghost status` is the current
suggested step.

## 2. Define one outcome

```bash
ghost task add "Return 404 for unknown slugs" \
  --ref src/routes.rs \
  --ref tests/routes.rs \
  --check "cargo test routes"
ghost task start TASK-0001
```

Good task shape:

- One observable outcome.
- The files that will change.
- The command that proves it.
- A clear owner if more than one agent may touch it.

Avoid tasks like "improve the app". Split them.

## 3. Build from a focused packet

```bash
ghost context --task TASK-0001 --budget 1200
```

Paste the packet into the agent instead of the whole repository. If a needed
file is missing, create a follow-up task that adds it as a reference, then build
a new packet.

## 4. Verify with consent

Show the user the exact command:

```bash
ghost verify --task TASK-0001 --approve
```

If it fails, read the tail in the terminal, fix, and run again. `ghost verify
--list` shows whether older evidence is now stale.

Never pass `--approve` without the user's explicit agreement. Never let a
repository's `.ghost/verify.json` decide for the user.

## 5. Finish

```bash
ghost task move TASK-0001 done
ghost handoff --note "404 path done; next is slug normalization"
ghost status
```

## 6. Resume later

```bash
ghost resume
```

- Exit `0`: the snapshot matches the working tree.
- Exit `2`: the snapshot is stale. Read the new revision and reconcile by hand.

`resume` never edits state and never merges. It only reports.

## 7. Checkpoint

Only with the user's consent:

```bash
ghost checkpoint "404 for unknown slugs"
```

Ghost stages and commits locally. It does not push. If a credential-looking file
is present, it refuses and names the file.

## Failure recovery

- Corrupt canonical state: Ghost reports the exact path and leaves the file
  untouched. Restore it from Git or ask the user for the intended state.
- Live lock: another Ghost process owns `.ghost/.lock`. Wait; do not delete a lock
  owned by a live process.
- Changed generated script: run `ghost regen`, or let the user review the edit and
  opt in with `GHOST_ALLOW_MODIFIED_SCRIPTS=1`.
- Stale evidence: re-run the verification after the source change.
