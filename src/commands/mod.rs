pub mod config;
pub mod cull;
pub mod init;
pub mod list;
pub mod new;
pub mod open;
pub mod pr;

use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;

use crate::config::{AskField, RepoCommand};

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

    env.iter()
        .map(|(key, template)| {
            let mut val = template.replace("{branch.path}", branch_path);
            for (ask_key, ask_val) in ask_vals {
                val = val.replace(&format!("{{ask.{}}}", ask_key), ask_val);
            }
            (key.clone(), val)
        })
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
            Some(&'n') => { chars.next(); result.push('\n'); }
            Some(&'r') => { chars.next(); result.push('\r'); }
            Some(&'t') => { chars.next(); result.push('\t'); }
            Some(&'\\') => { chars.next(); result.push('\\'); }
            _ => result.push('\\'),
        }
    }
    result
}

