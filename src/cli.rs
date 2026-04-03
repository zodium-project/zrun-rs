use std::path::PathBuf;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name    = "zrun",
    about   = "A fast TUI script launcher",
    long_about = "A fast , polished & memory-safe TUI script launcher",
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Run a script directly by name (shorthand: zrun "script-name").
    #[arg(value_name = "SCRIPT", conflicts_with = "command")]
    pub script: Option<String>,

    /// Add an extra script search directory (repeatable, highest priority first).
    #[arg(short = 'd', long = "dir", value_name = "DIR", action = clap::ArgAction::Append)]
    pub dirs: Option<Vec<PathBuf>>,

    /// Show what would run without executing.
    #[arg(long = "dry-run", global = true)]
    pub dry_run: bool,

    /// Don't clear the screen before running a script.
    #[arg(long = "no-clear", global = true)]
    pub no_clear: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Launch the interactive TUI picker (default when no subcommand given).
    #[command(name = "choose")]
    Choose,

    /// Run a script directly by name.
    Run {
        /// Script name (with or without .sh)
        name: String,
    },

    /// List all available scripts.
    List {
        /// Filter by tag
        #[arg(short, long)]
        tag: Option<String>,
    },

    /// Print a script's contents.
    Show {
        name: String,
    },

    /// Open a script in $EDITOR.
    Edit {
        name: String,
    },

    /// Print a script's full path.
    Which {
        name: String,
    },

    /// Show recent run history.
    History {
        /// Clear all history
        #[arg(long)]
        clear: bool,
    },

    /// List all tags found across scripts.
    Tags,
}