use anyhow::Result;
use colored::Colorize;
use inquire::Confirm;

use crate::config::{Config, RepoType, expand_tilde};
use crate::git;
use super::pr::{detect_context, parse_branch, PrStatus, assess_all_targets};

/// `fi sync` — keep conflict branches up to date with the feature branch.
///
/// For each conflict branch in the repo:
///   1. Fetch latest remotes
///   2. Merge the feature branch into the conflict worktree / branch
///   3. Push the conflict branch
///   4. Recreate any merged/closed PRs (prompts confirmation)
pub async fn run(config: &Config, dry_run: bool) -> Result<()> {
    let (repo, current_branch) = match detect_context(config)? {
        Some(ctx) => {
            println!("Detected: {} on {}", ctx.0.name.cyan(), ctx.1.green().bold());
            ctx
        }
        None => {
            anyhow::bail!("Not inside a known repo. Run `fi sync` from within a repo directory.");
        }
    };

    let branch_fmt = config.common.branch_format.as_deref()
        .unwrap_or("{branchPrefix}/{ticket.key}-{slug}");

    let parsed = parse_branch(&current_branch, branch_fmt)
        .ok_or_else(|| anyhow::anyhow!("Cannot parse ticket from branch: {}", current_branch))?;

    // Determine feature branch (may be on a conflict branch).
    let feature_branch = if parsed.conflict_base.is_some() {
        config.common.render_branch(&parsed.prefix, &parsed.ticket, &parsed.slug, None)
    } else {
        current_branch.clone()
    };

    let feature_info = if parsed.conflict_base.is_some() {
        parse_branch(&feature_branch, branch_fmt)
            .ok_or_else(|| anyhow::anyhow!("Cannot parse ticket from branch: {}", feature_branch))?
    } else {
        parsed.clone()
    };

    // Assess all targets (includes fetch).
    let assessments = assess_all_targets(&repo, &feature_branch, &feature_info, config, dry_run).await?;

    let root = expand_tilde(&repo.root);

    // Find all conflict branches that exist.
    let conflict_targets: Vec<_> = assessments.iter()
        .filter(|a| a.has_conflict && a.conflict_branch_exists)
        .collect();

    if conflict_targets.is_empty() {
        println!("No active conflict branches to sync.");
        return Ok(());
    }

    for a in &conflict_targets {
        println!();
        println!("Syncing {}…", a.conflict_branch.cyan());

        let wt_path = match a.conflict_worktree_path.as_deref() {
            Some(p) => p.to_string(),
            None => root.to_string_lossy().to_string(),
        };

        // For standard repos: switch to conflict branch first.
        if repo.repo_type == RepoType::Standard {
            if dry_run {
                println!("{} git switch {}", "[dry-run]".yellow(), a.conflict_branch);
            } else {
                let status = std::process::Command::new("git")
                    .current_dir(&root)
                    .args(["switch", &a.conflict_branch])
                    .status()?;
                if !status.success() {
                    eprintln!("  {} Failed to switch to {}", "✗".red(), a.conflict_branch);
                    continue;
                }
            }
        }

        // Merge feature branch into the conflict branch / worktree.
        if dry_run {
            println!(
                "{} git merge {} --no-edit  (in {})",
                "[dry-run]".yellow(), feature_branch, wt_path
            );
        } else {
            print!("  Merging {}… ", feature_branch.cyan());
            std::io::Write::flush(&mut std::io::stdout()).ok();
            let clean = git::merge_into(&wt_path, &feature_branch)?;
            if clean {
                println!("{}", "clean".green());
            } else {
                println!("{}", "conflicts! Resolve and run `fi pr --continue`".yellow());
                if repo.repo_type == RepoType::Standard {
                    // Switch back so the user isn't stranded.
                    let _ = std::process::Command::new("git")
                        .current_dir(&root)
                        .args(["switch", &feature_branch])
                        .status();
                }
                continue;
            }
        }

        // Push.
        if dry_run {
            println!("{} git push --set-upstream origin {}", "[dry-run]".yellow(), a.conflict_branch);
        } else {
            print!("  Pushing… ");
            std::io::Write::flush(&mut std::io::stdout()).ok();
            match git::push_branch(&root, &a.conflict_branch) {
                Ok(_) => println!("{}", "done".green()),
                Err(e) => { eprintln!("{}", e); continue; }
            }
        }

        // For standard repos: switch back to feature branch.
        if repo.repo_type == RepoType::Standard && !dry_run {
            let _ = std::process::Command::new("git")
                .current_dir(&root)
                .args(["switch", &feature_branch])
                .status();
        }

        // Recreate PR if it was merged or closed.
        match &a.conflict_pr {
            PrStatus::Merged(url) | PrStatus::Closed(url) => {
                println!(
                    "  {} Conflict PR was {} ({}). Recreate it?",
                    "⚡".yellow(),
                    if matches!(a.conflict_pr, PrStatus::Merged(_)) { "merged" } else { "closed" },
                    url
                );
                let should_recreate = dry_run
                    || Confirm::new("Recreate PR?").with_default(true).prompt()?;
                if should_recreate {
                    let msg = format!("fi pr --continue while on {}", a.conflict_branch.cyan());
                    println!("  Run: {}", msg.bold());
                }
            }
            PrStatus::Open(url) => {
                println!("  {} PR already open: {}", "✓".green(), url);
            }
            PrStatus::None => {
                println!(
                    "  {} No PR found. Run `fi pr --continue` from {} to create one.",
                    "⚡".yellow(), a.conflict_branch.cyan()
                );
            }
        }
    }

    println!();
    println!("{}", "Sync complete.".green().bold());
    Ok(())
}
