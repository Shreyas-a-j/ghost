# Agensi package

Build and inspect the skill package:

```bash
# from the repository root
bash packaging/agensi/build.sh
```

Output: `packaging/agensi/ghost.zip` with `SKILL.md` at the archive root.

The packaged zip ships with `ghost.zip.sha256` and a Sigstore bundle
`ghost.zip.sigstore.json`. Verify them before uploading or installing:

```bash
sha256sum -c ghost.zip.sha256
cosign verify-blob ghost.zip \
  --bundle ghost.zip.sigstore.json \
  --certificate-identity <email in the signing certificate> \
  --certificate-oidc-issuer https://accounts.google.com
```

Rebuilding the zip invalidates both files; re-sign after any rebuild.

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
