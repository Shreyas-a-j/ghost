# Security policy

## Supported versions

| Version | Supported |
| --- | --- |
| 2.0.x | Yes |
| 0.1.x | No |

## Reporting a vulnerability

Open a private security advisory on this repository:

<https://github.com/Shreyas-a-j/ghost/security/advisories/new>

If that is unavailable, contact the maintainer through the repository profile.
Do not open a public issue for an exploitable problem.

Include:

- the Ghost version (`ghost --version`) and operating system;
- the command and its exit code;
- the smallest reproduction you can share;
- the affected `.ghost/` file names, with secrets removed.

Do not include `.env` contents, tokens, private keys, or customer data.

## Scope

In scope:

- path traversal or symlink escape in generated writes;
- execution of a command without the documented approval path;
- secret disclosure through context packets, evidence, journal, or checkpoint;
- terminal escape injection from repository-controlled data;
- state corruption caused by untrusted input.

Out of scope:

- an approved command doing something malicious (approval is not a sandbox);
- a repository containing intentionally misleading text for an agent;
- secrets with filenames that the documented name-based filter does not cover;
- unsupported operating systems (Windows).

The full trust model is in `docs/THREAT-MODEL.md` and
`packaging/agensi/ghost/references/security.md`.
