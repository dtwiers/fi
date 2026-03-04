# Security Policy

## Reporting a vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

If you discover a security issue in fi, please report it privately:

1. **GitHub private advisory** (preferred): go to _Security → Advisories → Report a vulnerability_ in this repository.
2. **Email**: send details to the maintainers. Check the repository's GitHub profile or Cargo.toml for contact information.

Please include:
- A description of the vulnerability and its potential impact
- Steps to reproduce or a proof-of-concept
- The fi version and OS you tested on
- Any suggested mitigations, if you have them

We aim to acknowledge reports within **48 hours** and provide a resolution timeline within **7 days**.

## Scope

fi is a local CLI tool. The main attack surfaces are:

| Surface | Notes |
|---------|-------|
| `fi.yaml` config parsing | Config is read from `~/.config/fi/fi.yaml` — attacker would need write access to your home directory |
| Jira API token handling | Tokens are read from environment variables; fi never writes tokens to disk |
| `commands[].run` scripts | Scripts are written to a temp file and executed — malicious config files can run arbitrary code |
| Git operations | fi shells out to `git` and `gh`; path traversal in repo roots is a potential concern |

## Supported versions

Only the latest release on `main` receives security fixes.
