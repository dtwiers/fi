use anyhow::Result;
use colored::Colorize;
use inquire::{Confirm, MultiSelect, Select, Text};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use crate::config::{Config, RepoConfig, RepoType, expand_tilde};
use crate::git;
use super::{collect_ask_values, unescape};

// ── Branch parsing ────────────────────────────────────────────────────────────

pub struct BranchInfo {
    pub ticket: String,
    pub pretty_title: String,
}

const MINOR_WORDS: &[&str] = &[
    "a", "an", "the", "in", "on", "at", "to", "by", "for", "of", "from",
];

pub fn parse_branch(branch: &str) -> Option<BranchInfo> {
    // Expected: type/PROJECT-1234-some-slug
    let slash = branch.find('/')?;
    let rest = &branch[slash + 1..]; // "PROJECT-1234-some-slug"

    // First two dash-segments are the ticket: PROJECT + 1234
    let mut parts = rest.splitn(3, '-');
    let project = parts.next()?;
    let number = parts.next()?;
    let slug = parts.next()?;

    if !number.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    let ticket = format!("{}-{}", project, number);
    let pretty_title = to_title_case(slug);

    Some(BranchInfo { ticket, pretty_title })
}

fn to_title_case(slug: &str) -> String {
    slug.split('-')
        .enumerate()
        .map(|(i, word)| {
            // Keep ALL_CAPS words (e.g. API, AWS) as-is
            if word.len() > 1 && word.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
                word.to_string()
            // Minor words are lowercase unless first word
            } else if i > 0 && MINOR_WORDS.contains(&word) {
                word.to_string()
            } else {
                let mut s = word.to_string();
                if let Some(first) = s.get_mut(0..1) {
                    first.make_ascii_uppercase();
                }
                s
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ── Template rendering ────────────────────────────────────────────────────────
//
// Supports two forms:
//   {variable}                  → substitute value
//   {variable: 'fmt with $1'}   → if value non-empty, substitute $1; else ""

fn render_template(template: &str, vars: &HashMap<String, String>) -> String {
    let mut result = String::new();
    let mut rest = template;

    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        rest = &rest[start + 1..];

        let Some(end) = rest.find('}') else {
            result.push('{');
            continue;
        };

        let inner = &rest[..end];
        rest = &rest[end + 1..];

        if let Some(colon) = inner.find(':') {
            let var_name = inner[..colon].trim();
            let fmt = inner[colon + 1..]
                .trim()
                .trim_matches(|c| c == '\'' || c == '"');
            let value = vars.get(var_name).map(|s| s.as_str()).unwrap_or("");
            if !value.is_empty() {
                result.push_str(&fmt.replace("$1", value));
            }
        } else {
            let value = vars.get(inner.trim()).map(|s| s.as_str()).unwrap_or("");
            result.push_str(value);
        }
    }

    result.push_str(rest);
    result
}

// ── Context detection ─────────────────────────────────────────────────────────

fn detect_context(config: &Config) -> Result<Option<(RepoConfig, String)>> {
    let pwd = std::env::current_dir()?;

    let git_out = Command::new("git")
        .current_dir(&pwd)
        .args(["rev-parse", "--git-common-dir"])
        .output();

    let Ok(git_out) = git_out else { return Ok(None) };
    if !git_out.status.success() { return Ok(None); }

    let raw = String::from_utf8_lossy(&git_out.stdout).trim().to_string();
    let git_common_dir: PathBuf = if raw.starts_with('/') {
        PathBuf::from(&raw)
    } else {
        pwd.join(&raw)
    };
    let git_common_dir = git_common_dir.canonicalize().unwrap_or(git_common_dir);

    let branch_out = Command::new("git")
        .current_dir(&pwd)
        .args(["branch", "--show-current"])
        .output()?;

    if !branch_out.status.success() { return Ok(None); }
    let branch = String::from_utf8_lossy(&branch_out.stdout).trim().to_string();
    if branch.is_empty() { return Ok(None); } // detached HEAD

    for repo in &config.repos {
        let root = expand_tilde(&repo.root);
        let root = root.canonicalize().unwrap_or_else(|_| expand_tilde(&repo.root));

        let matches = match repo.repo_type {
            RepoType::Worktree => git_common_dir == root,
            RepoType::Standard => git_common_dir == root.join(".git"),
        };

        if matches {
            return Ok(Some((repo.clone(), branch)));
        }
    }

    Ok(None)
}

fn list_feature_branches(repo: &RepoConfig) -> Result<Vec<String>> {
    let root = expand_tilde(&repo.root);
    match repo.repo_type {
        RepoType::Worktree => {
            let persistent = repo.persistent_branches.as_deref().unwrap_or(&[]);
            Ok(git::list_worktrees(&root)
                .unwrap_or_default()
                .into_iter()
                .filter(|wt| !persistent.iter().any(|p| p == &wt.branch))
                .map(|wt| wt.branch)
                .collect())
        }
        RepoType::Standard => {
            let out = Command::new("git")
                .current_dir(&root)
                .args(["branch", "--list", "--format=%(refname:short)"])
                .output()?;
            Ok(String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect())
        }
    }
}

// ── Main command ──────────────────────────────────────────────────────────────

pub async fn run(config: &Config, dry_run: bool) -> Result<()> {
    // 1. Detect or ask for repo + branch
    let (repo, branch) = match detect_context(config)? {
        Some(ctx) => {
            println!(
                "Detected: {} on {}",
                ctx.0.name.cyan(),
                ctx.1.green().bold()
            );
            ctx
        }
        None => {
            let repo = Select::new("Which repo?", config.repos.clone()).prompt()?;
            let branches = list_feature_branches(&repo)?;
            anyhow::ensure!(!branches.is_empty(), "No feature branches found in {}", repo.name);
            let branch = Select::new("Which branch?", branches).prompt()?;
            (repo, branch)
        }
    };

    // 2. Parse branch name → ticket + pretty title, then let user correct
    let info = parse_branch(&branch)
        .ok_or_else(|| anyhow::anyhow!("Cannot parse ticket from branch: {}", branch))?;

    let ticket = Text::new("Ticket:")
        .with_default(&info.ticket)
        .prompt()?;
    let pretty_title = Text::new("Title:")
        .with_default(&info.pretty_title)
        .prompt()?;

    // 3. Look up PR config
    let tmpl = repo
        .pr_template
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No prTemplate configured for {}", repo.name))?;
    let pr_to = repo
        .pr_to_branches
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("No prToBranches configured for {}", repo.name))?;
    let default_branch = repo.default_branch.as_deref().unwrap_or("master");

    // 4. Collect template ask fields (e.g. description editor)
    let ask_vals = collect_ask_values(tmpl.ask.as_ref())?;

    // 5. Select target branches (all pre-selected)
    let targets = MultiSelect::new("Create PRs to:", pr_to.to_vec())
        .with_all_selected_by_default()
        .prompt()?;

    // 6. Per-target: show computed title, let user edit
    struct PrInfo { target: String, title: String, body: String }
    let mut prs: Vec<PrInfo> = Vec::new();

    for target in &targets {
        let target_prefix = if target == default_branch {
            String::new()
        } else {
            target.clone()
        };

        let mut vars: HashMap<String, String> = HashMap::new();
        vars.insert("pr.targetPrefix".into(), target_prefix);
        vars.insert("branch.prettyTitle".into(), pretty_title.clone());
        vars.insert("ticket.key".into(), ticket.clone());
        for (k, v) in &ask_vals {
            vars.insert(format!("ask.{}", k), v.clone());
        }

        let default_title = unescape(&render_template(&tmpl.title, &vars));
        let body = unescape(&render_template(&tmpl.body, &vars));

        let title = Text::new(&format!("Title (→ {}):", target))
            .with_default(&default_title)
            .prompt()?;

        println!("{}", "── Body preview ────────────────────────────".dimmed());
        for line in body.lines() {
            println!("  {}", line.dimmed());
        }
        println!("{}", "────────────────────────────────────────────".dimmed());

        prs.push(PrInfo { target: target.clone(), title, body });
    }

    // 7. Draft?
    let draft = Confirm::new("Create as drafts?").with_default(false).prompt()?;

    // 8. Summary + confirm
    println!();
    for pr in &prs {
        println!(
            "  {} → {}{}",
            branch.cyan(),
            pr.target.magenta(),
            format!(": {}", pr.title).green().bold()
        );
    }
    if draft { println!("  (all as drafts)"); }
    println!();

    if !Confirm::new("Create these PRs?").with_default(true).prompt()? {
        println!("Aborted.");
        return Ok(());
    }

    // 9. Create
    for pr in &prs {
        if dry_run {
            println!(
                "{} gh pr create --base {} --head {} --title {:?} --body {:?}",
                "[dry-run]".yellow(), pr.target, branch, pr.title, pr.body
            );
            continue;
        }

        println!("Creating PR {} → {}...", branch.cyan(), pr.target.magenta());
        let mut cmd = Command::new("gh");
        cmd.args(["pr", "create",
            "--base", &pr.target,
            "--head", &branch,
            "--title", &pr.title,
            "--body", &pr.body,
        ]);
        if draft { cmd.arg("--draft"); }

        let status = cmd.status()?;
        if status.success() {
            println!("  {}", "✓ Created".green());
        } else {
            eprintln!("  {} Failed to create PR for {}", "✗".red(), pr.target);
        }
    }

    Ok(())
}
