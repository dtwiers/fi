use anyhow::Result;
use colored::Colorize;
use std::process::Command;

use crate::config::{Config, RepoType, expand_tilde};
use crate::git::list_worktrees;

pub async fn run(config: &Config) -> Result<()> {
    for repo in &config.repos {
        let root = expand_tilde(&repo.root);
        println!(
            "{}  {}",
            repo.name.bold(),
            format!("({})", root.display()).dimmed()
        );

        match repo.repo_type {
            RepoType::Worktree => {
                let worktrees = list_worktrees(&root).unwrap_or_default();
                if worktrees.is_empty() {
                    println!("  {}", "(no worktrees)".dimmed());
                } else {
                    let persistent = repo.persistent_branches.clone().unwrap_or_default();
                    let default_branch = repo.default_branch.as_deref().unwrap_or("master");

                    for wt in &worktrees {
                        let is_persistent = persistent.contains(&wt.branch);
                        let is_default = wt.branch == default_branch;

                        let branch_display = if is_persistent || is_default {
                            wt.branch.blue().bold().to_string()
                        } else {
                            wt.branch.green().to_string()
                        };

                        let dirty_marker = if is_worktree_dirty(&wt.path) {
                            format!(" {}", "●".red())
                        } else {
                            String::new()
                        };

                        let tag = if is_persistent || is_default {
                            format!(" {}", "[persistent]".dimmed())
                        } else {
                            String::new()
                        };

                        println!(
                            "  {}{}{}  {}",
                            branch_display,
                            dirty_marker,
                            tag,
                            wt.path.dimmed()
                        );
                    }
                }
            }

            RepoType::Standard => {
                let current = current_branch(&root);
                let dirty_marker = if is_standard_dirty(&root) {
                    format!(" {}", "●".red())
                } else {
                    String::new()
                };

                match current {
                    Some(branch) => println!(
                        "  {}{}  {}",
                        branch.green().bold(),
                        dirty_marker,
                        root.display().to_string().dimmed()
                    ),
                    None => println!("  {}", "(could not read branch)".dimmed()),
                }
            }
        }

        println!(); // blank line between repos
    }

    Ok(())
}

fn current_branch(repo_root: &std::path::Path) -> Option<String> {
    let output = Command::new("git")
        .args([
            "-C",
            repo_root.to_str().unwrap_or("."),
            "branch",
            "--show-current",
        ])
        .output()
        .ok()?;
    if output.status.success() {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if branch.is_empty() {
            // Detached HEAD — show short SHA
            let sha = Command::new("git")
                .args([
                    "-C",
                    repo_root.to_str().unwrap_or("."),
                    "rev-parse",
                    "--short",
                    "HEAD",
                ])
                .output()
                .ok()?;
            Some(format!(
                "(detached) {}",
                String::from_utf8_lossy(&sha.stdout).trim()
            ))
        } else {
            Some(branch)
        }
    } else {
        None
    }
}

fn is_worktree_dirty(path: &str) -> bool {
    Command::new("git")
        .args(["-C", path, "status", "--porcelain"])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

fn is_standard_dirty(root: &std::path::Path) -> bool {
    Command::new("git")
        .args(["-C", root.to_str().unwrap_or("."), "status", "--porcelain"])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}
