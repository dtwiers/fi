use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::generate;
use clap_complete::shells::{Bash, Fish, Zsh};
use clap_complete_nushell::Nushell;

mod commands;
mod config;
mod git;
mod jira;

#[derive(Parser)]
#[command(name = "fi", about = "Feature workflow tool", version)]
struct Cli {
    /// Path to a config file (overrides the default search path)
    #[arg(short, long)]
    config: Option<String>,
    /// Print the config path being loaded and other diagnostic info
    #[arg(short, long, global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(ValueEnum, Clone)]
enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    Nushell,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a starter config at ~/.config/fi/fi.yaml
    Init {
        /// Overwrite an existing config
        #[arg(long)]
        force: bool,
    },
    /// Create a new branch or worktree from a Jira ticket
    New {
        #[arg(short = 'n', long)]
        dry_run: bool,
        #[arg(short, long, help = "Skip issue picker and use this ticket key")]
        ticket: Option<String>,
    },
    /// Clean up (delete) selected worktrees
    Cull {
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// Create pull requests for the current worktree/branch
    Pr {
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// Open an existing worktree
    Open {
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// List all repos and their current branches / worktrees
    List,
    /// Inspect or edit the config file
    Config {
        #[command(subcommand)]
        sub: commands::config::ConfigSubcommand,
    },
    /// Generate shell completion scripts
    Completions {
        /// Shell to generate completions for
        shell: CompletionShell,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Commands::Completions { shell } = cli.command {
        let mut cmd = Cli::command();
        let name = cmd.get_name().to_string();
        match shell {
            CompletionShell::Bash => generate(Bash, &mut cmd, &name, &mut std::io::stdout()),
            CompletionShell::Zsh => generate(Zsh, &mut cmd, &name, &mut std::io::stdout()),
            CompletionShell::Fish => generate(Fish, &mut cmd, &name, &mut std::io::stdout()),
            CompletionShell::Nushell => generate(Nushell, &mut cmd, &name, &mut std::io::stdout()),
        }
        return Ok(());
    }

    if let Commands::Init { force } = cli.command {
        return commands::init::run(force);
    }

    // fi config subcommands don't need a loaded config (validate/path/show/edit
    // all handle the "no config yet" case themselves)
    if let Commands::Config { ref sub } = cli.command {
        return commands::config::run(sub, cli.config.as_deref()).await;
    }

    let config_path = config::find_config_path(cli.config.as_deref())?;
    if cli.verbose {
        eprintln!("fi: loading config from {}", config_path.display());
    }
    let config = config::find_config(cli.config.as_deref())?;

    match cli.command {
        Commands::New { dry_run, ticket } => {
            commands::new::run(&config, dry_run, ticket.as_deref()).await
        }
        Commands::Cull { dry_run } => commands::cull::run(&config, dry_run).await,
        Commands::Pr { dry_run } => commands::pr::run(&config, dry_run).await,
        Commands::Open { dry_run } => commands::open::run(&config, dry_run).await,
        Commands::List => commands::list::run(&config).await,
        Commands::Completions { .. } | Commands::Init { .. } | Commands::Config { .. } => {
            unreachable!()
        }
    }
}
