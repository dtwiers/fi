use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::generate;
use clap_complete::shells::{Bash, Fish, Zsh};
use clap_complete_nushell::Nushell;

mod commands;
mod config;
mod git;
mod jira;
mod template;

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
        /// Resume PR creation after resolving merge conflicts in a conflict branch
        #[arg(long = "continue")]
        continue_mode: bool,
    },
    /// Open an existing worktree
    Open {
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// List all repos and their current branches / worktrees
    List,
    /// Sync conflict branches with the feature branch after new commits
    Sync {
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
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

// clap_complete Fish backend emits nested-subcommand conditions like:
//   -n "__fish_fi_using_subcommand config; and __fish_seen_subcommand_from validate"
// The semicolon inside the -n string is NOT treated as a statement separator
// in fish eval context, causing "Unknown command" errors on tab-completion.
// fix_fish_completions() post-processes the output to inject two correct helper
// functions and replace the broken conditions with calls to those helpers.
fn fix_fish_completions(raw: &str) -> String {
    let helpers = concat!(
        "\nfunction __fish_fi_config_needs_subcommand\n",
        "\tset -l cmd (commandline -opc)\n",
        "\tset -e cmd[1]\n",
        "\tset -l found 0\n",
        "\tfor tok in $cmd\n",
        "\t\tif test $found -eq 1\n",
        "\t\t\tcontains -- $tok validate show path edit help; and return 1\n",
        "\t\tend\n",
        "\t\tif test $tok = config\n",
        "\t\t\tset found 1\n",
        "\t\tend\n",
        "\tend\n",
        "\ttest $found -eq 1\n",
        "end\n",
        "\nfunction __fish_fi_using_config_subcommand\n",
        "\tset -l cmd (commandline -opc)\n",
        "\tset -e cmd[1]\n",
        "\tset -l after_config 0\n",
        "\tfor tok in $cmd\n",
        "\t\tif test $after_config -eq 1\n",
        "\t\t\tcontains -- $tok $argv; and return 0\n",
        "\t\t\treturn 1\n",
        "\t\tend\n",
        "\t\tif test $tok = config\n",
        "\t\t\tset after_config 1\n",
        "\t\tend\n",
        "\tend\n",
        "\treturn 1\n",
        "end\n",
        "\nfunction __fish_fi_help_needs_subcommand\n",
        "\tset -l cmd (commandline -opc)\n",
        "\tset -e cmd[1]\n",
        "\tset -l found 0\n",
        "\tfor tok in $cmd\n",
        "\t\tif test $found -eq 1\n",
        "\t\t\tcontains -- $tok init new cull pr open list config sync completions help; and return 1\n",
        "\t\tend\n",
        "\t\tif test $tok = help\n",
        "\t\t\tset found 1\n",
        "\t\tend\n",
        "\tend\n",
        "\ttest $found -eq 1\n",
        "end\n",
        "\nfunction __fish_fi_using_help_subcommand\n",
        "\tset -l cmd (commandline -opc)\n",
        "\tset -e cmd[1]\n",
        "\tset -l after_help 0\n",
        "\tfor tok in $cmd\n",
        "\t\tif test $after_help -eq 1\n",
        "\t\t\tcontains -- $tok $argv; and return 0\n",
        "\t\t\treturn 1\n",
        "\t\tend\n",
        "\t\tif test $tok = help\n",
        "\t\t\tset after_help 1\n",
        "\t\tend\n",
        "\tend\n",
        "\treturn 1\n",
        "end\n",
    );

    let injection = raw.find("\ncomplete -c fi").unwrap_or(raw.len());
    let (before, after) = raw.split_at(injection);
    let with_helpers = format!("{}{}{}", before, helpers, after);

    // Fix "config" nested-subcommand guards.
    let fixed = with_helpers.replace(
        "__fish_fi_using_subcommand config; and not __fish_seen_subcommand_from validate show path edit help",
        "__fish_fi_config_needs_subcommand",
    );
    let mut fixed = fixed;
    for sub in ["validate", "show", "path", "edit", "help"] {
        let broken = format!(
            "__fish_fi_using_subcommand config; and __fish_seen_subcommand_from {}",
            sub
        );
        fixed = fixed.replace(
            &broken,
            &format!("__fish_fi_using_config_subcommand {}", sub),
        );
    }

    // Fix "help" subcommand guards (same semicolon problem).
    let all_subs = "init new cull pr open list config sync completions help";
    fixed = fixed.replace(
        &format!(
            "__fish_fi_using_subcommand help; and not __fish_seen_subcommand_from {}",
            all_subs
        ),
        "__fish_fi_help_needs_subcommand",
    );
    fixed = fixed.replace(
        "__fish_fi_using_subcommand help; and __fish_seen_subcommand_from config",
        "__fish_fi_using_help_subcommand config",
    );

    fixed
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
            CompletionShell::Fish => {
                let mut buf = Vec::new();
                generate(Fish, &mut cmd, &name, &mut buf);
                let raw = String::from_utf8_lossy(&buf);
                print!("{}", fix_fish_completions(&raw));
            }
            CompletionShell::Nushell => generate(Nushell, &mut cmd, &name, &mut std::io::stdout()),
        }
        return Ok(());
    }

    if let Commands::Init { force } = cli.command {
        return commands::init::run(force);
    }

    // fi config subcommands do not need a loaded config -- validate/path/show/edit
    // all handle the "no config yet" case themselves.
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
        Commands::Pr {
            dry_run,
            continue_mode,
        } => commands::pr::run(&config, dry_run, continue_mode).await,
        Commands::Open { dry_run } => commands::open::run(&config, dry_run).await,
        Commands::List => commands::list::run(&config).await,
        Commands::Sync { dry_run } => commands::sync::run(&config, dry_run).await,
        Commands::Completions { .. } | Commands::Init { .. } | Commands::Config { .. } => {
            unreachable!()
        }
    }
}
