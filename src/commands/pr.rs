use anyhow::Result;
use colored::Colorize;
use inquire::{Confirm, MultiSelect, Select, Text};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use crate::config::{Config, RepoConfig, RepoType, expand_tilde};
use crate::git;
use crate::template::{render_template, unescape};
use super::collect_ask_values;

// ── Branch parsing ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BranchInfo {
    pub prefix: String,      // e.g. "fix"
    pub ticket: String,      // e.g. "CAPY-1234"
    pub slug: String,        // e.g. "some-feature"
    pub pretty_title: String, // e.g. "Some Feature"
    /// Set when this IS a conflict branch (e.g. "DEVELOP").
    pub conflict_base: Option<String>,
}

const MINOR_WORDS: &[&str] = &[
    "a", "an", "the", "in", "on", "at", "to", "by", "for", "of", "from",
];

/// Parse a branch name into its components, guided by the configured `branch_format`.
///
/// Normal feature branch: `fix/CAPY-1234-some-feature`
///   → prefix=fix, ticket=CAPY-1234, slug=some-feature, conflict_base=None
///
/// Conflict branch: `fix/CAPY-1234-DEVELOP-some-feature`
///   → prefix=fix, ticket=CAPY-1234, slug=some-feature, conflict_base=Some("DEVELOP")
///
/// Conflict detection is driven entirely by the `{conflictBase: '...'}` clause in
/// `branch_format` — nothing is hardcoded.  If the format has no conflictBase
/// variable, conflict branches are never detected.
pub fn parse_branch(branch: &str, branch_format: &str) -> Option<BranchInfo> {
    let slash = branch.find('/')?;
    let prefix = branch[..slash].to_string();
    let after_slash = &branch[slash + 1..]; // e.g. "CAPY-1234-DEVELOP-some-slug"

    // Robustly find the ticket (ALL_CAPS letters then '-' then digits) at the
    // start of `after_slash` — does not assume a fixed segment count.
    let ticket_end = ticket_end_index(after_slash)?;
    let ticket = after_slash[..ticket_end].to_string();
    let after_ticket = &after_slash[ticket_end..]; // e.g. "-DEVELOP-some-slug" or "-some-slug"

    // Derive the separator that immediately precedes the conflictBase value from
    // the format template: for `{conflictBase: '-$1'}` this is "-".
    let cb_sep = conflict_base_separator(branch_format);

    let (conflict_base, slug) = match cb_sep {
        None => {
            // No conflictBase variable in the format — never a conflict branch.
            (None, after_ticket.trim_start_matches('-').to_string())
        }
        Some(sep) if after_ticket.starts_with(sep) => {
            let rest = &after_ticket[sep.len()..]; // e.g. "DEVELOP-some-slug" or "some-slug"
            // Try to read an ALL_CAPS conflict base word followed by another separator.
            if let Some(dash) = rest.find('-') {
                let candidate = &rest[..dash];
                let slug_part = &rest[dash + 1..];
                if !slug_part.is_empty() && is_all_caps_word(candidate) {
                    (Some(candidate.to_string()), slug_part.to_string())
                } else {
                    (None, rest.to_string())
                }
            } else {
                // Nothing after the potential CB — it's just the slug.
                (None, rest.to_string())
            }
        }
        Some(_) => (None, after_ticket.trim_start_matches('-').to_string()),
    };

    let pretty_title = to_title_case(&slug);
    Some(BranchInfo { prefix, ticket, slug, pretty_title, conflict_base })
}

/// Return the index just past the `PROJECT-1234` ticket at the start of `s`.
fn ticket_end_index(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    // One or more ASCII uppercase letters (project key).
    while i < bytes.len() && bytes[i].is_ascii_uppercase() { i += 1; }
    if i == 0 { return None; }
    // Literal '-'.
    if i >= bytes.len() || bytes[i] != b'-' { return None; }
    i += 1;
    // One or more ASCII digits.
    let num_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
    if i == num_start { return None; }
    Some(i)
}

/// Extract the literal string that precedes `$1` inside a `{conflictBase: '...'}` clause.
/// Returns `None` when no conflictBase variable exists in the template.
///
/// E.g. `"{branchPrefix}/{ticket.key}{conflictBase: '-$1'}-{slug}"` → `Some("-")`
fn conflict_base_separator(format: &str) -> Option<&str> {
    let tag = "{conflictBase:";
    let start = format.find(tag)? + tag.len();
    let rest = &format[start..];
    let close = rest.find('}')?;
    let inner = rest[..close].trim().trim_matches(|c| c == '\'' || c == '"');
    // inner is something like "-$1"; return what comes before "$1".
    let dollar = inner.find("$1")?;
    Some(&inner[..dollar])
}

/// A valid ALL_CAPS conflict base is 2+ uppercase ASCII letters (optionally
/// also underscores). Single-char segments like "A" are too ambiguous to count.
fn is_all_caps_word(s: &str) -> bool {
    s.len() >= 2 && s.chars().all(|c| c.is_ascii_uppercase() || c == '_')
}

fn to_title_case(slug: &str) -> String {
    slug.split('-')
        .enumerate()
        .map(|(i, word)| {
            if word.len() > 1 && word.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
                word.to_string()
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

// ── PR state detection ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum PrStatus {
    None,
    Open(String),
    Merged(String),
    Closed(String),
}

impl PrStatus {
    fn label(&self) -> &str {
        match self {
            PrStatus::None       => "none",
            PrStatus::Open(_)    => "open",
            PrStatus::Merged(_)  => "merged",
            PrStatus::Closed(_)  => "closed",
        }
    }
}

fn check_pr_status(head: &str, base: &str) -> PrStatus {
    // gh pr list scoped to this repo is fine — `gh` picks up the remote automatically.
    let out = Command::new("gh")
        .args([
            "pr", "list",
            "--head", head,
            "--base", base,
            "--state", "all",
            "--json", "url,state",
            "--limit", "1",
        ])
        .output();

    let Ok(out) = out else { return PrStatus::None };
    if !out.status.success() { return PrStatus::None; }

    let text = String::from_utf8_lossy(&out.stdout);
    let text = text.trim();
    if text == "[]" || text.is_empty() { return PrStatus::None; }

    // Parse the first entry manually (avoid full JSON dep for two fields).
    let url = extract_json_str(text, "url").unwrap_or_default();
    let state = extract_json_str(text, "state").unwrap_or_default();

    match state.to_uppercase().as_str() {
        "OPEN"   => PrStatus::Open(url),
        "MERGED" => PrStatus::Merged(url),
        _        => PrStatus::Closed(url),
    }
}

fn extract_json_str(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":", key);
    let start = json.find(&needle)? + needle.len();
    let rest = json[start..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

// ── Per-target assessment ─────────────────────────────────────────────────────

#[derive(Debug)]
pub struct TargetAssessment {
    pub target: String,
    pub is_default: bool,
    /// True if merging feature → origin/<target> would conflict.
    pub has_conflict: bool,
    pub conflict_branch: String,
    pub conflict_branch_exists: bool,
    /// Feature branch has been merged into the conflict branch.
    pub feature_merged_in: bool,
    /// Unresolved merge conflicts remain in the conflict worktree.
    pub conflict_unresolved: bool,
    /// Path to conflict worktree (worktree repos only).
    pub conflict_worktree_path: Option<String>,
    pub conflict_pr: PrStatus,
    pub main_pr: PrStatus,
}

impl TargetAssessment {
    fn summary(&self) -> String {
        if self.has_conflict {
            if !self.conflict_branch_exists {
                format!("{} {} (conflict branch needed)", "⚠".yellow(), self.target.magenta())
            } else if self.conflict_unresolved {
                format!("{} {} (conflicts unresolved in {})", "⚡".yellow(), self.target.magenta(), self.conflict_branch.cyan())
            } else if !self.feature_merged_in {
                format!("{} {} (feature not merged into {})", "⚡".yellow(), self.target.magenta(), self.conflict_branch.cyan())
            } else if self.conflict_pr == PrStatus::None {
                format!("{} {} (conflict branch ready → PR needed)", "●".cyan(), self.target.magenta())
            } else {
                format!("{} {} ({} PR: {})", "✓".green(), self.target.magenta(), self.conflict_branch.cyan(), self.conflict_pr.label())
            }
        } else if self.main_pr == PrStatus::None {
            format!("{} {} (PR needed)", "○".cyan(), self.target)
        } else {
            format!("{} {} (PR: {})", "✓".green(), self.target, self.main_pr.label())
        }
    }
}

/// Fetch latest remote state then assess all targets in parallel.
pub async fn assess_all_targets(
    repo: &RepoConfig,
    feature_branch: &str,
    info: &BranchInfo,
    config: &Config,
    dry_run: bool,
) -> Result<Vec<TargetAssessment>> {
    let root = expand_tilde(&repo.root);
    let default_branch = repo.default_branch.as_deref().unwrap_or("master");
    let pr_to = repo.pr_to_branches.as_deref().unwrap_or(&[]);

    // Always fetch first so origin/* refs are current.
    if dry_run {
        println!("{} git fetch --all --prune", "[dry-run]".yellow());
    } else {
        print!("Fetching remote refs… ");
        std::io::Write::flush(&mut std::io::stdout()).ok();
        git::fetch(&root).unwrap_or_else(|e| eprintln!("\n{} fetch failed: {}", "⚠".yellow(), e));
        println!("{}", "done".green());
    }

    // Assess each target concurrently.
    let mut handles = Vec::new();
    for target in pr_to {
        let target = target.clone();
        let root = root.clone();
        let feature_branch = feature_branch.to_string();
        let conflict_branch = config.common.render_branch(
            &info.prefix, &info.ticket, &info.slug, Some(&target.to_uppercase()),
        );
        let is_default = target == default_branch;
        let merge_conflict_path = repo.merge_conflict_path.clone();
        let repo_type = repo.repo_type.clone();

        handles.push(tokio::task::spawn_blocking(move || -> TargetAssessment {
            let has_conflict = !dry_run && git::check_merge_conflicts(&root, &feature_branch, &target);

            let conflict_branch_exists = git::branch_exists(&root, &conflict_branch);

            let feature_merged_in = conflict_branch_exists
                && git::is_ancestor(&root, &feature_branch, &conflict_branch);

            // Conflict worktree path (worktree repos only).
            let conflict_worktree_path = if conflict_branch_exists {
                git::find_worktree_for_branch(&root, &conflict_branch)
            } else if let (RepoType::Worktree, Some(mcp)) = (&repo_type, &merge_conflict_path) {
                Some(root.join(mcp).join(&conflict_branch)
                    .to_string_lossy().to_string())
            } else {
                None
            };

            let conflict_unresolved = conflict_branch_exists
                && feature_merged_in
                && conflict_worktree_path.as_deref()
                    .map(|p| git::has_unresolved_conflicts(p))
                    .unwrap_or_else(|| git::has_unresolved_conflicts(root.to_str().unwrap_or(".")));

            let conflict_pr = if has_conflict {
                check_pr_status(&conflict_branch, &target)
            } else {
                PrStatus::None
            };
            let main_pr = check_pr_status(&feature_branch, &target);

            TargetAssessment {
                target,
                is_default,
                has_conflict,
                conflict_branch,
                conflict_branch_exists,
                feature_merged_in,
                conflict_unresolved,
                conflict_worktree_path,
                conflict_pr,
                main_pr,
            }
        }));
    }

    let mut assessments = Vec::new();
    for h in handles {
        assessments.push(h.await?);
    }
    Ok(assessments)
}

// ── Context detection ─────────────────────────────────────────────────────────

pub fn detect_context(config: &Config) -> Result<Option<(RepoConfig, String)>> {
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
    if branch.is_empty() { return Ok(None); }

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

// ── PR creation ───────────────────────────────────────────────────────────────

struct PrSpec {
    head: String,
    base: String,
    title: String,
    body: String,
}

fn build_pr_vars(
    _info: &BranchInfo,
    ticket: &str,
    pretty_title: &str,
    target: &str,
    default_branch: &str,
    conflict_base: Option<&str>,
    ask_vals: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    let target_prefix = if target == default_branch { String::new() } else { target.to_string() };
    vars.insert("pr.targetPrefix".into(), target_prefix);
    vars.insert("branch.prettyTitle".into(), pretty_title.to_string());
    vars.insert("ticket.key".into(), ticket.to_string());
    vars.insert("pr.conflictBase".into(), conflict_base.unwrap_or("").to_string());
    for (k, v) in ask_vals {
        vars.insert(format!("ask.{}", k), v.clone());
    }
    vars
}

fn create_pr(spec: &PrSpec, draft: bool, dry_run: bool) -> bool {
    if dry_run {
        println!(
            "{} gh pr create --base {} --head {} --title {:?}",
            "[dry-run]".yellow(), spec.base, spec.head, spec.title
        );
        return true;
    }

    println!("Creating PR {} → {}…", spec.head.cyan(), spec.base.magenta());
    let mut cmd = Command::new("gh");
    cmd.args(["pr", "create",
        "--base", &spec.base,
        "--head", &spec.head,
        "--title", &spec.title,
        "--body", &spec.body,
    ]);
    if draft { cmd.arg("--draft"); }
    let ok = cmd.status().map(|s| s.success()).unwrap_or(false);
    if ok { println!("  {}", "✓ Created".green()); } else { eprintln!("  {} Failed", "✗".red()); }
    ok
}

// ── Conflict branch creation ──────────────────────────────────────────────────

fn create_conflict_branch(
    repo: &RepoConfig,
    assessment: &TargetAssessment,
    feature_branch: &str,
    dry_run: bool,
) -> Result<Option<String>> {
    let root = expand_tilde(&repo.root);
    // Always base conflict branches on origin/<target> — latest remote state.
    let remote_base = format!("origin/{}", assessment.target);

    match repo.repo_type {
        RepoType::Worktree => {
            let mcp = repo.merge_conflict_path.as_deref()
                .ok_or_else(|| anyhow::anyhow!(
                    "mergeConflictPath not set for worktree repo {}", repo.name
                ))?;
            let wt_path = root.join(mcp).join(&assessment.conflict_branch);
            println!(
                "Creating conflict worktree {} based on {}…",
                assessment.conflict_branch.cyan(),
                remote_base.yellow()
            );
            if dry_run {
                println!(
                    "{} git worktree add {} -b {} {}",
                    "[dry-run]".yellow(),
                    wt_path.display(), assessment.conflict_branch, remote_base
                );
            } else {
                git::create_worktree(&root, &wt_path, &assessment.conflict_branch, &remote_base)?;
                println!("  Merging {}…", feature_branch.cyan());
                let clean = git::merge_into(wt_path.to_str().unwrap_or("."), feature_branch)?;
                if clean {
                    println!("  {} Clean merge — no conflicts!", "✓".green());
                } else {
                    println!("  {} Merge conflicts detected. Resolve, then run: fi pr --continue", "⚡".yellow());
                }
            }
            Ok(Some(wt_path.to_string_lossy().to_string()))
        }
        RepoType::Standard => {
            println!(
                "Creating conflict branch {} based on {}…",
                assessment.conflict_branch.cyan(),
                remote_base.yellow()
            );
            if dry_run {
                println!(
                    "{} git checkout -b {} {}",
                    "[dry-run]".yellow(), assessment.conflict_branch, remote_base
                );
            } else {
                git::create_branch(&root, &assessment.conflict_branch, &remote_base)?;
                println!("  Merging {}…", feature_branch.cyan());
                let clean = git::merge_into(root.to_str().unwrap_or("."), feature_branch)?;
                if clean {
                    println!("  {} Clean merge — no conflicts!", "✓".green());
                } else {
                    println!("  {} Merge conflicts detected. Resolve, then run: fi pr --continue", "⚡".yellow());
                }
            }
            Ok(None)
        }
    }
}

// ── Main command ──────────────────────────────────────────────────────────────

pub async fn run(config: &Config, dry_run: bool, continue_mode: bool) -> Result<()> {
    // Detect context — could be on the feature branch OR a conflict branch.
    let (repo, current_branch) = match detect_context(config)? {
        Some(ctx) => {
            println!("Detected: {} on {}", ctx.0.name.cyan(), ctx.1.green().bold());
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

    let branch_fmt = config.common.branch_format.as_deref()
        .unwrap_or("{branchPrefix}/{ticket.key}-{slug}");

    let parsed = parse_branch(&current_branch, branch_fmt)
        .ok_or_else(|| anyhow::anyhow!("Cannot parse ticket from branch: {}", current_branch))?;

    // Determine the feature branch — may need to switch if on a conflict branch.
    let feature_branch = if let Some(ref cb) = parsed.conflict_base {
        if !continue_mode {
            anyhow::bail!(
                "You appear to be on a conflict branch (conflict base: {}). \
                 Use `fi pr --continue` to submit the PR.",
                cb
            );
        }
        // Derive the original feature branch by rendering without conflictBase.
        config.common.render_branch(&parsed.prefix, &parsed.ticket, &parsed.slug, None)
    } else {
        current_branch.clone()
    };

    // Parse the feature branch info (slug, ticket, prefix).
    let feature_info = if parsed.conflict_base.is_some() {
        parse_branch(&feature_branch, branch_fmt)
            .ok_or_else(|| anyhow::anyhow!("Cannot parse ticket from branch: {}", feature_branch))?
    } else {
        parsed.clone()
    };

    // Let the user confirm / correct ticket and title.
    let ticket = Text::new("Ticket:").with_default(&feature_info.ticket).prompt()?;
    let pretty_title = Text::new("Title:").with_default(&feature_info.pretty_title).prompt()?;

    let tmpl = repo.pr_template.as_ref()
        .ok_or_else(|| anyhow::anyhow!("No prTemplate configured for {}", repo.name))?;
    let default_branch = repo.default_branch.as_deref().unwrap_or("master");

    let ask_vals = collect_ask_values(tmpl.ask.as_ref())?;

    // Assess all targets (fetches first, uses origin/* refs).
    let assessments = assess_all_targets(&repo, &feature_branch, &feature_info, config, dry_run).await?;

    // Show the full picture.
    println!();
    for a in &assessments {
        println!("  {}", a.summary());
    }
    println!();

    // In continue mode: if currently on a conflict branch, handle this PR first.
    if continue_mode {
        if let Some(ref cb) = parsed.conflict_base {
            // Find matching assessment.
            let target = cb.to_lowercase();
            let a = assessments.iter().find(|a| a.target == target);

            if let Some(a) = a {
                if a.conflict_unresolved {
                    anyhow::bail!(
                        "Unresolved merge conflicts remain in {}. Resolve them, stage changes, \
                         and commit before running `fi pr --continue`.",
                        a.conflict_branch
                    );
                }
                // Build and create the PR for this conflict branch.
                let vars = build_pr_vars(
                    &feature_info, &ticket, &pretty_title,
                    &a.target, default_branch, Some(cb), &ask_vals,
                );
                let title_default = unescape(&render_template(&tmpl.title, &vars));
                let body = unescape(&render_template(&tmpl.body, &vars));

                println!("{}", "── Body preview ────────────────────────────".dimmed());
                for line in body.lines() { println!("  {}", line.dimmed()); }
                println!("{}", "────────────────────────────────────────────".dimmed());

                let title = Text::new(&format!("Conflict PR title (→ {}):", a.target))
                    .with_default(&title_default).prompt()?;
                let draft = Confirm::new("Create as draft?").with_default(false).prompt()?;

                if Confirm::new("Create this PR?").with_default(true).prompt()? {
                    create_pr(&PrSpec {
                        head: a.conflict_branch.clone(),
                        base: a.target.clone(),
                        title,
                        body,
                    }, draft, dry_run);
                }
            }
        }
    }

    // Check default branch — hard fail if it conflicts.
    let default_assessment = assessments.iter().find(|a| a.is_default);
    if let Some(da) = default_assessment {
        if da.has_conflict && da.main_pr == PrStatus::None {
            anyhow::bail!(
                "Merge conflicts detected between {} and origin/{}! \
                 Resolve conflicts on {} before proceeding.",
                feature_branch.cyan(), da.target.magenta(), da.target
            );
        }
    }

    // Non-conflicting targets: select which PRs to create.
    let clean_targets_needing_prs: Vec<&TargetAssessment> = assessments.iter()
        .filter(|a| !a.has_conflict && a.main_pr == PrStatus::None)
        .collect();

    if !clean_targets_needing_prs.is_empty() {
        let target_names: Vec<String> = clean_targets_needing_prs.iter()
            .map(|a| a.target.clone()).collect();
        let selected = MultiSelect::new("Create PRs to:", target_names)
            .with_all_selected_by_default().prompt()?;

        if !selected.is_empty() {
            let mut prs = Vec::new();
            for target in &selected {
                let vars = build_pr_vars(
                    &feature_info, &ticket, &pretty_title,
                    target, default_branch, None, &ask_vals,
                );
                let title_default = unescape(&render_template(&tmpl.title, &vars));
                let body = unescape(&render_template(&tmpl.body, &vars));

                println!("{}", "── Body preview ────────────────────────────".dimmed());
                for line in body.lines() { println!("  {}", line.dimmed()); }
                println!("{}", "────────────────────────────────────────────".dimmed());

                let title = Text::new(&format!("Title (→ {}):", target))
                    .with_default(&title_default).prompt()?;
                prs.push(PrSpec { head: feature_branch.clone(), base: target.clone(), title, body });
            }

            let draft = Confirm::new("Create as drafts?").with_default(false).prompt()?;

            println!();
            for p in &prs {
                println!("  {} → {}: {}", p.head.cyan(), p.base.magenta(), p.title.green().bold());
            }
            println!();

            if Confirm::new("Create these PRs?").with_default(true).prompt()? {
                for p in &prs { create_pr(p, draft, dry_run); }
            }
        }
    }

    // Conflicting non-default targets: create conflict branches as needed.
    let needs_conflict_branch: Vec<&TargetAssessment> = assessments.iter()
        .filter(|a| !a.is_default && a.has_conflict && !a.conflict_branch_exists)
        .collect();

    if !needs_conflict_branch.is_empty() {
        println!();
        println!(
            "{} The following targets have merge conflicts — conflict resolution branches will be created:",
            "⚠".yellow()
        );
        for a in &needs_conflict_branch {
            println!("  {} → {}", feature_branch.cyan(), a.conflict_branch.magenta());
        }
        println!();

        if !dry_run && !Confirm::new("Create conflict branches?").with_default(true).prompt()? {
            println!("Skipping conflict branches.");
            return Ok(());
        }

        for a in &needs_conflict_branch {
            create_conflict_branch(&repo, a, &feature_branch, dry_run)?;

            // Offer to open the conflict worktree.
            if let Some(ref wt_path) = a.conflict_worktree_path {
                if let Some(cmds) = repo.commands.as_ref() {
                    let open_cmd = cmds.iter().find(|c| c.command == "open");
                    if let Some(cmd) = open_cmd {
                        if !dry_run
                            && Confirm::new(&format!("Open {} in your editor?", a.conflict_branch))
                                .with_default(true).prompt()?
                        {
                            super::run_repo_cmd(cmd, wt_path, dry_run)?;
                        }
                    }
                }
            }

            println!(
                "  → Resolve merge conflicts in {}, then run: {}",
                a.conflict_branch.cyan(),
                "fi pr --continue".bold()
            );
        }
    }

    // Conflict branches that exist but haven't had the feature merged in.
    let needs_merge: Vec<&TargetAssessment> = assessments.iter()
        .filter(|a| a.conflict_branch_exists && !a.feature_merged_in)
        .collect();
    for a in &needs_merge {
        println!(
            "  {} Feature branch not yet merged into {} — run `fi sync` to update",
            "⚡".yellow(), a.conflict_branch.cyan()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FMT: &str = "{branchPrefix}/{ticket.key}{conflictBase: '-$1'}-{slug}";
    const FMT_PLAIN: &str = "{branchPrefix}/{ticket.key}-{slug}";

    #[test]
    fn parse_normal_branch() {
        let b = parse_branch("fix/CAPY-1234-some-feature", FMT).unwrap();
        assert_eq!(b.prefix, "fix");
        assert_eq!(b.ticket, "CAPY-1234");
        assert_eq!(b.slug, "some-feature");
        assert!(b.conflict_base.is_none());
    }

    #[test]
    fn parse_conflict_branch() {
        let b = parse_branch("fix/CAPY-1234-DEVELOP-some-feature", FMT).unwrap();
        assert_eq!(b.ticket, "CAPY-1234");
        assert_eq!(b.slug, "some-feature");
        assert_eq!(b.conflict_base, Some("DEVELOP".into()));
    }

    #[test]
    fn parse_conflict_branch_staging() {
        let b = parse_branch("fix/CAPY-1234-STAGING-fix-wy-claim", FMT).unwrap();
        assert_eq!(b.conflict_base, Some("STAGING".into()));
        assert_eq!(b.slug, "fix-wy-claim");
    }

    #[test]
    fn no_conflict_base_when_format_lacks_it() {
        // If branchFormat has no {conflictBase}, never detect conflict branches.
        let b = parse_branch("fix/CAPY-1234-DEVELOP-some-feature", FMT_PLAIN).unwrap();
        assert!(b.conflict_base.is_none());
        assert_eq!(b.slug, "DEVELOP-some-feature");
    }

    #[test]
    fn single_segment_after_ticket_is_not_conflict_base() {
        // A branch like fix/CAPY-1234-DEVELOP (no slug after) should not parse CB.
        let b = parse_branch("fix/CAPY-1234-DEVELOP", FMT).unwrap();
        assert!(b.conflict_base.is_none());
    }
}
