use anyhow::Result;
use colored::Colorize;
use std::path::PathBuf;

use crate::config::find_config;

const EXAMPLE_CONFIG: &str = r#"# yaml-language-server: $schema=https://raw.githubusercontent.com/dtwiers/fi/main/fi.schema.json
---
version: 1

jira:
  # Base URL of your Jira instance
  baseUrl: https://myorg.atlassian.net
  # Board ID (visible in the URL when viewing your board)
  boardId: 123
  # Quick filter ID to scope which issues appear in `fi new`
  quickFilterId: 456
  # Optional: extra JQL appended to the issue query
  # jqlExtension: "assignee = currentUser()"
  token:
    # Name of the env var holding your base64-encoded "email:token" Jira API token
    # Generate: echo -n "your@email.com:your_token" | base64
    env: JIRA_TOKEN

common:
  # Available branch type prefixes
  branchPrefixes:
    - feature
    - fix
    - chore
    - wip
    - hotfix
  # Which prefix is pre-selected when creating a branch
  defaultBranchPrefix: feature
  # Branch name format. Supports {branchPrefix}, {ticket.key}, {slug}.
  # Default if omitted: "{branchPrefix}/{ticket.key}-{slug}"
  branchFormat: "{branchPrefix}/{ticket.key}-{slug}"

repos:
  # ── Example: git worktree repo (bare repo) ────────────────────────────────
  - name: My API
    # Path to the bare repo. Tilde is expanded.
    root: ~/proj/my-api.git
    # "worktree" for bare repos using git worktrees
    type: worktree
    # The main integration branch
    defaultBranch: master
    # New worktrees are created under root/featurePath/
    featurePath: work
    # Worktrees under root/persistentPath/ are never shown in `fi cull`
    persistentPath: persistent
    # These branch names are excluded from `fi cull` and `fi open`
    persistentBranches:
      - master
      - develop
    # Target branches offered when running `fi pr` (all pre-selected)
    prToBranches:
      - master
      - develop
    prTemplate:
      # Fields to prompt for before rendering the PR. Available as {ask.<name>} in templates.
      ask:
        description:
          type: editor   # opens $EDITOR; press Esc to skip
          optional: true
      # Template syntax:
      #   {variable}                    plain substitution
      #   {variable: 'format $1'}       conditional — only rendered if variable is non-empty
      # Available variables: branch.prettyTitle, ticket.key, pr.targetPrefix, ask.<name>
      # pr.targetPrefix is the target branch name, or "" if target == defaultBranch
      title: "{pr.targetPrefix: '[$1]: '}{branch.prettyTitle}"
      body: |
        ### Ticket: {ticket.key}
        {ask.description: '\n### Description: $1'}
    commands:
      # The "open" command is invoked when you select a worktree in `fi open`
      - command: open
        # Script is written to a temp file; runner is called as: fish <tempfile>
        runner: fish
        ask:
          shouldInit: boolean
        env:
          # Template vars available: {branch.path}, {ask.<name>}
          BRANCH_PATH: "{branch.path}"
          SHOULD_INIT: "{ask.shouldInit}"
        run: |
          echo "Opening $BRANCH_PATH"
          # Example: open editor + a side pane with wezterm
          # set -l PANE_ID (wezterm cli spawn --cwd "$BRANCH_PATH")
          # wezterm cli send-text "nvim\n" --pane-id $PANE_ID

  # ── Example: standard (non-worktree) repo ─────────────────────────────────
  - name: My Config
    root: ~/proj/my-config
    type: standard
    defaultBranch: main
    prToBranches:
      - main
      - staging
    prTemplate:
      ask:
        description:
          type: editor
          optional: true
      title: "{pr.targetPrefix: '[$1]: '}{branch.prettyTitle}"
      body: |
        ### Ticket: {ticket.key}
        {ask.description: '\n### Description: $1'}
"#;

pub fn run(force: bool) -> Result<()> {
    // Determine config path - use ~/.config/fi/fi.yaml on all platforms for consistency
    // This matches the path that find_config() looks for in config.rs
    let config_path: PathBuf = dirs::home_dir()
        .map(|h| h.join(".config").join("fi").join("fi.yaml"))
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;

    // Check if already exists
    if config_path.exists() && !force {
        println!(
            "{} Config already exists at {}",
            "✗".red().bold(),
            config_path.display().to_string().yellow()
        );
        println!(
            "  Run {} to overwrite.",
            "fi init --force".cyan()
        );
        // Still print the example so they can reference it
        println!();
        println!("{}", "── Example config (not written) ─────────────────────────".dimmed());
        print!("{}", EXAMPLE_CONFIG.dimmed());
        return Ok(());
    }

    // Ensure directory exists
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Check existing valid config (even with --force, show a warning)
    if config_path.exists() && force {
        // Try to parse — if it's valid, warn before overwriting
        if find_config(Some(config_path.to_str().unwrap_or(""))).is_ok() {
            println!(
                "{} Overwriting valid config at {}",
                "⚠".yellow().bold(),
                config_path.display().to_string().yellow()
            );
        }
    }

    std::fs::write(&config_path, EXAMPLE_CONFIG)?;

    println!(
        "{} Created {}",
        "✓".green().bold(),
        config_path.display().to_string().cyan()
    );
    println!();
    println!("Next steps:");
    println!(
        "  1. Edit {} and fill in your Jira details and repos.",
        config_path.display().to_string().cyan()
    );
    println!(
        "  2. For editor autocomplete/validation, point your YAML language server at:"
    );
    println!("       {}", "fi.schema.json".cyan());
    println!(
        "     (the # yaml-language-server comment at the top of the file does this automatically)"
    );
    println!("  3. Run {} to test your config.", "fi new --dry-run".cyan());

    Ok(())
}
