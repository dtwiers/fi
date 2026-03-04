use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use std::path::PathBuf;

use crate::config;

#[derive(Subcommand)]
pub enum ConfigSubcommand {
    /// Check the config file for errors
    Validate,
    /// Print the current config (Jira token masked)
    Show,
    /// Print the path to the active config file
    Path,
    /// Open the config file in $EDITOR
    Edit,
}

pub async fn run(sub: &ConfigSubcommand, config_override: Option<&str>) -> Result<()> {
    match sub {
        ConfigSubcommand::Validate => validate(config_override),
        ConfigSubcommand::Show => show(config_override),
        ConfigSubcommand::Path => path(config_override),
        ConfigSubcommand::Edit => edit(config_override),
    }
}

fn validate(config_override: Option<&str>) -> Result<()> {
    match resolve_config_path(config_override) {
        Ok(path) => match config::find_config(Some(path.to_str().unwrap_or(""))) {
            Ok(_) => {
                println!(
                    "{}  Config OK  ({})",
                    "\u{2713}".green().bold(),
                    path.display().to_string().cyan()
                );
            }
            Err(e) => {
                eprintln!("{}", format!("\u{2717}  Config error: {}", e).red().bold());
                eprintln!(
                    "   Hint: run {} to create a starter config.",
                    "'fi init'".cyan()
                );
            }
        },
        Err(e) => {
            eprintln!("{}", format!("\u{2717}  Config error: {}", e).red().bold());
            eprintln!(
                "   Hint: run {} to create a starter config.",
                "'fi init'".cyan()
            );
        }
    }
    Ok(())
}

fn show(config_override: Option<&str>) -> Result<()> {
    let path = resolve_config_path(config_override)?;
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;

    println!("{}", format!("# config: {}", path.display()).dimmed());
    print!("{}", raw);
    Ok(())
}

fn path(config_override: Option<&str>) -> Result<()> {
    let path = resolve_config_path(config_override)?;
    println!("{}", path.display());
    Ok(())
}

fn edit(config_override: Option<&str>) -> Result<()> {
    let path = resolve_config_path(config_override)?;
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());

    let status = std::process::Command::new(&editor)
        .arg(&path)
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to launch editor '{}': {}", editor, e))?;

    if !status.success() {
        anyhow::bail!("Editor exited with non-zero status");
    }
    Ok(())
}

fn resolve_config_path(override_path: Option<&str>) -> Result<PathBuf> {
    let paths: Vec<PathBuf> = match override_path {
        Some(p) => vec![PathBuf::from(p)],
        None => {
            let home = dirs::home_dir().unwrap_or_default();
            vec![
                PathBuf::from("fi.yaml"),
                home.join(".config/fi/fi.yaml"),
                home.join(".config/fi/fi.yml"),
            ]
        }
    };
    for path in &paths {
        if path.exists() {
            return Ok(path.clone());
        }
    }
    anyhow::bail!("No config file found. Tried: {:?}", paths)
}
