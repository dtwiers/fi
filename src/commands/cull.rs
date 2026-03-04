use anyhow::{Context, Result};
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use inquire::{Confirm, MultiSelect};
use std::fmt;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use tokio::task::JoinSet;

use crate::config::{Config, RepoConfig, RepoType, expand_tilde};
use crate::git::{WorktreeInfo, list_worktrees};

const CONCURRENCY: usize = 3;

#[derive(Debug, Clone, PartialEq)]
enum WtStatus {
    /// Uncommitted changes present — data loss risk
    Dirty,
    /// Committed but not pushed / no upstream — data loss if remote gone
    Unpushed,
    /// Pushed to remote but not merged — safe-ish
    Clean,
    /// Branch present in `git branch --merged <default>` — fully safe
    Merged,
}

impl fmt::Display for WtStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WtStatus::Dirty => write!(f, "{}", "dirty".red().bold()),
            WtStatus::Unpushed => write!(f, "{}", "unpushed".yellow().bold()),
            WtStatus::Clean => write!(f, "{}", "clean".green()),
            WtStatus::Merged => write!(f, "{}", "merged".cyan()),
        }
    }
}

struct CullTarget {
    repo: RepoConfig,
    worktree: WorktreeInfo,
    status: WtStatus,
}

impl fmt::Display for CullTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:<55} [{}]  {}",
            self.worktree.branch.yellow(),
            self.status,
            self.repo.name.dimmed(),
        )
    }
}

/// Compute the status of a single worktree.
fn worktree_status(
    wt_path: &str,
    repo_root: &Path,
    branch: &str,
    default_branch: &str,
) -> WtStatus {
    // 1. dirty?
    let dirty = Command::new("git")
        .args(["-C", wt_path, "status", "--porcelain"])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    if dirty {
        return WtStatus::Dirty;
    }

    // 2. unpushed? Compare against origin/<branch> directly — more reliable than
    //    @{u} which requires an explicit tracking ref to be configured.
    let remote_ref = format!("origin/{branch}");
    let unpushed = Command::new("git")
        .args(["-C", wt_path, "rev-parse", "--verify", &remote_ref])
        .output()
        .map(|o| {
            if !o.status.success() {
                // Remote branch doesn't exist at all → truly unpushed
                return true;
            }
            // Remote exists — check if HEAD has commits not on it
            Command::new("git")
                .args([
                    "-C",
                    wt_path,
                    "rev-list",
                    "--count",
                    &format!("{remote_ref}..HEAD"),
                ])
                .output()
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .trim()
                        .parse::<u32>()
                        .unwrap_or(0)
                        > 0
                })
                .unwrap_or(false)
        })
        .unwrap_or(true);
    if unpushed {
        return WtStatus::Unpushed;
    }

    // 3. merged into default branch?
    let merged = Command::new("git")
        .args([
            "-C",
            repo_root.to_str().unwrap_or("."),
            "branch",
            "--merged",
            default_branch,
        ])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|l| l.trim().trim_start_matches("* ") == branch)
        })
        .unwrap_or(false);
    if merged {
        return WtStatus::Merged;
    }

    WtStatus::Clean
}

pub async fn run(config: &Config, dry_run: bool) -> Result<()> {
    // Collect all cullable worktrees across all worktree-type repos
    let mut raw: Vec<(RepoConfig, WorktreeInfo)> = Vec::new();

    for repo in &config.repos {
        if repo.repo_type != RepoType::Worktree {
            continue;
        }
        let root = expand_tilde(&repo.root);
        let persistent: Vec<&str> = repo
            .persistent_branches
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|s| s.as_str())
            .collect();

        let worktrees = list_worktrees(&root).unwrap_or_default();
        for wt in worktrees {
            if !persistent.iter().any(|p| *p == wt.branch) {
                raw.push((repo.clone(), wt));
            }
        }
    }

    if raw.is_empty() {
        println!("{}", "No cullable worktrees found.".dimmed());
        return Ok(());
    }

    // Compute statuses concurrently on blocking threads (git calls are blocking I/O)
    println!("{}", "Checking worktree statuses…".dimmed());
    let mut status_set: JoinSet<(RepoConfig, WorktreeInfo, WtStatus)> = JoinSet::new();
    for (repo, wt) in raw {
        let default_branch = repo
            .default_branch
            .clone()
            .unwrap_or_else(|| "master".to_string());
        let root = expand_tilde(&repo.root);
        status_set.spawn_blocking(move || {
            let st = worktree_status(&wt.path, &root, &wt.branch, &default_branch);
            (repo, wt, st)
        });
    }

    let mut targets: Vec<CullTarget> = Vec::new();
    while let Some(res) = status_set.join_next().await {
        if let Ok((repo, worktree, status)) = res {
            targets.push(CullTarget {
                repo,
                worktree,
                status,
            });
        }
    }

    // Sort: dirty first, then unpushed, then clean, then merged
    targets.sort_by_key(|t| match t.status {
        WtStatus::Dirty => 0,
        WtStatus::Unpushed => 1,
        WtStatus::Clean => 2,
        WtStatus::Merged => 3,
    });

    let selected = MultiSelect::new("Select worktrees to cull:", targets).prompt()?;

    if selected.is_empty() {
        println!("{}", "Nothing selected.".dimmed());
        return Ok(());
    }

    // Confirmation
    let names: Vec<String> = selected.iter().map(|t| t.worktree.branch.clone()).collect();
    let confirmed = Confirm::new(&format!("Cull {} worktree(s)?", selected.len()))
        .with_help_message(&names.join(", "))
        .with_default(false)
        .prompt()?;

    if !confirmed {
        println!("{}", "Aborted.".dimmed());
        return Ok(());
    }

    if dry_run {
        for t in &selected {
            let root = expand_tilde(&t.repo.root);
            println!(
                "{} git -C {} worktree remove --force {}  &&  rm -rf {}",
                "[dry-run]".cyan(),
                root.display(),
                t.worktree.path,
                t.worktree.path,
            );
        }
        return Ok(());
    }

    // Concurrent deletion with per-item spinners
    let mp = MultiProgress::new();
    let spinner_style = ProgressStyle::with_template("{spinner:.cyan} {msg}")
        .unwrap()
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", ""]);

    let mut join_set: JoinSet<Result<String>> = JoinSet::new();
    let mut iter = selected.into_iter().peekable();

    loop {
        while join_set.len() < CONCURRENCY {
            let Some(target) = iter.next() else { break };

            let root = expand_tilde(&target.repo.root);
            let wt_path = target.worktree.path.clone();
            let branch = target.worktree.branch.clone();
            let pb = mp.add(ProgressBar::new_spinner());
            pb.set_style(spinner_style.clone());
            pb.enable_steady_tick(Duration::from_millis(80));
            pb.set_message(format!("Culling {}…", branch.yellow()));

            join_set.spawn(async move {
                cull_worktree(&root, &wt_path, &branch, &pb).await?;
                pb.finish_with_message(format!("{} {}", "✓".green(), branch));
                Ok(branch)
            });
        }

        if join_set.is_empty() {
            break;
        }

        if let Some(res) = join_set.join_next().await {
            match res {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => eprintln!("{} {}", "✗ error:".red(), e),
                Err(e) => eprintln!("{} {}", "✗ task panic:".red(), e),
            }
        }
    }

    mp.clear()?;
    println!("{}", "Done.".green().bold());
    Ok(())
}

async fn cull_worktree(
    repo_root: &Path,
    wt_path: &str,
    branch: &str,
    _pb: &ProgressBar,
) -> Result<()> {
    let status = Command::new("git")
        .args([
            "-C",
            repo_root.to_str().unwrap_or("."),
            "worktree",
            "remove",
            "--force",
            wt_path,
        ])
        .output()
        .with_context(|| format!("git worktree remove {branch}"))?;

    if !status.status.success() {
        let stderr = String::from_utf8_lossy(&status.stderr);
        anyhow::bail!("git worktree remove failed for {branch}: {stderr}");
    }

    if std::path::Path::new(wt_path).exists() {
        std::fs::remove_dir_all(wt_path).with_context(|| format!("rm -rf {wt_path}"))?;
    }

    // Non-fatal: branch may already be gone after worktree remove
    let _ = Command::new("git")
        .args([
            "-C",
            repo_root.to_str().unwrap_or("."),
            "branch",
            "-D",
            branch,
        ])
        .output();

    Ok(())
}
