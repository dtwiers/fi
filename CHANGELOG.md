# Changelog

All notable changes to fi are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
fi uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added
- `fi completions <shell>` — generate shell completion scripts for bash, zsh, fish, and nushell
- `fi init` — write a well-commented starter config to `~/.config/fi/fi.yaml`
- `fi init --force` — overwrite an existing config (warns when current config is valid)
- `--dry-run` / `-n` flag on all mutating commands (`new`, `pr`, `cull`, `open`)
- JSON Schema (`fi.schema.json`) for `fi.yaml` — enables editor autocomplete and inline validation
- `yaml-language-server` comment embedded in generated config for zero-config LSP integration
- Worktree status badges in `fi cull`: 🔴 dirty · 🟡 unpushed · 🟢 clean · 🔵 merged
- Parallel worktree status checks and parallel deletions (3 at a time with live spinners) in `fi cull`
- Multi-repo branch creation in `fi new` (create the same branch across multiple repos at once)
- MIT license

### Fixed
- Config path consistency: `fi init` and `find_config()` now both use `~/.config/fi/fi.yaml`
  on all platforms (previously `fi init` wrote to macOS Application Support on macOS)

---

## [0.1.0] — TBD

_Initial public release._
