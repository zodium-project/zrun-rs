# zrun

A fast, polished TUI shell-script launcher written in Rust.  
No external runtime deps — just a single static binary.

```
┌─ zrun v1.0.0 ────────────────────── Scripts (4/12) ──┐
│ ▶ backup_home                 /etc/zrun-scripts      │
│   deploy_nginx                                       │
│   fix_perms                                          │
│   ...                                                │
├─ Preview ────────────────────────────────────────────┤
│   1  #!/usr/bin/env bash                             │
│   2  # @tags: backup, cron                           │
│   3  # Backs up home directory to /mnt/backup        │
│   4                                                  │
│   5  set -euo pipefail                               │
│   ...                                                │
├──────────────────────────────────────────────────────┤
│ ↑↓/jk navigate  enter/r run  e edit  / search  q quit│
└──────────────────────────────────────────────────────┘
```

## Features

- **Zero external runtime deps** — no fzf, no bat, no glibc, no anything
- **Built-in fuzzy search** — `/` to search, highlights matches
- **Script tagging** — add `# @tags: deploy, infra` to scripts; filter by tag
- **Run history** — persisted in `~/.cache/zrun/history.json`
- **Shell syntax highlighting** in preview panel

## Script directories (priority order)

1. Dirs passed via `-d <path>` flags (CLI, highest priority)
2. `search_dirs` in `~/.config/zrun/config.toml`
3. `/etc/zrun-scripts` (fallback) (system-wide)
4. `/usr/lib/zrun-scripts` (fallback) (vendor)

When the same filename exists in multiple dirs, the highest-priority dir wins.

## Tagging scripts

Add a `# @tags:` line anywhere in the first 40 lines of a script:

```bash
#!/usr/bin/env bash
# @tags: backup, cron, important
# Backs up home directory to /mnt/backup
...
```

Single-tag variant also works: `# @tag: backup`

## Config file

`~/.config/zrun/config.toml`:
`( config is not automatically created )`

```toml
search_dirs   = ["~/.local/share/zrun/scripts", "/etc/zrun-scripts"]
history_limit = 100
clear_on_run  = true
```

## Key bindings

### Scripts tab
| Key | Action |
|-----|--------|
| `↑↓` / `j k` | Navigate list |
| `Enter` / `r` | Run selected script |
| `e` | Open in `$EDITOR` |
| `/` | Fuzzy search |
| `Esc` | Clear search / tag filter |
| `t` | Switch to Tags tab |
| `y` | Copy path to clipboard |
| `g` / `G` | Jump to top / bottom |
| `Ctrl-u/d` | Scroll preview up/down |
| `Tab` | Switch tab |
| `q` / `Ctrl-c` | Quit |

### Tags tab
| Key | Action |
|-----|--------|
| `Enter` / `t` | Apply tag as filter |
| `T` | Clear tag filter |

## CLI

```
zrun [OPTIONS] [SUBCOMMAND]

SUBCOMMANDS:
  pick              Launch TUI picker (default)
  run   <name>      Run a script directly
  list  [--tag t]   List all scripts
  show  <name>      Print script contents
  edit  <name>      Open in $EDITOR
  which <name>      Print full path
  history           Show recent runs
  history --clear   Clear history
  tags              List all tags

OPTIONS:
  -d, --dir <DIR>   Add search dir (repeatable)
      --dry-run     Print command, don't execute
      --no-clear    Don't clear screen before running
  -h, --help
  -V, --version
```