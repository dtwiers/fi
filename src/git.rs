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
