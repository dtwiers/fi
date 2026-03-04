use anyhow::Result;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: String,
}

pub fn list_worktrees(repo_root: &Path) -> Result<Vec<WorktreeInfo>> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["worktree", "list", "--porcelain"])
        .output()?;

    if !output.status.success() {
        anyhow::bail!(
            "git worktree list failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(parse_worktree_list(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_worktree_list(output: &str) -> Vec<WorktreeInfo> {
    output
        .split("\n\n")
        .filter_map(|block| {
            let block = block.trim();
            if block.is_empty() {
                return None;
            }
            let mut path = None;
            let mut branch = None;
            for line in block.lines() {
                if let Some(p) = line.strip_prefix("worktree ") {
                    path = Some(p.to_string());
                } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
                    branch = Some(b.to_string());
                }
            }
            // skip detached HEADs (no branch line)
            Some(WorktreeInfo {
                path: path?,
                branch: branch?,
            })
        })
        .collect()
}

pub fn is_dirty(path: &str) -> bool {
    Command::new("git")
        .current_dir(path)
        .args(["status", "--porcelain"])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

/// Fetch all remotes, pruning deleted remote branches.
pub fn fetch(repo_root: &Path) -> Result<()> {
    let status = Command::new("git")
        .current_dir(repo_root)
        .args(["fetch", "--all", "--prune"])
        .status()?;
    if !status.success() {
        anyhow::bail!("git fetch failed");
    }
    Ok(())
}

/// Check whether merging `feature_branch` into `origin/<target>` would produce
/// conflicts. Always uses the remote tracking ref so results reflect reality.
pub fn check_merge_conflicts(repo_root: &Path, feature_branch: &str, target: &str) -> bool {
    let remote_target = format!("origin/{target}");

    // Prefer git merge-tree --write-tree (git ≥ 2.38): exit 1 means conflicts.
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(["merge-tree", "--write-tree", &remote_target, feature_branch])
        .output();

    match out {
        Ok(o) if o.status.code() == Some(0) => return false,
        Ok(o) if o.status.code() == Some(1) => return true,
        _ => {}
    }

    // Fallback: 3-arg form (widely supported).
    let base = Command::new("git")
        .current_dir(repo_root)
        .args(["merge-base", feature_branch, &remote_target])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    if let Some(base) = base {
        Command::new("git")
            .current_dir(repo_root)
            .args(["merge-tree", &base, &remote_target, feature_branch])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("<<<<<<<"))
            .unwrap_or(false)
    } else {
        false
    }
}

/// Return true if `branch` exists as a local ref.
pub fn branch_exists(repo_root: &Path, branch: &str) -> bool {
    Command::new("git")
        .current_dir(repo_root)
        .args(["rev-parse", "--verify", &format!("refs/heads/{branch}")])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Return true if `ancestor` is reachable from `descendant` (i.e. all commits
/// of `ancestor` are already in `descendant`).
pub fn is_ancestor(repo_root: &Path, ancestor: &str, descendant: &str) -> bool {
    Command::new("git")
        .current_dir(repo_root)
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Return true if there are unresolved merge conflicts in `path`
/// (`git ls-files --unmerged` is non-empty).
pub fn has_unresolved_conflicts(path: &str) -> bool {
    Command::new("git")
        .current_dir(path)
        .args(["ls-files", "--unmerged"])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

/// Return the worktree path for `branch`, if it is checked out as a worktree.
pub fn find_worktree_for_branch(repo_root: &Path, branch: &str) -> Option<String> {
    list_worktrees(repo_root)
        .ok()?
        .into_iter()
        .find(|wt| wt.branch == branch)
        .map(|wt| wt.path)
}

/// Return the currently checked-out branch name, or None for detached HEAD.
#[allow(dead_code)]
pub fn current_branch(path: &str) -> Option<String> {
    Command::new("git")
        .current_dir(path)
        .args(["branch", "--show-current"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Merge `branch` into the current checkout at `path`.
/// Returns `Ok(true)` for a clean merge, `Ok(false)` if conflicts remain.
pub fn merge_into(path: &str, branch: &str) -> Result<bool> {
    let status = Command::new("git")
        .current_dir(path)
        .args(["merge", branch, "--no-edit"])
        .status()?;
    Ok(status.success())
}

/// Push `branch` to origin, setting upstream tracking if not already set.
pub fn push_branch(repo_root: &Path, branch: &str) -> Result<()> {
    let status = Command::new("git")
        .current_dir(repo_root)
        .args(["push", "--set-upstream", "origin", branch])
        .status()?;
    if !status.success() {
        anyhow::bail!("git push failed for {branch}");
    }
    Ok(())
}

pub fn create_worktree(
    repo_root: &Path,
    worktree_path: &Path,
    branch: &str,
    base: &str,
) -> Result<()> {
    if let Some(parent) = worktree_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let status = Command::new("git")
        .current_dir(repo_root)
        .args([
            "worktree",
            "add",
            worktree_path.to_str().unwrap_or(""),
            "-b",
            branch,
            base,
        ])
        .status()?;

    if !status.success() {
        anyhow::bail!("git worktree add failed for branch {}", branch);
    }
    Ok(())
}

pub fn create_branch(repo_root: &Path, branch: &str, base: &str) -> Result<()> {
    let status = Command::new("git")
        .current_dir(repo_root)
        .args(["checkout", "-b", branch, base])
        .status()?;

    if !status.success() {
        anyhow::bail!("git checkout -b failed for branch {}", branch);
    }
    Ok(())
}
