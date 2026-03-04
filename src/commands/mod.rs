pub mod config;
pub mod cull;
pub mod init;
pub mod list;
pub mod new;
pub mod open;
pub mod pr;
pub mod sync;

use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;

use crate::config::{AskField, HookConfig, HookWhen, RepoCommand, RepoConfig};
use crate::template::render_template;
use crate::vlog;

// ── Hook execution ────────────────────────────────────────────────────────────

/// Context available to hooks when their env-var templates are rendered.
pub struct HookContext<'a> {
    /// The fi subcommand being run: "new", "open", "pr", "cull", "sync".
    pub command: &'a str,
    pub repo: &'a RepoConfig,
    /// Newly created / selected branch name, if known.
    pub branch_name: Option<&'a str>,
    /// Path to the branch's worktree (or repo root for standard repos), if known.
    pub branch_path: Option<&'a str>,
}

/// Collect hooks from both the global config and the per-repo config, deduplicating by identity.
pub fn merged_hooks<'a>(
    global: Option<&'a Vec<HookConfig>>,
    repo: Option<&'a Vec<HookConfig>>,
) -> Vec<&'a HookConfig> {
    let mut out: Vec<&'a HookConfig> = Vec::new();
    if let Some(g) = global {
        out.extend(g.iter());
    }
    if let Some(r) = repo {
        out.extend(r.iter());
    }
    out
}

/// Run all hooks that match `command` + `when`, prompting for optional ones.
pub fn run_hooks_for(
    hooks: &[&HookConfig],
    when: HookWhen,
    ctx: &HookContext<'_>,
    dry_run: bool,
) -> Result<()> {
    let decisions = prompt_hook_confirmations(hooks, when.clone(), ctx, dry_run)?;
    execute_hook_decisions(&decisions, ctx, dry_run)
}

/// Phase 1: ask the user about optional hooks upfront (before a command like `open` steals focus).
/// Returns a list of (hook, should_run) pairs.
pub fn prompt_hook_confirmations<'a>(
    hooks: &[&'a HookConfig],
    when: HookWhen,
    ctx: &HookContext<'_>,
    dry_run: bool,
) -> Result<Vec<(&'a HookConfig, bool)>> {
    let mut decisions = Vec::new();
    for hook in hooks {
        if !hook.trigger.matches(ctx.command) {
            continue;
        }
        if hook.when != when {
            continue;
        }
        let display = hook.name.as_deref().unwrap_or("hook");
        let should_run = if hook.optional {
            if dry_run {
                println!(
                    "{} Would prompt: run {}? (default: {})",
                    "[dry-run]".yellow(),
                    display,
                    if hook.default_on { "yes" } else { "no" }
                );
                true
            } else {
                inquire::Confirm::new(&format!("Run {}?", display))
                    .with_default(hook.default_on)
                    .prompt()?
            }
        } else {
            true
        };
        decisions.push((*hook, should_run));
    }
    Ok(decisions)
}

/// Phase 2: execute hooks using decisions already collected by `prompt_hook_confirmations`.
pub fn execute_hook_decisions(
    decisions: &[(&HookConfig, bool)],
    ctx: &HookContext<'_>,
    dry_run: bool,
) -> Result<()> {
    for (hook, should_run) in decisions {
        let display = hook.name.as_deref().unwrap_or("hook");
        if !should_run {
            vlog!("hook '{}' skipped by user", display);
            continue;
        }

        let ask_vals = collect_ask_values(hook.ask.as_ref())?;
        let env_vars = resolve_hook_env(hook, ctx, &ask_vals);

        if dry_run {
            println!(
                "{} Would run {} hook '{}' via {} with env:",
                "[dry-run]".yellow(),
                format!("{:?}", hook.when).to_lowercase(),
                display,
                hook.runner
            );
            for (k, v) in &env_vars {
                println!("    {}={}", k.cyan(), v);
            }
            continue;
        }

        vlog!(
            "running {} hook '{}' via {}",
            format!("{:?}", hook.when).to_lowercase(),
            display,
            hook.runner
        );

        let bin = hook
            .runner
            .split_whitespace()
            .next()
            .ok_or_else(|| anyhow::anyhow!("hook runner is empty"))?;

        let tmp = write_temp_script(&hook.run)?;
        let status = std::process::Command::new(bin)
            .arg(&tmp)
            .envs(&env_vars)
            .status()
            .map_err(|e| anyhow::anyhow!("Failed to spawn '{}': {}", bin, e));
        std::fs::remove_file(&tmp).ok();

        let status = status?;
        if !status.success() {
            anyhow::bail!("Hook '{}' exited with {}", display, status);
        }
    }

    Ok(())
}

fn resolve_hook_env(
    hook: &HookConfig,
    ctx: &HookContext<'_>,
    ask_vals: &HashMap<String, String>,
) -> HashMap<String, String> {
    use crate::config::expand_tilde;

    let Some(env) = &hook.env else {
        return HashMap::new();
    };

    let mut vars: HashMap<String, String> = HashMap::new();
    vars.insert(
        "branch.name".into(),
        ctx.branch_name.unwrap_or("").to_string(),
    );
    vars.insert(
        "branch.path".into(),
        ctx.branch_path.unwrap_or("").to_string(),
    );
    let root = expand_tilde(&ctx.repo.root).to_string_lossy().to_string();
    vars.insert("repo.root".into(), root);
    vars.insert("repo.name".into(), ctx.repo.name.clone());
    vars.insert("repo.remote".into(), ctx.repo.remote().to_string());
    for (k, v) in ask_vals {
        vars.insert(format!("ask.{k}"), v.clone());
    }

    env.iter()
        .map(|(key, template)| (key.clone(), render_template(template, &vars)))
        .collect()
}

pub fn run_repo_cmd(cmd: &RepoCommand, branch_path: &str, dry_run: bool) -> Result<()> {
    let ask_vals = collect_ask_values(cmd.ask.as_ref())?;
    let env_vars = resolve_env(cmd, branch_path, &ask_vals);

    if dry_run {
        println!(
            "{} Would run '{}' via {} with env:",
            "[dry-run]".yellow(),
            cmd.command,
            cmd.runner
        );
        for (k, v) in &env_vars {
            println!("    {}={}", k.cyan(), v);
        }
        return Ok(());
    }

    let bin = cmd
        .runner
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow::anyhow!("runner is empty"))?;

    let tmp = write_temp_script(&cmd.run)?;
    let status = std::process::Command::new(bin)
        .arg(&tmp)
        .envs(&env_vars)
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to spawn '{}': {}", bin, e));
    std::fs::remove_file(&tmp).ok();

    let status = status?;
    if !status.success() {
        anyhow::bail!("Command '{}' exited with {}", cmd.command, status);
    }

    Ok(())
}

pub(crate) fn collect_ask_values(
    ask: Option<&HashMap<String, AskField>>,
) -> Result<HashMap<String, String>> {
    let mut vals = HashMap::new();
    let Some(ask_map) = ask else {
        return Ok(vals);
    };

    for (key, field) in ask_map {
        let val = match field.field_type() {
            "boolean" => {
                let ans = inquire::Confirm::new(&format!("{}?", key))
                    .with_default(false)
                    .prompt()?;
                (if ans { "true" } else { "false" }).to_string()
            }
            "editor" => {
                if field.is_optional() {
                    inquire::Editor::new(key)
                        .prompt_skippable()?
                        .unwrap_or_default()
                } else {
                    inquire::Editor::new(key).prompt()?
                }
            }
            _ => inquire::Text::new(&format!("{}:", key)).prompt()?,
        };
        vals.insert(key.clone(), val);
    }

    Ok(vals)
}

fn resolve_env(
    cmd: &RepoCommand,
    branch_path: &str,
    ask_vals: &HashMap<String, String>,
) -> HashMap<String, String> {
    let Some(env) = &cmd.env else {
        return HashMap::new();
    };

    let mut vars: HashMap<String, String> = HashMap::new();
    vars.insert("branch.path".into(), branch_path.to_string());
    for (k, v) in ask_vals {
        vars.insert(format!("ask.{k}"), v.clone());
    }

    env.iter()
        .map(|(key, template)| (key.clone(), render_template(template, &vars)))
        .collect()
}

fn write_temp_script(content: &str) -> Result<std::path::PathBuf> {
    use std::io::Write;
    let path = std::env::temp_dir().join(format!("fi_script_{}.sh", std::process::id()));
    let mut file = std::fs::File::create(&path)?;
    file.write_all(unescape(content).as_bytes())?;
    Ok(path)
}

/// Convert backslash escape sequences preserved literally by YAML block scalars
/// into their actual byte values (\n → newline, \t → tab, \r → CR, \\ → \).
pub(crate) fn unescape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            result.push(c);
            continue;
        }
        match chars.peek() {
            Some(&'n') => {
                chars.next();
                result.push('\n');
            }
            Some(&'r') => {
                chars.next();
                result.push('\r');
            }
            Some(&'t') => {
                chars.next();
                result.push('\t');
            }
            Some(&'\\') => {
                chars.next();
                result.push('\\');
            }
            _ => result.push('\\'),
        }
    }
    result
}
