use anyhow::Result;
use colored::Colorize;
use inquire::Select;
use std::fmt;
use tokio::task::JoinSet;

use super::{HookContext, execute_hook_decisions, merged_hooks, prompt_hook_confirmations, run_repo_cmd};
use crate::config::{Config, HookWhen, RepoConfig, RepoType, expand_tilde};
use crate::git::{WorktreeInfo, is_dirty, list_worktrees};

struct WorktreeOption {
    info: WorktreeInfo,
    repo: RepoConfig,
    is_persistent: bool,
    is_dirty: bool,
    multi_repo: bool,
}

impl fmt::Display for WorktreeOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let branch = if self.is_persistent {
            self.info.branch.blue().bold().to_string()
        } else if self.is_dirty {
            self.info.branch.yellow().bold().to_string()
        } else {
            self.info.branch.green().bold().to_string()
        };
        let dirty = if self.is_dirty {
            " ●".red().to_string()
        } else {
            String::new()
        };
        let repo_tag = if self.multi_repo {
            format!(" ({})", self.repo.name).cyan().to_string()
        } else {
            String::new()
        };
        let path = format!("  {}", self.info.path).dimmed().to_string();
        write!(f, "{}{}{}{}", branch, dirty, repo_tag, path)
    }
}

pub async fn run(config: &Config, dry_run: bool) -> Result<()> {
    if config.repos.is_empty() {
        println!("No repos configured.");
        return Ok(());
    }

    let multi_repo = config.repos.len() > 1;

    // Collect all (repo, worktree-like entry, is_persistent) tuples
    let mut raw: Vec<(RepoConfig, WorktreeInfo, bool)> = Vec::new();
    for repo in &config.repos {
        let repo_root = expand_tilde(&repo.root);
        let persistent = repo.persistent_branches.clone().unwrap_or_default();

        let entries: Vec<WorktreeInfo> = match repo.repo_type {
            RepoType::Worktree => list_worktrees(&repo_root).unwrap_or_default(),
            RepoType::Standard => {
                // Only show the currently checked-out branch — we don't switch branches
                let out = std::process::Command::new("git")
                    .current_dir(&repo_root)
                    .args(["branch", "--show-current"])
                    .output();
                match out {
                    Ok(o) if o.status.success() => {
                        let branch = String::from_utf8_lossy(&o.stdout).trim().to_string();
                        if branch.is_empty() {
                            vec![] // detached HEAD
                        } else {
                            vec![WorktreeInfo {
                                path: repo_root.to_string_lossy().to_string(),
                                branch,
                            }]
                        }
                    }
                    _ => vec![],
                }
            }
        };

        for wt in entries {
            let is_persistent = persistent.contains(&wt.branch);
            raw.push((repo.clone(), wt, is_persistent));
        }
    }

    if raw.is_empty() {
        println!("No worktrees or branches found.");
        return Ok(());
    }

    eprint!("Checking {} worktrees...", raw.len());

    // Run dirty checks in parallel on blocking threads
    let mut join_set: JoinSet<(RepoConfig, WorktreeInfo, bool, bool)> = JoinSet::new();
    for (repo, wt, is_persistent) in raw {
        join_set.spawn_blocking(move || {
            let dirty = is_dirty(&wt.path);
            (repo, wt, is_persistent, dirty)
        });
    }

    let mut options: Vec<WorktreeOption> = Vec::new();
    while let Some(res) = join_set.join_next().await {
        if let Ok((repo, wt, is_persistent, dirty)) = res {
            options.push(WorktreeOption {
                info: wt,
                repo,
                is_persistent,
                is_dirty: dirty,
                multi_repo,
            });
        }
    }

    // Sort: persistent first (blue), then dirty (yellow), then clean (green); alpha within groups
    options.sort_by_key(|o| {
        let group = if o.is_persistent {
            0
        } else if o.is_dirty {
            1
        } else {
            2
        };
        (group, o.info.branch.clone())
    });

    eprintln!(" done");

    let selected = Select::new("Select worktree to open:", options)
        .with_page_size(15)
        .prompt()?;

    let open_cmd = selected
        .repo
        .commands
        .as_deref()
        .and_then(|cmds| cmds.iter().find(|c| c.command == "open"))
        .cloned();

    let hook_ctx = HookContext {
        command: "open",
        repo: &selected.repo,
        branch_name: Some(&selected.info.branch),
        branch_path: Some(&selected.info.path),
    };
    let hooks = merged_hooks(config.hooks.as_ref(), selected.repo.hooks.as_ref());

    // Pre-hooks (run normally — open hasn't stolen focus yet)
    {
        use super::run_hooks_for;
        run_hooks_for(&hooks, HookWhen::Pre, &hook_ctx, dry_run)?;
    }

    // Prompt for optional post-hooks BEFORE open steals focus.
    let post_hook_decisions =
        prompt_hook_confirmations(&hooks, HookWhen::Post, &hook_ctx, dry_run)?;

    match open_cmd {
        Some(cmd) => run_repo_cmd(&cmd, &selected.info.path, dry_run)?,
        None => println!(
            "No 'open' command configured for {}. Path: {}",
            selected.repo.name, selected.info.path
        ),
    }

    execute_hook_decisions(&post_hook_decisions, &hook_ctx, dry_run)?;

    Ok(())
}
