# fi

A personal developer workflow CLI for teams using Jira + GitHub. `fi` connects your issue tracker to your git workflow — browse tickets, create branches or worktrees, open them in your editor, submit PRs, and clean up when you're done.

Built in Rust. Fast, interactive, driven by a single YAML config file.

---

## Features

| Command | What it does |
|---|---|
| `fi new` | Pick a Jira ticket, create a branch or worktree |
| `fi open` | List worktrees, select one, run your custom open script |
| `fi pr` | Create GitHub PRs; auto-detects and manages merge-conflict branches |
| `fi pr --continue` | Continue PR creation after resolving merge conflicts |
| `fi sync` | Push feature branch updates into existing conflict branches |
| `fi cull` | Multi-select stale worktrees to delete (shows dirty/unpushed/merged status) |
| `fi init` | Write a starter config to `~/.config/fi/fi.yaml` |
| `fi completions` | Print shell completion scripts for bash, zsh, fish, or nushell |

---

## Installation

```sh
# Clone and install
git clone https://github.com/YOU/fi
cd fi
cargo install --path .

# Generate shell completions (example: fish)
fi completions fish > ~/.config/fish/completions/fi.fish
```

Requires Rust 1.81+. The only runtime dependency is the [`gh` CLI](https://cli.github.com) (for `fi pr`).

---

## Quick start

```sh
# 1. Generate a starter config
fi init

# 2. Edit it
$EDITOR ~/.config/fi/fi.yaml

# 3. Try creating a branch without actually doing it
fi new --dry-run
```

---

## Config file

Config lives at `~/.config/fi/fi.yaml`. A JSON Schema is included at `fi.schema.json` in this repo for editor validation and autocomplete — the generated config wires it up automatically via a `# yaml-language-server` comment.

### Top-level structure

```yaml
version: 1
jira:   { ... }
common: { ... }
repos:  [ ... ]
```

### `jira`

Connects to your Jira instance to fetch issues for `fi new`.

```yaml
jira:
  baseUrl: https://myorg.atlassian.net
  boardId: 131          # numeric board ID (visible in the board URL)
  quickFilterId: 137    # scopes which issues appear — use a "My work" filter
  jqlExtension: "assignee = currentUser()"  # optional extra JQL
  token:
    env: JIRA_TOKEN     # env var holding base64("email:token")
```

To generate your token:
```sh
echo -n "you@company.com:your_jira_api_token" | base64
# paste result into JIRA_TOKEN env var (e.g. in ~/.config/fish/config.fish)
```

### `common`

Branch naming conventions shared across all repos.

```yaml
common:
  branchPrefixes: [feature, fix, chore, wip, hotfix]
  defaultBranchPrefix: feature
  branchFormat: "{branchPrefix}/{ticket.key}-{slug}"  # optional, this is the default
```

### `repos`

A list of repositories. Each can be a `worktree` (bare repo using `git worktree`) or `standard` (regular checkout).

```yaml
repos:
  - name: My API
    root: ~/proj/my-api.git   # path to the bare repo
    type: worktree
    defaultBranch: master
    featurePath: work          # new worktrees go in root/work/
    persistentPath: persistent # worktrees here are never shown in `fi cull`
    mergeConflictPath: conflicts # conflict-resolution worktrees go here (used by fi pr)
    persistentBranches: [master, develop]
    prToBranches: [master, develop]
    prTemplate: { ... }
    commands: [ ... ]
```

#### `prTemplate`

Controls the title and body of PRs created with `fi pr`.

```yaml
prTemplate:
  ask:
    description:
      type: editor    # opens $EDITOR before rendering
      optional: true  # Esc to skip
  title: "{pr.targetPrefix: '[$1]: '}{branch.prettyTitle}"
  body: |
    ### Ticket: {ticket.key}
    {ask.description: '\n### Description: $1'}
```

**Template variables:**

| Variable | Value |
|---|---|
| `{branch.prettyTitle}` | Branch slug converted to Title Case (e.g. `Fix Wy Claim Report`) |
| `{ticket.key}` | Jira ticket key (e.g. `PROJ-1234`) |
| `{pr.targetPrefix}` | Target branch name, or `""` if target is the default branch |
| `{pr.conflictBase}` | Uppercase conflict base (e.g. `DEVELOP`), or `""` for non-conflict PRs |
| `{ask.<name>}` | Value collected from an `ask` field |

**Conditional syntax:** `{variable: 'format with $1'}` — only rendered if the variable is non-empty. `$1` is replaced with the value. Useful for optional sections:

```
{pr.targetPrefix: '[$1]: '}{branch.prettyTitle}
# → "[staging]: Fix Wy Claim Report"  (when targeting staging)
# → "Fix Wy Claim Report"             (when targeting master)
```

#### `commands`

Custom scripts that run after creating or opening a worktree. The script is written to a temp file and executed as `<runner> <tempfile>`.

```yaml
commands:
  - command: open      # "open" is special: invoked automatically by `fi open`
    runner: fish       # or bash, /usr/local/bin/fish, etc.
    ask:
      shouldInit: boolean   # prompts "shouldInit? [y/n]"
    env:
      BRANCH_PATH: "{branch.path}"
      SHOULD_INIT: "{ask.shouldInit}"
    run: |
      set -l PANE_ID (wezterm cli spawn --cwd "$BRANCH_PATH")
      wezterm cli send-text "nvim\n" --pane-id $PANE_ID
```

**`ask` field types:**

| Type | Behaviour |
|---|---|
| `boolean` | Yes/no confirm prompt. Sets env var to `"true"` or `"false"`. |
| `editor` | Opens `$EDITOR`. Saves content to env var. Add `optional: true` to allow skipping with Esc. |
| `text` | Single-line text input. |

---

## Commands

### `fi new`

Fetches your Jira board (filtered by `quickFilterId`), shows a searchable list of tickets, and creates a branch or git worktree.

```
fi new [--dry-run] [--ticket PROJ-1234]
```

Flow:
1. Fetch and fuzzy-search Jira issues (shows key, summary, status, assignee)
2. Enter a short description (becomes the branch slug)
3. Select branch type prefix (`feature`, `fix`, etc.)
4. Select which repos to create the branch in
5. For each worktree repo, choose a base branch (defaults to `defaultBranch`)
6. Preview the branch name → confirm → create
7. Optionally run any configured `commands` (multi-select)

`--dry-run` prints the git commands without running them.

---

### `fi open`

Lists all non-persistent worktrees across all worktree-type repos and runs the `open` command on the selected one.

```
fi open [--dry-run]
```

Worktrees are color-coded:
- 🟡 **yellow** — dirty (uncommitted changes)
- 🔴 **red** — persistent branch (shown for context, not selectable in cull)
- 🟢 **green** — clean

After selecting, you're prompted for any `ask` fields defined on the `open` command, then the script runs.

---

### `fi pr`

Creates GitHub pull requests for the current branch. Uses `gh pr create` under the hood and handles merge-conflict scenarios automatically.

```
fi pr [--dry-run]
fi pr --continue
```

#### Normal flow (no merge conflicts)

1. **Auto-detect** the repo and branch from `$PWD` (or prompt if ambiguous)
2. **Fetch** the latest state from the remote so conflict checks use fresh refs
3. **Parse** the branch name into ticket key + pretty title — both shown as editable prompts
4. Collect any `ask` fields (e.g. open `$EDITOR` for a description)
5. **Assess all targets in parallel** — for each branch in `prToBranches`, `fi pr` checks:
   - Does merging your feature branch into `origin/<target>` conflict? (`git merge-tree`)
   - Does a PR already exist (open, merged, or closed)?
6. Show a status summary for every target
7. For non-conflicting targets with no open PR: select which to create (all pre-selected)
8. For each selected target: preview the rendered body, edit the title, choose draft → confirm → create

#### Conflict resolution flow

When `fi pr` finds merge conflicts between your feature branch and one or more **non-default** target branches (e.g. `develop`, `staging`), it follows this workflow:

```
feature/PROJ-1234-my-work
    ├── → master         ✓ no conflict  →  PR created normally
    └── → develop        ⚠ conflict     →  conflict branch created
```

**Step 1 — conflict branch creation:**

For each conflicting non-default target, `fi pr` creates a *conflict branch* named after your feature branch with the conflict base embedded. For example:

```
# branchFormat: "{branchPrefix}/{ticket.key}{conflictBase: '-$1'}-{slug}"
feature/PROJ-1234-my-work  +  target: develop
    → feature/PROJ-1234-DEVELOP-my-work
```

For **worktree repos**, the conflict branch is created as a new worktree under `mergeConflictPath`. For **standard repos**, it's a normal `git checkout -b`. Either way, fi then merges the feature branch in — which will produce a merge conflict.

**Step 2 — manual resolution:**

fi opens the conflict worktree in your editor (via the `open` command) and prints:

```
→ Resolve merge conflicts in feature/PROJ-1234-DEVELOP-my-work, then run: fi pr --continue
```

Resolve the conflicts, stage the result, and commit.

**Step 3 — `fi pr --continue`:**

Run `fi pr --continue` from either the feature branch or the conflict branch. fi re-assesses the situation from scratch each time — it never relies on stale state:

| State detected | Action |
|---|---|
| Conflict branch doesn't exist yet | Creates it (same as Step 1) |
| Merge conflicts unresolved | Bails with a helpful message |
| Feature not yet merged in | Reminds you to run `fi sync` |
| Conflict branch ready | Creates the PR for the conflict branch |
| PR already open | Shows existing URL, skips creation |

The PR for a conflict branch targets the *conflict base branch* (e.g. `develop`), and the `{pr.conflictBase}` template variable is set to `DEVELOP` (uppercased) so it can appear in the PR title/body.

**Hard fail:** if the **default branch** (e.g. `master`) has merge conflicts, `fi pr` stops immediately — those conflicts must be resolved on the feature branch itself before any PRs are created.

#### `fi sync`

After adding commits to your feature branch, run `fi sync` to propagate them to any existing conflict branches:

```
fi sync [--dry-run]
```

For each conflict branch that exists:
1. Merges the latest feature branch into the conflict branch
2. Pushes the conflict branch to the remote
3. Checks if the associated PR is still open — if it was merged or closed, prompts to recreate it

Run `fi sync` from anywhere inside the repo (feature branch or conflict branch both work).

#### Status indicators shown by `fi pr`

| Symbol | Meaning |
|---|---|
| `○ target (PR needed)` | Clean merge, no open PR yet |
| `✓ target (PR: open)` | PR already exists |
| `⚠ target (conflict branch needed)` | Conflict detected, conflict branch not yet created |
| `⚡ target (conflicts unresolved in …)` | Conflict branch exists but merge isn't resolved |
| `⚡ target (feature not merged into …)` | Conflict branch exists but `fi sync` hasn't run |
| `● target (conflict branch ready → PR needed)` | All resolved, ready for `fi pr --continue` |
| `✓ target (conflict-branch PR: open)` | Conflict PR already submitted |

#### Template variables for conflict PRs

| Variable | Value |
|---|---|
| `{pr.conflictBase}` | The uppercase conflict base (e.g. `DEVELOP`, `STAGING`) |

Example title template that works for both normal and conflict PRs:

```yaml
title: "{pr.conflictBase: '[$1 CONFLICT]: '}{pr.targetPrefix: '[$1]: '}{branch.prettyTitle}"
# Normal PR to master:   "My Feature Title"
# Normal PR to develop:  "[develop]: My Feature Title"
# Conflict PR:           "[DEVELOP CONFLICT]: My Feature Title"
```

---

### `fi cull`

Deletes selected worktrees from disk and from git.

```
fi cull [--dry-run]
```

Before showing the selection menu, `fi cull` checks every worktree's status **in parallel**:

| Status | Meaning |
|---|---|
| 🔴 `dirty` | Uncommitted changes — data loss risk |
| 🟡 `unpushed` | Committed locally but not on `origin/<branch>` |
| 🟢 `clean` | Pushed to remote, not yet merged |
| 🔵 `merged` | Present in `git branch --merged <defaultBranch>` — safe to delete |

Results are sorted dirty-first. After selecting and confirming, worktrees are removed 3 at a time (with live spinners) via:
1. `git worktree remove --force <path>`
2. `rm -rf <path>` (if anything remains)
3. `git branch -D <branch>`

---

### `fi init`

Writes a well-commented example config to `~/.config/fi/fi.yaml`. Will not overwrite an existing config unless you pass `--force`.

```
fi init [--force]
```

If the config already exists, the example is printed to stdout for reference without touching the file.

---

### `fi completions`

Prints shell completion scripts to stdout.

```sh
fi completions fish    > ~/.config/fish/completions/fi.fish
fi completions zsh     > ~/.zfunc/_fi
fi completions bash    > ~/.local/share/bash-completion/completions/fi
fi completions nushell > ~/.config/nushell/completions/fi.nu
```

---

## Project layout

```
fi/
├── src/
│   ├── main.rs          CLI entry point (clap)
│   ├── config.rs        YAML deserialization
│   ├── git.rs           git helpers (worktree list, create, branch status)
│   ├── jira.rs          Jira API client
│   └── commands/
│       ├── mod.rs       Shared helpers (run_repo_cmd, collect_ask_values, unescape, …)
│       ├── new.rs       fi new
│       ├── open.rs      fi open
│       ├── pr.rs        fi pr
│       ├── cull.rs      fi cull
│       └── init.rs      fi init
├── fi.schema.json       JSON Schema for fi.yaml (editor validation + autocomplete)
└── Cargo.toml
```

---

## Tips

- **Editor completions:** If you use the yaml-language-server (VSCode, Neovim + nvim-lspconfig, etc.), the `# yaml-language-server: $schema=...` comment at the top of your generated config enables inline validation and autocomplete for every field.

- **Dry run everything first:** All mutating commands (`new`, `pr`, `cull`, `open`) accept `--dry-run` / `-n`.

- **Multiple repos at once:** `fi new` lets you create the branch in multiple repos simultaneously — handy when a ticket touches both app and config repos.

- **Shell completions + `cargo install`:** After `cargo install --path .`, re-generate completions since the binary path changed. The completions cover all subcommands and flags.
