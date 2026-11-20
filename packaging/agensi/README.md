# Agensi package

Build and inspect the skill package:

```bash
# from the repository root
bash packaging/agensi/build.sh
```

Output: `packaging/agensi/ghost.zip` with `SKILL.md` at the archive root.

Layout:

```text
ghost/
  SKILL.md
  scripts/install.sh
  references/commands.md
  references/workflow.md
  references/security.md
  templates/answers.example.json
```

Before submitting, fill the bracketed fields in `LISTING.md` and check that the
skill id `ghost` is available.

The install script changes the host machine and therefore requires an explicit
`--yes`. The skill instructs the agent to ask the user first.
