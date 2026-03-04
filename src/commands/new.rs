use anyhow::Result;
use colored::Colorize;
use inquire::{MultiSelect, Select, Text};

use super::run_repo_cmd;
use crate::config::{Config, RepoConfig, RepoType, expand_tilde};
use crate::git;
use crate::jira;

fn to_kebab_case(s: &str) -> String {
    let mut result = String::new();
    let mut prev_hyphen = true; // start true to suppress leading hyphens
    for ch in s.to_lowercase().chars() {
        if ch.is_alphanumeric() {
            result.push(ch);
            prev_hyphen = false;
        } else if !prev_hyphen {
            result.push('-');
            prev_hyphen = true;
        }
    }
    result.trim_end_matches('-').to_string()
}

pub async fn run(config: &Config, dry_run: bool, ticket: Option<&str>) -> Result<()> {
    // 1. Fetch issues
    eprint!("Fetching Jira issues...");
    let issues = jira::fetch_issues(config).await?;
    eprintln!(" {} issues", issues.len());

    // 2. Select issue
    let issue = match ticket {
        Some(key) => issues
            .into_iter()
            .find(|i| i.key == key)
            .ok_or_else(|| anyhow::anyhow!("Ticket {} not found", key))?,
        None => Select::new("Choose your Jira issue:", issues)
            .with_page_size(15)
            .prompt()?,
    };

    // 3. Description slug
    let default_slug = to_kebab_case(&issue.fields.summary);
    let description = Text::new("Short description:")
        .with_default(&default_slug)
        .prompt()?;

    // 4. Repos
    let repos = MultiSelect::new("Which repo(s)?", config.repos.clone())
        .with_all_selected_by_default()
        .prompt()?;

    // 5. Branch type
    let branch_type =
        Select::new("Branch type:", config.common.branch_prefixes.clone()).prompt()?;

    // 6. Per-repo base branch
    let repos_with_base: Vec<(RepoConfig, String)> = repos
        .into_iter()
        .map(|repo| {
            let default = repo
                .default_branch
                .clone()
                .unwrap_or_else(|| "master".to_string());
            let base = Text::new(&format!("Base branch for {}:", repo.name))
                .with_default(&default)
                .prompt()?;
            Ok((repo, base))
        })
        .collect::<Result<_>>()?;

    // 7. Preview
    let branch_name = config
        .common
        .render_branch(&branch_type, &issue.key, &description, None);
    println!();
    for (repo, base) in &repos_with_base {
        println!(
            "  {}: {} => {}",
            repo.name.cyan(),
            base.magenta(),
            branch_name.green().bold()
        );
    }
    println!();

    // 8. Confirm
    if !inquire::Confirm::new("Create above branch(es)?")
        .with_default(true)
        .prompt()?
    {
        println!("Aborted.");
        return Ok(());
    }

    // 9. Create
    for (repo, base) in &repos_with_base {
        let repo_root = expand_tilde(&repo.root);
        let branch_path = match repo.repo_type {
            RepoType::Worktree => repo_root
                .join(repo.feature_path.as_deref().unwrap_or("work"))
                .join(&branch_name),
            RepoType::Standard => repo_root.clone(),
        };

        match repo.repo_type {
            RepoType::Worktree => {
                if dry_run {
                    println!(
                        "{} git -C {} worktree add {} -b {} {}",
                        "[dry-run]".yellow(),
                        repo_root.display(),
                        branch_path.display(),
                        branch_name,
                        base
                    );
                } else {
                    println!("Creating worktree for {}...", repo.name.cyan());
                    git::create_worktree(&repo_root, &branch_path, &branch_name, base)?;
                    println!("  {} {}", "✓".green(), branch_path.display());
                }
            }
            RepoType::Standard => {
                if dry_run {
                    println!(
                        "{} git -C {} checkout -b {} {}",
                        "[dry-run]".yellow(),
                        repo_root.display(),
                        branch_name,
                        base
                    );
                } else {
                    println!("Creating branch for {}...", repo.name.cyan());
                    git::create_branch(&repo_root, &branch_name, base)?;
                    println!("  {} {}", "✓".green(), branch_name.green());
                }
            }
        }

        // 10. Post-creation commands
        let cmds = repo.commands.as_deref().unwrap_or(&[]);
        if !cmds.is_empty() {
            let branch_path_str = branch_path.to_string_lossy().to_string();
            let selected =
                MultiSelect::new(&format!("Run commands for {}?", repo.name), cmds.to_vec())
                    .prompt()?;

            for cmd in selected {
                run_repo_cmd(&cmd, &branch_path_str, dry_run)?;
            }
        }
    }

    Ok(())
}
