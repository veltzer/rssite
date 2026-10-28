//! rssite: a static site generator that can say what it will write before
//! it writes it. See README.md for the contract and docs/ for the rest.

mod build_info;
mod builtin;
mod config;
mod content;
mod git;
mod i18n;
mod init;
mod markdown;
mod plan;
mod render;
mod serve;
mod shortcode;
mod site;
mod slug;
mod templates;
mod util;

use anyhow::Result;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "rssite",
    version,
    about = "Static site generator with a pre-declared output manifest",
    arg_required_else_help = true,
    disable_help_subcommand = true
)]
struct Cli {
    /// The site root: the directory holding rssite.toml
    #[arg(short = 'C', long, global = true, default_value = ".")]
    root: PathBuf,

    /// Include pages and sections marked draft = true
    #[arg(long, global = true)]
    drafts: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Print the manifest of every file a build would write, with the sources of each
    Plan,
    /// Build the site into the output directory, writing only what changed
    Build {
        /// Also print the manifest, as `plan` would
        #[arg(long)]
        plan: bool,
        /// Render everything twice and fail if any output differs
        #[arg(long)]
        verify_determinism: bool,
    },
    /// Render every page without writing anything: templates, links, translations
    Check,
    /// Print the parsed site (pages, sections, taxonomies) as JSON
    Query,
    /// Remove every file the plan lists from the output directory
    Clean,
    /// Build, then serve the output directory, rebuilding when a source changes
    Serve {
        /// Port on 127.0.0.1
        #[arg(long, default_value_t = 8000)]
        port: u16,
        /// Do not watch for changes
        #[arg(long)]
        no_watch: bool,
        /// Do not reload open pages in the browser after a rebuild
        #[arg(long)]
        no_reload: bool,
    },
    /// Write a minimal working site into a directory
    Init {
        /// Where to write it; defaults to the current directory
        dir: Option<PathBuf>,
    },
    /// Print version and build details
    Version,
}

/// The bare `rssite` (or `rssite --drafts`) lists the commands with a line
/// each and nothing else; the flags are in `--help`.
fn parse_cli() -> Cli {
    let command = Cli::command();
    match command.clone().try_get_matches() {
        Ok(matches) => Cli::from_arg_matches(&matches).expect("matches parse"),
        Err(error) => {
            use clap::error::ErrorKind;
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand | ErrorKind::MissingSubcommand
            ) {
                let width = command
                    .get_subcommands()
                    .map(|c| c.get_name().len())
                    .max()
                    .unwrap_or(0);
                eprintln!("rssite needs a command:");
                for sub in command.get_subcommands() {
                    eprintln!(
                        "  {:<width$}  {}",
                        sub.get_name(),
                        sub.get_about().map(|a| a.to_string()).unwrap_or_default()
                    );
                }
                eprintln!("Run 'rssite --help' for the options.");
                std::process::exit(2);
            }
            error.exit()
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Plan => render::print_plan(&cli.root, cli.drafts),
        Commands::Build {
            plan,
            verify_determinism,
        } => {
            let options = render::Options {
                print_plan: plan,
                verify_determinism,
                write: true,
            };
            let report = render::run(&cli.root, cli.drafts, &options)?;
            eprintln!(
                "built {} files: {} written, {} unchanged",
                report.outputs, report.written, report.unchanged
            );
            Ok(())
        }
        Commands::Check => {
            let options = render::Options {
                print_plan: false,
                verify_determinism: true,
                write: false,
            };
            let report = render::run(&cli.root, cli.drafts, &options)?;
            eprintln!(
                "checked {} files: every template, link and translation resolves",
                report.outputs
            );
            Ok(())
        }
        Commands::Query => render::query(&cli.root, cli.drafts),
        Commands::Clean => {
            let removed = render::clean(&cli.root, cli.drafts)?;
            eprintln!("removed {removed} files");
            Ok(())
        }
        Commands::Serve {
            port,
            no_watch,
            no_reload,
        } => serve::serve(
            &cli.root,
            cli.drafts,
            port,
            !no_watch,
            !no_watch && !no_reload,
        ),
        Commands::Init { dir } => {
            let dir = dir.unwrap_or_else(|| cli.root.clone());
            let written = init::init(&dir)?;
            eprintln!(
                "wrote {written} files into {}; run `rssite build` there",
                dir.display()
            );
            Ok(())
        }
        Commands::Version => {
            println!(
                "rssite {} by {}",
                env!("CARGO_PKG_VERSION"),
                env!("CARGO_PKG_AUTHORS")
            );
            println!("GIT_DESCRIBE: {}", env!("GIT_DESCRIBE"));
            println!("GIT_SHA: {}", env!("GIT_SHA"));
            println!("GIT_BRANCH: {}", env!("GIT_BRANCH"));
            println!("GIT_DIRTY: {}", env!("GIT_DIRTY"));
            println!("RUSTC_SEMVER: {}", env!("RUSTC_SEMVER"));
            println!("RUST_EDITION: {}", env!("RUST_EDITION"));
            println!("BUILD_TIMESTAMP: {}", env!("BUILD_TIMESTAMP"));
            Ok(())
        }
    }
}

fn main() {
    let cli = parse_cli();
    if let Err(error) = run(cli) {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
