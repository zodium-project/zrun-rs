use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// Resolved at startup — merges config file + CLI flags.
#[derive(Debug, Clone)]
pub struct Config {
    /// Script search dirs, highest-priority first.
    pub search_dirs: Vec<PathBuf>,
    pub history_limit: usize,
    pub clear_on_run: bool,
    pub dry_run: bool,
}

/// Raw TOML config file shape.
#[derive(Debug, Deserialize, Serialize, Default)]
struct RawConfig {
    search_dirs:   Option<Vec<String>>,
    history_limit: Option<usize>,
    clear_on_run:  Option<bool>,
}

// ── XDG helpers ───────────────────────────────────────────────

pub fn config_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let mut p = home_dir();
            p.push(".config");
            p
        })
        .join("zrun")
}

pub fn cache_dir() -> PathBuf {
    std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let mut p = home_dir();
            p.push(".cache");
            p
        })
        .join("zrun")
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

// ── Default system script dirs ────────────────────────────────

pub const DEFAULT_SEARCH_DIRS: &[&str] = &[
    "/etc/zrun-scripts",
    "/usr/lib/zrun-scripts",
];

// ── Config loading ────────────────────────────────────────────

impl Config {
    /// Load from file (if present) then apply CLI overrides.
    pub fn load(
        extra_dirs: Vec<PathBuf>,
        dry_run: bool,
        no_clear: bool,
    ) -> Self {
        let raw = Self::read_file().unwrap_or_default();

        // Build search dirs: CLI extras first, then config file, then defaults.
        let mut search_dirs: Vec<PathBuf> = extra_dirs;

        if let Some(dirs) = raw.search_dirs {
            for d in dirs {
                let p = PathBuf::from(shellexpand_tilde(&d));
                if !search_dirs.contains(&p) {
                    search_dirs.push(p);
                }
            }
        }

        for &d in DEFAULT_SEARCH_DIRS {
            let p = PathBuf::from(d);
            if !search_dirs.contains(&p) {
                search_dirs.push(p);
            }
        }

        Config {
            search_dirs,
            history_limit: raw.history_limit.unwrap_or(100),
            clear_on_run: !no_clear && raw.clear_on_run.unwrap_or(true),
            dry_run,
        }
    }

    fn read_file() -> Option<RawConfig> {
        let path = config_dir().join("config.toml");
        let content = std::fs::read_to_string(&path).ok()?;
        toml::from_str(&content).ok()
    }
}

/// Minimal tilde expansion — avoids pulling in extra deps.
fn shellexpand_tilde(s: &str) -> String {
    if s.starts_with("~/") || s == "~" {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        s.replacen('~', &home, 1)
    } else {
        s.to_owned()
    }
}