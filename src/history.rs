use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::config::cache_dir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Script name (without .sh)
    pub name: String,
    /// Full path at time of run
    pub path: String,
    /// Unix timestamp (seconds)
    pub timestamp: u64,
    /// Number of times run total (denormalised for quick display)
    pub run_count: u32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct HistoryFile {
    entries: Vec<HistoryEntry>,
}

fn history_path() -> PathBuf {
    cache_dir().join("history.json")
}

fn load_raw() -> HistoryFile {
    let path = history_path();
    let Ok(content) = fs::read_to_string(&path) else {
        return HistoryFile::default();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

fn save_raw(hf: &HistoryFile) {
    let path = history_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(hf) {
        let _ = fs::write(path, json);
    }
}

/// Record a run. Bumps run_count if already present, inserts otherwise.
/// Trims to `limit` most-recent entries.
pub fn record(name: &str, path: &str, limit: usize) {
    let mut hf = load_raw();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if let Some(existing) = hf.entries.iter_mut().find(|e| e.name == name) {
        existing.timestamp = now;
        existing.run_count += 1;
        existing.path = path.to_owned();
    } else {
        hf.entries.push(HistoryEntry {
            name: name.to_owned(),
            path: path.to_owned(),
            timestamp: now,
            run_count: 1,
        });
    }

    // Most recent first
    hf.entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    hf.entries.truncate(limit);

    save_raw(&hf);
}

/// Load history entries, most-recent first.
pub fn load() -> Vec<HistoryEntry> {
    load_raw().entries
}

/// Clear all history.
pub fn clear() {
    save_raw(&HistoryFile::default());
}

/// Format a unix timestamp as a human-readable relative string.
pub fn relative_time(ts: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let diff = now.saturating_sub(ts);

    if diff < 60 {
        "just now".into()
    } else if diff < 3600 {
        let m = diff / 60;
        format!("{m}m ago")
    } else if diff < 86400 {
        let h = diff / 3600;
        format!("{h}h ago")
    } else if diff < 86400 * 7 {
        let d = diff / 86400;
        format!("{d}d ago")
    } else if diff < 86400 * 30 {
        let w = diff / (86400 * 7);
        format!("{w}w ago")
    } else {
        let mo = diff / (86400 * 30);
        format!("{mo}mo ago")
    }
}