mod cli;
mod config;
mod scripts;
mod fuzzy;
mod history;
mod app;

use std::{
    io,
    process,
};

use clap::Parser;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use cli::{Cli, Command};
use config::Config;

fn main() {
    if let Err(e) = run_inner() {
        eprintln!("\x1b[31m⦻\x1b[0m  {e}");
        process::exit(1);
    }
}

fn run_inner() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Build config from CLI flags
    let extra_dirs = cli.dirs.unwrap_or_default();
    let config = Config::load(extra_dirs, cli.dry_run, cli.no_clear);

    // Dispatch non-TUI commands first (no terminal needed)
    match &cli.command {
        // ── list ──
        Some(Command::List { tag }) => {
            let scripts = scripts::collect(&config);
            let filtered: Vec<_> = if let Some(t) = tag {
                scripts.iter().filter(|s| s.tags.iter().any(|st| st == t)).collect()
            } else {
                scripts.iter().collect()
            };
            cmd_list(&filtered);
            return Ok(());
        }

        // ── show ──
        Some(Command::Show { name }) => {
            let scripts = scripts::collect(&config);
            let script = scripts::find_by_name(&scripts, name)
                .ok_or_else(|| format!("Script not found: {name}"))?;
            println!("{}", script.contents());
            return Ok(());
        }

        // ── which ──
        Some(Command::Which { name }) => {
            let scripts = scripts::collect(&config);
            let script = scripts::find_by_name(&scripts, name)
                .ok_or_else(|| format!("Script not found: {name}"))?;
            println!("{}", script.path.display());
            return Ok(());
        }

        // ── edit ──
        Some(Command::Edit { name }) => {
            let scripts = scripts::collect(&config);
            let script = scripts::find_by_name(&scripts, name)
                .ok_or_else(|| format!("Script not found: {name}"))?;
            cmd_edit(&script.path.to_string_lossy())?;
            return Ok(());
        }

        // ── history ──
        Some(Command::History { clear }) => {
            if *clear {
                history::clear();
                println!("History cleared.");
            } else {
                cmd_history();
            }
            return Ok(());
        }

        // ── tags ──
        Some(Command::Tags) => {
            let scripts = scripts::collect(&config);
            let tags = scripts::all_tags(&scripts);
            if tags.is_empty() {
                println!("No tags found.");
                println!("Add  # @tags: foo, bar  to your scripts.");
            } else {
                for tag in &tags {
                    let count = scripts.iter().filter(|s| s.tags.contains(tag)).count();
                    println!("  \x1b[34m#{tag}\x1b[0m  ({count} script{})", if count == 1 { "" } else { "s" });
                }
            }
            return Ok(());
        }

        // ── run (direct) ──
        Some(Command::Run { name }) => {
            let scripts = scripts::collect(&config);
            let script = scripts::find_by_name(&scripts, name)
                .ok_or_else(|| format!("Script not found: {name}"))?;
            execute_script(
                &script.path.to_string_lossy(),
                &script.name,
                &config,
            )?;
            return Ok(());
        }

        // ── pick / no subcommand → TUI ──
        Some(Command::Choose) | None => {}
    }

    // ── bare positional: zrun "script-name" ──
    if let Some(ref name) = cli.script {
        let scripts = scripts::collect(&config);
        let script = scripts::find_by_name(&scripts, name)
            .ok_or_else(|| format!("Script not found: {name}"))?;
        execute_script(
            &script.path.to_string_lossy(),
            &script.name,
            &config,
        )?;
        return Ok(());
    }

    // ── TUI path ──────────────────────────────────────────────
    let all_scripts = scripts::collect(&config);

    if all_scripts.is_empty() {
        eprintln!("\x1b[33m◇\x1b[0m  No scripts found in:");
        for d in &config.search_dirs {
            eprintln!("     {}", d.display());
        }
        eprintln!("\n  Create some .sh files or specify a dir with -d <path>");
        return Ok(());
    }

    let app = app::App::new(all_scripts, config.clone());

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = app::run_tui(&mut terminal, app);

    // Always restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    match result? {
        app::AppResult::RunScript { path, name } => {
            execute_script(&path, &name, &config)?;
        }
        app::AppResult::EditScript { path } => {
            cmd_edit(&path)?;
        }
        app::AppResult::Quit => {}
    }

    Ok(())
}

// ── Subcommand implementations ────────────────────────────────

fn cmd_list(scripts: &[&scripts::Script]) {
    println!();
    if scripts.is_empty() {
        println!("  \x1b[2mNo scripts found.\x1b[0m");
        return;
    }
    for s in scripts {
        let tags_str = if s.tags.is_empty() {
            String::new()
        } else {
            s.tags.iter().map(|t| format!("\x1b[34m#{t}\x1b[0m")).collect::<Vec<_>>().join(" ")
        };
        println!(
            "  \x1b[36m{:<30}\x1b[0m \x1b[2m{}\x1b[0m",
            s.name,
            s.path.display()
        );
        if !s.description.is_empty() || !s.tags.is_empty() {
            let sep = if !s.description.is_empty() && !s.tags.is_empty() { "  " } else { "" };
            println!(
                "  \x1b[2m  └ {}\x1b[0m{}",
                s.description,
                if !tags_str.is_empty() { format!("{sep}{tags_str}") } else { String::new() }
            );
        }
    }
    println!();
}

fn cmd_history() {
    let entries = history::load();
    if entries.is_empty() {
        println!("No history yet.");
        return;
    }
    println!();
    for e in &entries {
        println!(
            "  \x1b[36m{:<28}\x1b[0m  \x1b[35m✕ {:<4}\x1b[0m  \x1b[2m{}\x1b[0m",
            e.name,
            e.run_count,
            history::relative_time(e.timestamp),
        );
    }
    println!();
}

fn cmd_edit(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".into());

    let status = process::Command::new(&editor)
        .arg(path)
        .status()?;

    if !status.success() {
        eprintln!("\x1b[33m◇\x1b[0m  Editor exited with non-zero status");
    }
    Ok(())
}

fn execute_script(
    path: &str,
    name: &str,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if config.dry_run {
        println!("\x1b[36m◈\x1b[0m  DRY-RUN: would execute: bash {path}");
        return Ok(());
    }

    // Record history before running (so we capture even if script fails)
    history::record(name, path, config.history_limit);

    if config.clear_on_run {
        print!("\x1b[2J\x1b[H");
    }

    println!("\x1b[32m▶\x1b[0m  Running: \x1b[1m{name}\x1b[0m");
    println!("\x1b[2m   Path:   {path}\x1b[0m\n");

    // Use exec-style replacement so the script inherits our process
    let err = exec_bash(path);
    // Only reached if exec fails
    Err(format!("Failed to exec bash: {err}").into())
}

#[cfg(unix)]
fn exec_bash(path: &str) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    let mut cmd = process::Command::new("bash");
    cmd.arg(path);
    cmd.exec()
}

#[cfg(not(unix))]
fn exec_bash(path: &str) -> std::io::Error {
    // Fallback for non-Unix: just run and wait
    match process::Command::new("bash").arg(path).status() {
        Ok(_) => std::io::Error::new(std::io::ErrorKind::Other, "done"),
        Err(e) => e,
    }
}