use anyhow::Result;
use colored::Colorize;
use inquire::Select;
use std::fmt;

use crate::config::{Config, RepoConfig, RepoType, expand_tilde};
use crate::git::{WorktreeInfo, is_dirty, list_worktrees};
use super::run_repo_cmd;

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
    let worktree_repos: Vec<&RepoConfig> = config
        .repos
        .iter()
        .filter(|r| r.repo_type == RepoType::Worktree)
        .collect();

    if worktree_repos.is_empty() {
        println!("No worktree repos configured.");
        return Ok(());
    }

    let multi_repo = worktree_repos.len() > 1;

    eprint!("Fetching worktrees...");
    let mut options: Vec<WorktreeOption> = Vec::new();
    for repo in &worktree_repos {
        let repo_root = expand_tilde(&repo.root);
        let persistent = repo.persistent_branches.clone().unwrap_or_default();
        for wt in list_worktrees(&repo_root).unwrap_or_default() {
            let is_persistent = persistent.contains(&wt.branch);
            let dirty = is_dirty(&wt.path);
            options.push(WorktreeOption {
                info: wt,
                repo: (*repo).clone(),
                is_persistent,
                is_dirty: dirty,
                multi_repo,
            });
        }
    }
    eprintln!(" {} worktrees", options.len());

    if options.is_empty() {
        println!("No worktrees found.");
        return Ok(());
    }

    let selected = Select::new("Select worktree to open:", options)
        .with_page_size(15)
        .prompt()?;

    let open_cmd = selected
        .repo
        .commands
        .as_deref()
        .and_then(|cmds| cmds.iter().find(|c| c.command == "open"))
        .cloned();

    match open_cmd {
        Some(cmd) => run_repo_cmd(&cmd, &selected.info.path, dry_run)?,
        None => println!(
            "No 'open' command configured for {}. Path: {}",
            selected.repo.name, selected.info.path
        ),
    }

    Ok(())
}
