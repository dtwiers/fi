# Contributing to fi

Thanks for your interest in contributing! fi is a Rust CLI tool that integrates Jira with Git workflows.
This guide gets you from zero to an open pull request.

---

## Prerequisites

| Tool | Version | Notes |
|------|---------|-------|
| [Rust](https://rustup.rs) | 1.80+ | Install via `rustup` |
| Git | 2.30+ | Worktree support required |
| [GitHub CLI](https://cli.github.com) (`gh`) | any | Used by `fi pr` at runtime, not needed to build |

Verify your setup:

```sh
rustup show        # confirm active toolchain
cargo --version    # should be 1.80+
```

---

## Building locally

```sh
git clone https://github.com/block/fi.git   # adjust URL to match the actual repo
cd fi
cargo build                                  # debug build
cargo build --release                        # optimised → target/release/fi
```

Install the dev binary into your `$PATH`:

```sh
cargo install --path .
```

---

## Running the tests

```sh
cargo test                    # run all tests
cargo test -- --nocapture     # show println! output
```

CI enforces these checks — run them locally before pushing:

```sh
cargo fmt -- --check          # formatting
cargo clippy -- -D warnings   # linting (all warnings are errors)
```

Auto-fix formatting in one step:

```sh
cargo fmt
```

---

## Project layout

```
fi/
├── src/
│   ├── main.rs          CLI entry point (clap)
│   ├── config.rs        YAML config deserialization
│   ├── git.rs           git helpers (worktree list, create, branch status)
│   ├── jira.rs          Jira REST API client
│   └── commands/
│       ├── mod.rs       Shared helpers used across commands
│       ├── new.rs       fi new
│       ├── open.rs      fi open
│       ├── pr.rs        fi pr
│       ├── cull.rs      fi cull
│       └── init.rs      fi init
├── fi.schema.json       JSON Schema for fi.yaml (editor validation + autocomplete)
├── examples/
│   └── fi.yaml.example  Fully-commented example config
└── Cargo.toml
```

---

## Making a change

1. **Find or open an issue** before starting — comment on it so others know it's claimed.
2. **Fork** the repo and create a descriptive branch:
   ```sh
   git checkout -b fix/my-thing
   ```
3. Make your change. Add or update tests where relevant.
4. Run the full local check:
   ```sh
   cargo fmt && cargo clippy -- -D warnings && cargo test
   ```
5. **Open a pull request** against `main`. Fill in the PR template.
6. A maintainer will review within a few days.

---

## Code style

- Follow standard Rust idioms; `cargo clippy` is the authority.
- Use `anyhow::Result` for error propagation — avoid `unwrap()` in non-test code.
- Each command lives in `src/commands/<name>.rs` with a `pub async fn run(…)` entry point.
- Interactive prompts go through the `inquire` crate; keep UX consistent with existing commands.
- All destructive operations must support `--dry-run` / `-n`.

---

## Reporting bugs

Use the **Bug report** issue template on GitHub. Please include:

- Your OS and `fi --version` output _(version flag coming soon — include `cargo pkgid fi` for now)_
- The exact command you ran and the full error output
- A minimal `fi.yaml` snippet (redact real URLs and tokens)

---

## Questions?

Open a [GitHub Discussion](../../discussions) or file an issue tagged `question`.
