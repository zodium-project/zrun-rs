use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use crate::config::Config;

/// A discovered shell script.
#[derive(Debug, Clone)]
pub struct Script {
    /// Filename without .sh extension, e.g. "backup_home"
    pub name: String,
    /// Full path on disk.
    pub path: PathBuf,
    /// First non-shebang comment line, used as description.
    pub description: String,
    /// Tags parsed from `# @tags: foo, bar` anywhere in the file.
    pub tags: Vec<String>,
    /// Info block: lines under `# @info:` or a `# @info` sentinel, up to 40 lines.
    pub info: String,
}

impl Script {
    /// Load metadata by reading the first ~40 lines of the file.
    fn from_path(path: PathBuf) -> Option<Self> {
        let name = path
            .file_stem()?
            .to_string_lossy()
            .into_owned();

        let content = std::fs::read_to_string(&path).ok()?;
        let (description, tags, info) = parse_header(&content);

        Some(Script {
            name,
            path,
            description,
            tags,
            info,
        })
    }

    /// Name formatted for display: hyphens → spaces.
    pub fn display_name(&self) -> String {
        self.name.replace('-', " ")
    }

    /// Full source as a string (for preview / show).
    pub fn contents(&self) -> String {
        std::fs::read_to_string(&self.path).unwrap_or_else(|e| format!("(error reading file: {e})"))
    }
}

// ── Header parsing ────────────────────────────────────────────

/// Scan up to 40 lines for:
///   - First non-shebang `# …` line  → description
///   - `# @tags: foo, bar`           → tags vec
///   - `# @tag: foo`                 → single tag
///   - `# @info: single line`        → info (single-line form)
///   - `# @info` followed by `# …`   → info (multi-line block until blank/non-comment)
fn parse_header(content: &str) -> (String, Vec<String>, String) {
    let mut description = String::new();
    let mut tags: Vec<String> = Vec::new();
    let mut info_lines: Vec<String> = Vec::new();
    let mut in_info_block = false;

    for line in content.lines().take(40) {
        let trimmed = line.trim();

        if trimmed.starts_with("#!") {
            in_info_block = false;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix('#') {
            let rest = rest.trim();

            // @tags / @tag directive
            if let Some(tag_str) = rest.strip_prefix("@tags:").or_else(|| rest.strip_prefix("@tag:")) {
                in_info_block = false;
                for t in tag_str.split(',') {
                    let t = t.trim().to_lowercase();
                    if !t.is_empty() {
                        tags.push(t);
                    }
                }
                continue;
            }

            // @info: single-line form  →  `# @info: some text`
            if let Some(inline) = rest.strip_prefix("@info:") {
                in_info_block = false;
                let text = inline.trim();
                if !text.is_empty() {
                    info_lines.push(text.to_owned());
                }
                continue;
            }

            // @info sentinel  →  `# @info` (block start)
            if rest == "@info" {
                in_info_block = true;
                continue;
            }

            // Inside an @info block: accumulate continuation comment lines
            if in_info_block {
                info_lines.push(rest.to_owned());
                continue;
            }

            // First plain comment → description
            if description.is_empty() && !rest.is_empty() && !rest.starts_with('@') {
                description = rest.to_owned();
            }
        } else {
            // Non-comment line ends any open @info block
            in_info_block = false;
        }
    }

    let info = info_lines.join("\n");
    (description, tags, info)
}

// ── Discovery ─────────────────────────────────────────────────

/// Collect all scripts from all search dirs, highest-priority first.
/// First occurrence of a filename wins (shadowing).
pub fn collect(config: &Config) -> Vec<Script> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut scripts: Vec<Script> = Vec::new();

    for dir in &config.search_dirs {
        if !dir.is_dir() {
            continue;
        }
        let mut entries = match read_dir_scripts(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        // Sort within the dir for stable ordering
        entries.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

        for entry in entries {
            let name = entry
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();

            if seen.contains(&name) {
                continue; // higher-priority dir already provided this script
            }
            seen.insert(name.clone());

            if let Some(script) = Script::from_path(entry) {
                scripts.push(script);
            }
        }
    }

    // Final alphabetical sort across all dirs
    scripts.sort_by(|a, b| a.name.cmp(&b.name));
    scripts
}

fn read_dir_scripts(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file()
            && path.extension().map(|e| e == "sh").unwrap_or(false)
        {
            out.push(path);
        }
    }
    Ok(out)
}

// ── Lookup by name ────────────────────────────────────────────

pub fn find_by_name<'a>(scripts: &'a [Script], name: &str) -> Option<&'a Script> {
    // Accept with or without .sh suffix
    let bare = name.strip_suffix(".sh").unwrap_or(name);
    scripts.iter().find(|s| s.name == bare)
}

// ── All unique tags ────────────────────────────────────────────

pub fn all_tags(scripts: &[Script]) -> Vec<String> {
    let mut set: HashSet<String> = HashSet::new();
    for s in scripts {
        for t in &s.tags {
            set.insert(t.clone());
        }
    }
    let mut v: Vec<_> = set.into_iter().collect();
    v.sort();
    v
}