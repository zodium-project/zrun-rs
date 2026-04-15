use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        BorderType,
        Block, Borders, List, ListItem, ListState, Paragraph, Wrap,
    },
    Frame, Terminal,
};

use crate::{
    config::Config,
    fuzzy,
    history,
    scripts::{self, Script},
};

// ── Tabs ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Scripts,
    History,
    Tags,
}

// ── Input mode ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,
    Search,
}

// ── Public result ─────────────────────────────────────────────

pub enum AppResult {
    RunScript { path: String, name: String },
    EditScript { path: String },
    Quit,
}

// ── App state ─────────────────────────────────────────────────

pub struct App {
    all_scripts:    Vec<Script>,
    filtered:       Vec<(usize, i32)>,

    tab:            Tab,
    list_state:     ListState,
    preview_scroll: usize,

    mode:           InputMode,
    search_query:   String,

    tag_filter:     Option<String>,
    tag_list:       Vec<String>,
    tag_list_state: ListState,
    tag_pane_right: bool,         // false = left (tag list), true = right (scripts in tag)
    tag_scripts:        Vec<usize>,   // indices into all_scripts for hovered tag
    tag_scripts_state:  ListState,
    tag_scripts_map:    std::collections::HashMap<String, Vec<usize>>,

    history:        Vec<history::HistoryEntry>,
    history_state:  ListState,

    status_msg:     Option<(String, Instant)>,

    // (script_index, raw file contents) — only reloaded on selection change
    preview_cache:  Option<(usize, String)>,

    dry_run:        bool,
}

const STATUS_DURATION: Duration = Duration::from_secs(3);
const SCROLL_PAGE: usize = 10;
const SCROLL_HALF: usize = 5;

impl App {
    pub fn new(all_scripts: Vec<Script>, config: Config) -> Self {
        let tag_list = scripts::all_tags(&all_scripts);
        let history = history::load();
        let n       = all_scripts.len();

        let filtered: Vec<(usize, i32)> = (0..n).map(|i| (i, 0)).collect();

        let mut list_state = ListState::default();
        if n > 0 { list_state.select(Some(0)); }

        let mut history_state = ListState::default();
        if !history.is_empty() { history_state.select(Some(0)); }

        App {
            all_scripts,
            filtered,
            tab:            Tab::Scripts,
            list_state,
            preview_scroll: 0,
            mode:           InputMode::Normal,
            search_query:   String::new(),
            tag_filter:     None,
            tag_list,
            tag_list_state: ListState::default(),
            tag_pane_right: false,
            tag_scripts:        Vec::new(),
            tag_scripts_state:  ListState::default(),
            tag_scripts_map:    std::collections::HashMap::new(),
            history,
            history_state,
            status_msg:     None,
            preview_cache:  None,
            dry_run:        config.dry_run,
        }
    }

    // ── Filtering ─────────────────────────────────────────────

    fn refilter(&mut self) {
        let base_indices: Vec<usize> = if let Some(ref tag) = self.tag_filter {
            self.all_scripts
                .iter()
                .enumerate()
                .filter(|(_, s)| s.tags.iter().any(|t| t == tag))
                .map(|(i, _)| i)
                .collect()
        } else {
            (0..self.all_scripts.len()).collect()
        };

        let base_scripts: Vec<&Script> = base_indices
            .iter()
            .map(|&i| &self.all_scripts[i])
            .collect();

        let ranked = fuzzy::rank(&self.search_query, &base_scripts);

        self.filtered = ranked
            .into_iter()
            .map(|(local_i, score)| (base_indices[local_i], score))
            .collect();

        let sel = if self.filtered.is_empty() { None } else { Some(0) };
        self.list_state.select(sel);
        self.preview_scroll = 0;
        self.preview_cache  = None;
    }

    // ── Selection ─────────────────────────────────────────────

    fn selected_script_idx(&self) -> Option<usize> {
        let i = self.list_state.selected()?;
        self.filtered.get(i).map(|&(si, _)| si)
    }

    fn selected_script(&self) -> Option<&Script> {
        self.selected_script_idx()
            .and_then(|si| self.all_scripts.get(si))
    }

    // ── Preview cache ─────────────────────────────────────────

    fn warm_cache(&mut self) {
        let Some(si) = self.selected_script_idx() else { return };
        let stale = self.preview_cache
            .as_ref()
            .map(|(ci, _)| *ci != si)
            .unwrap_or(true);
        if stale {
            let contents = self.all_scripts[si].contents();
            self.preview_cache = Some((si, contents));
        }
    }

    // ── Input ─────────────────────────────────────────────────

    fn handle_key(&mut self, code: KeyCode, mods: KeyModifiers) -> Option<AppResult> {
        match self.mode {
            InputMode::Search => self.handle_search_key(code),
            InputMode::Normal => self.handle_normal_key(code, mods),
        }
    }

    fn handle_search_key(&mut self, code: KeyCode) -> Option<AppResult> {
        match code {
            KeyCode::Esc => {
                self.mode = InputMode::Normal;
                self.search_query.clear();
                self.refilter();
            }
            KeyCode::Char('/') => {
                // toggle off search mode, keep query
                self.mode = InputMode::Normal;
            }
            KeyCode::Enter => {
                self.mode = InputMode::Normal;
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                self.refilter();
            }
            KeyCode::Char(c) => {
                self.search_query.push(c);
                self.refilter();
            }
            KeyCode::Up   => self.move_sel(-1),
            KeyCode::Down => self.move_sel(1),
            _ => {}
        }
        None
    }

    fn handle_normal_key(&mut self, code: KeyCode, mods: KeyModifiers) -> Option<AppResult> {
        if (code == KeyCode::Char('c') && mods.contains(KeyModifiers::CONTROL))
            || code == KeyCode::Char('q')
        {
            return Some(AppResult::Quit);
        }

        match code {
            KeyCode::Tab | KeyCode::BackTab => {
                self.tab = match self.tab {
                    Tab::Scripts => Tab::History,
                    Tab::History => Tab::Tags,
                    Tab::Tags    => Tab::Scripts,
                };

                if self.tab == Tab::Tags
                    && self.tag_list_state.selected().is_none()
                    && !self.tag_list.is_empty()
                {
                    self.tag_list_state.select(Some(0));
                }

                self.preview_scroll = 0;
                if self.tab == Tab::Tags {
                    self.tag_pane_right = false;
                    self.refresh_tag_scripts();
                }
            }

            KeyCode::Up   | KeyCode::Char('k') => self.move_sel(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_sel(1),
            KeyCode::Left | KeyCode::Char('h') => {
                if self.tab == Tab::Tags && self.tag_pane_right {
                    self.tag_pane_right = false;
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if self.tab == Tab::Tags && !self.tag_pane_right && !self.tag_scripts.is_empty() {
                    self.tag_pane_right = true;
                    if self.tag_scripts_state.selected().is_none() {
                        self.tag_scripts_state.select(Some(0));
                    }
                }
            }
            KeyCode::Char('g') | KeyCode::Home => self.jump_sel(0),
            KeyCode::Char('G') | KeyCode::End  => {
                let last = self.current_list_len().saturating_sub(1);
                self.jump_sel(last);
            }

            KeyCode::PageUp => {
                self.preview_scroll = self.preview_scroll.saturating_sub(SCROLL_PAGE);
            }
            KeyCode::PageDown => {
                self.preview_scroll = self.preview_scroll.saturating_add(SCROLL_PAGE);
            }
            KeyCode::Char('u') if mods.contains(KeyModifiers::CONTROL) => {
                self.preview_scroll = self.preview_scroll.saturating_sub(SCROLL_HALF);
            }
            KeyCode::Char('d') if mods.contains(KeyModifiers::CONTROL) => {
                self.preview_scroll = self.preview_scroll.saturating_add(SCROLL_HALF);
            }

            KeyCode::Char('/') if self.tab == Tab::Scripts => {
                if self.mode == InputMode::Search {
                    self.mode = InputMode::Normal;
                } else {
                    self.mode = InputMode::Search;
                }
            }

            KeyCode::Esc => {
                let changed = !self.search_query.is_empty() || self.tag_filter.is_some();
                self.search_query.clear();
                self.tag_filter = None;
                if changed { self.refilter(); }
            }

            KeyCode::Enter | KeyCode::Char('r') => return self.action_run(),
            KeyCode::Char('e')                  => return self.action_edit(),

            KeyCode::Char('t') => {
                if self.tab == Tab::Scripts {
                    self.tab = Tab::Tags;
                    if self.tag_list_state.selected().is_none() && !self.tag_list.is_empty() {
                        self.tag_list_state.select(Some(0));
                    }
                    self.tag_pane_right = false;
                    self.refresh_tag_scripts();
                } else if self.tab == Tab::Tags {
                    self.apply_tag();
                }
            }

            KeyCode::Char('T') => {
                if self.tag_filter.is_some() {
                    self.tag_filter = None;
                    self.refilter();
                    self.set_status("Tag filter cleared".into());
                }
            }

            KeyCode::Char('y') => {
                if let Some(script) = self.selected_script() {
                    let path = script.path.to_string_lossy().to_string();
                    if try_copy_to_clipboard(&path) {
                        self.set_status(format!("Copied: {path}"));
                    } else {
                        self.set_status(format!("Path: {path}"));
                    }
                }
            }

            _ => {}
        }
        None
    }

    fn apply_tag(&mut self) {
        let tag = self.tag_list_state.selected()
            .and_then(|i| self.tag_list.get(i))
            .cloned();
        if let Some(tag) = tag {
            let msg = format!("Filtered by tag: {tag}");
            self.tag_filter = Some(tag);
            self.tab = Tab::Scripts;
            self.refilter();
            self.set_status(msg);
        }
    }

    fn action_run(&mut self) -> Option<AppResult> {
        match self.tab {
            Tab::Scripts => self.selected_script().map(|s| AppResult::RunScript {
                path: s.path.to_string_lossy().into_owned(),
                name: s.name.clone(),
            }),
            Tab::History => self.history_state.selected()
                .and_then(|i| self.history.get(i))
                .map(|e| {
                    let live = scripts::find_by_name(&self.all_scripts, &e.name);
                    AppResult::RunScript {
                        path: live.map(|s| s.path.to_string_lossy().into_owned())
                            .unwrap_or_else(|| e.path.clone()),
                        name: e.name.clone(),
                    }
                }),
            Tab::Tags => {
                if self.tag_pane_right {
                    // Launch selected script from right pane
                    self.tag_scripts_state.selected()
                        .and_then(|i| self.tag_scripts.get(i))
                        .and_then(|&si| self.all_scripts.get(si))
                        .map(|s| AppResult::RunScript {
                            path: s.path.to_string_lossy().into_owned(),
                            name: s.name.clone(),
                        })
                } else {
                    self.apply_tag();
                    None
                }
            }
        }
    }

    fn action_edit(&mut self) -> Option<AppResult> {
        self.selected_script().map(|s| AppResult::EditScript {
            path: s.path.to_string_lossy().into_owned(),
        })
    }

    // ── List movement ─────────────────────────────────────────

    fn current_list_len(&self) -> usize {
        match self.tab {
            Tab::Scripts => self.filtered.len(),
            Tab::History => self.history.len(),
            Tab::Tags    => if self.tag_pane_right { self.tag_scripts.len() } else { self.tag_list.len() },
        }
    }

    fn active_list_state_mut(&mut self) -> &mut ListState {
        match self.tab {
            Tab::Scripts => &mut self.list_state,
            Tab::History => &mut self.history_state,
            Tab::Tags    => if self.tag_pane_right { &mut self.tag_scripts_state } else { &mut self.tag_list_state },
        }
    }

    fn move_sel(&mut self, delta: i32) {
        let len = self.current_list_len();
        if len == 0 { return; }
        let cur  = self.active_list_state_mut().selected().unwrap_or(0) as i32;
        let next = (cur + delta).rem_euclid(len as i32) as usize;
        self.active_list_state_mut().select(Some(next));
        if self.tab == Tab::Scripts {
            self.preview_scroll = 0;
            self.preview_cache  = None;
        }
        if self.tab == Tab::Tags && !self.tag_pane_right {
            self.refresh_tag_scripts();
        }
    }

    fn jump_sel(&mut self, idx: usize) {
        let len = self.current_list_len();
        if len == 0 {
            self.active_list_state_mut().select(None);
        } else {
            self.active_list_state_mut().select(Some(idx.min(len - 1)));
        }

        self.preview_scroll = 0;
        if self.tab == Tab::Scripts {
            self.preview_cache = None;
        }
        if self.tab == Tab::Tags && !self.tag_pane_right {
            self.refresh_tag_scripts();
        }
    }

    fn set_status(&mut self, msg: String) {
        self.status_msg = Some((msg, Instant::now()));
    }

    fn refresh_tag_scripts(&mut self) {
        // Rebuild the full map if empty (first call or after reload)
        if self.tag_scripts_map.is_empty() {
            for (i, s) in self.all_scripts.iter().enumerate() {
                for t in &s.tags {
                    self.tag_scripts_map.entry(t.clone()).or_default().push(i);
                }
            }
        }

        let tag = self.tag_list_state.selected()
            .and_then(|i| self.tag_list.get(i))
            .cloned();

        self.tag_scripts = tag
            .and_then(|t| self.tag_scripts_map.get(&t).cloned())
            .unwrap_or_default();

        self.tag_scripts_state.select(if self.tag_scripts.is_empty() { None } else { Some(0) });
    }

    // ── Draw ──────────────────────────────────────────────────

    pub fn draw(&mut self, frame: &mut Frame) {
        // Expire status message
        if matches!(&self.status_msg, Some((_, t)) if t.elapsed() > STATUS_DURATION) {
            self.status_msg = None;
        }

        // Load preview file only when selection changed — O(0) if cached
        if self.tab == Tab::Scripts {
            self.warm_cache();
        }

        let area = frame.area();
        let root = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2), // header
                Constraint::Min(0),    // body
                Constraint::Length(1), // footer
            ])
            .split(area);

        self.draw_header(frame, root[0]);
        self.draw_body(frame, root[1]);
        self.draw_footer(frame, root[2]);
    }

    // ── Header ────────────────────────────────────────────────

    fn draw_header(&self, frame: &mut Frame, area: Rect) {
        let version = env!("CARGO_PKG_VERSION");

        let tab_style = |active: bool| {
            if active {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            }
        };

        let mut spans = vec![
            Span::styled(
                format!(" zrun v{version} "),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("[ Scripts {}/{} ]", self.filtered.len(), self.all_scripts.len()),
                tab_style(self.tab == Tab::Scripts),
            ),
            Span::raw(" "),
            Span::styled(
                format!("[ History {} ]", self.history.len()),
                tab_style(self.tab == Tab::History),
            ),
            Span::raw(" "),
            Span::styled(
                format!("[ Tags {} ]", self.tag_list.len()),
                tab_style(self.tab == Tab::Tags),
            ),
        ];

        if let Some(ref tag) = self.tag_filter {
            spans.push(Span::styled(
                format!("  ⬥ {tag}"),
                Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
            ));
        }

        if self.dry_run {
            spans.push(Span::styled(
                "  [dry-run]",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ));
        }

        frame.render_widget(
            Paragraph::new(Line::from(spans)).block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(Color::DarkGray)),
            ),
            area,
        );
    }

    // ── Body ──────────────────────────────────────────────────

    fn draw_body(&mut self, frame: &mut Frame, area: Rect) {
        match self.tab {
            Tab::Scripts => self.draw_scripts_tab(frame, area),
            Tab::History => self.draw_history_tab(frame, area),
            Tab::Tags    => self.draw_tags_tab(frame, area),
        }
    }

    // ── Scripts tab ───────────────────────────────────────────

    fn draw_scripts_tab(&mut self, frame: &mut Frame, area: Rect) {
        let outer = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = outer.inner(area);
        frame.render_widget(outer, area);

        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(inner);

        self.draw_script_list(frame, panes[0]);

        // Divider between list and preview
        frame.render_widget(
            Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(Color::DarkGray)),
            Rect { x: panes[1].x, y: panes[1].y, width: 1, height: panes[1].height },
        );

        let preview_area = Rect {
            x:      panes[1].x + 1,
            y:      panes[1].y,
            width:  panes[1].width.saturating_sub(1),
            height: panes[1].height,
        };
        self.draw_preview(frame, preview_area);
    }

    fn draw_script_list(&mut self, frame: &mut Frame, area: Rect) {
        // Search bar occupies top line when active
        let (list_area, search_area) =
            if self.mode == InputMode::Search || !self.search_query.is_empty() {
                let split = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(1), Constraint::Min(0)])
                    .split(area);
                (split[1], Some(split[0]))
            } else {
                (area, None)
            };

        if let Some(sa) = search_area {
            let cursor = if self.mode == InputMode::Search { "█" } else { "" };
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(" > ", Style::default().fg(Color::Yellow)),
                    Span::styled(self.search_query.clone(), Style::default().fg(Color::Yellow)),
                    Span::styled(cursor, Style::default().fg(Color::Yellow)),
                ])),
                sa,
            );
        }

        // "▶ " = 2 chars
        let avail = list_area.width.saturating_sub(2) as usize;

        let items: Vec<ListItem> = self.filtered
            .iter()
            .enumerate()
            .map(|(pos, &(script_idx, _))| {
                let script = &self.all_scripts[script_idx];
                let num    = format!("{:>3}  ", pos + 1);
                let name   = &script.name;
                let display_name = script.display_name();

                // Fuzzy highlight matched characters
                let name_spans: Vec<Span> = if self.search_query.is_empty() {
                    vec![Span::raw(display_name.clone())]
                } else {
                    let positions = fuzzy::match_positions(&self.search_query, name);
                    let mut pos_iter = positions.into_iter().peekable();
                    let mut spans = Vec::new();
                    let mut buf   = String::new();

                    for (i, ch) in display_name.chars().enumerate() {
                        if pos_iter.peek().copied() == Some(i) {
                            let _ = pos_iter.next();
                            if !buf.is_empty() {
                                spans.push(Span::raw(std::mem::take(&mut buf)));
                            }
                            spans.push(Span::styled(
                                ch.to_string(),
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD),
                            ));
                        } else {
                            buf.push(ch);
                        }
                    }

                    if !buf.is_empty() { spans.push(Span::raw(buf)); }
                    spans
                };

                let name_char_len = display_name.chars().count();
                let used = num.len() + name_char_len;
                let pad  = " ".repeat(avail.saturating_sub(used));

                let mut line_spans: Vec<Span> = vec![
                    Span::styled(num, Style::default().fg(Color::DarkGray)),
                ];
                line_spans.extend(name_spans);
                line_spans.push(Span::raw(pad));

                ListItem::new(Line::from(line_spans))
            })
            .collect();

        let list = List::new(items)
            .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ ");

        frame.render_stateful_widget(list, list_area, &mut self.list_state);
    }

    fn draw_preview(&mut self, frame: &mut Frame, area: Rect) {
        let Some((_, ref contents)) = self.preview_cache else {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "  No script selected.",
                    Style::default().fg(Color::DarkGray),
                )),
                area,
            );
            return;
        };

        let all_lines: Vec<&str> = contents.lines().collect();
        let total_lines = all_lines.len();

        // Clamp scroll
        let visible = area.height as usize;
        let max_scroll = total_lines.saturating_sub(visible);
        self.preview_scroll = self.preview_scroll.min(max_scroll);

        let has_scrollbar = total_lines > visible;
        let text_area = if has_scrollbar && area.width > 1 {
            Rect {
                x: area.x,
                y: area.y,
                width: area.width - 1,
                height: area.height,
            }
        } else {
            area
        };

        // Render only the visible window — no syntax highlighting
        let start = self.preview_scroll;
        let end   = (start + visible).min(total_lines);

        let lines: Vec<Line> = all_lines[start..end]
            .iter()
            .map(|line| Line::from(Span::raw((*line).to_string())))
            .collect();

        frame.render_widget(
            Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
            text_area,
        );

        // Scrollbar — only if content overflows
        if has_scrollbar {
            use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};
            let mut sb = ScrollbarState::new(max_scroll).position(self.preview_scroll);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(None)
                    .end_symbol(None),
                Rect {
                    x:      area.x + area.width - 1,
                    y:      area.y,
                    width:  1,
                    height: area.height,
                },
                &mut sb,
            );
        }
    }

    // ── History tab ───────────────────────────────────────────

    fn draw_history_tab(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Span::styled(
                " Recent Runs ",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ));

        if self.history.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "  No history yet.",
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                )))
                .block(block),
                area,
            );
            return;
        }

        let items: Vec<ListItem> = self.history.iter().map(|e| {
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(
                        format!(" {} ", e.name),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("×{}", e.run_count),
                        Style::default().fg(Color::Yellow),
                    ),
                ]),
                Line::from(Span::styled(
                    format!("   {}", history::relative_time(e.timestamp)),
                    Style::default().fg(Color::DarkGray),
                )),
            ])
        }).collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ ");

        frame.render_stateful_widget(list, area, &mut self.history_state);
    }

    // ── Tags tab ──────────────────────────────────────────────

    fn draw_tags_tab(&mut self, frame: &mut Frame, area: Rect) {
        // Split into left (tag list) and right (scripts in tag) panes
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(area);

        // ── Left: tag list ──
        let left_active = !self.tag_pane_right;
        let left_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(if left_active { Color::Cyan } else { Color::DarkGray }))
            .title(Span::styled(
                " Tags ",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ));

        if self.tag_list.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "  No tags found.",
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                )))
                .block(left_block),
                panes[0],
            );
        } else {
            let items: Vec<ListItem> = self.tag_list.iter().map(|tag| {
                let count = self.tag_scripts_map.get(tag).map_or(0, |v| v.len());
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!(" ⬥ {tag} "),
                        Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("({count})"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            }).collect();

            let list = List::new(items)
                .block(left_block)
                .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                .highlight_symbol("▶ ");

            frame.render_stateful_widget(list, panes[0], &mut self.tag_list_state);
        }

        // ── Right: scripts for hovered tag ──
        let hovered_tag = self.tag_list_state.selected()
            .and_then(|i| self.tag_list.get(i))
            .cloned()
            .unwrap_or_default();

        let right_active = self.tag_pane_right;
        let right_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(if right_active { Color::Cyan } else { Color::DarkGray }))
            .title(Span::styled(
                format!(" Scripts tagged ⬥ {hovered_tag} "),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ));

        if self.tag_scripts.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "  No scripts.",
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                )))
                .block(right_block),
                panes[1],
            );
        } else {
            let items: Vec<ListItem> = self.tag_scripts.iter().map(|&si| {
                let s = &self.all_scripts[si];
                ListItem::new(Line::from(Span::styled(
                    format!(" {}", s.display_name()),
                    Style::default().add_modifier(Modifier::BOLD),
                )))
            }).collect();

            let list = List::new(items)
                .block(right_block)
                .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                .highlight_symbol("▶ ");

            frame.render_stateful_widget(list, panes[1], &mut self.tag_scripts_state);
        }
    }

    // ── Footer ────────────────────────────────────────────────

    fn draw_footer(&self, frame: &mut Frame, area: Rect) {
        let line = if let Some((ref s, _)) = self.status_msg {
            Line::from(vec![
                Span::styled(" ◈ ", Style::default().fg(Color::Cyan)),
                Span::styled(s.clone(), Style::default().fg(Color::Green)),
            ])
        } else {
            let keybinds: &[(&str, &str)] = match (self.tab, self.mode) {
                (_, InputMode::Search) => &[
                    ("esc", "cancel"),
                    ("enter", "confirm"),
                    ("↑↓", "navigate"),
                ],
                (Tab::Scripts, _) => &[
                    ("↑↓/jk", "navigate"),
                    ("enter/r", "run"),
                    ("e", "edit"),
                    ("/", "search"),
                    ("t", "tags"),
                    ("y", "copy path"),
                    ("tab", "history"),
                    ("q", "quit"),
                ],
                (Tab::History, _) => &[
                    ("↑↓/jk", "navigate"),
                    ("enter", "run"),
                    ("tab", "tags"),
                    ("q", "quit"),
                ],
                (Tab::Tags, _) => &[
                    ("↑↓/jk", "navigate"),
                    ("←→/hl", "switch pane"),
                    ("enter", "run/filter"),
                    ("T", "clear filter"),
                    ("tab", "scripts"),
                    ("q", "quit"),
                ],
            };

            let mut spans: Vec<Span> = Vec::new();
            for (i, (key, desc)) in keybinds.iter().enumerate() {
                if i > 0 { spans.push(Span::raw("  ")); }
                spans.push(Span::styled(
                    key.to_string(),
                    Style::default().fg(Color::Cyan),
                ));
                spans.push(Span::styled(
                    format!(" {desc}"),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            Line::from(spans)
        };

        frame.render_widget(Paragraph::new(line), area);
    }
}

// ── Event loop ────────────────────────────────────────────────
// Draw once upfront, then only redraw after input or status expiry.
// When idle, poll blocks indefinitely — zero CPU usage.

pub fn run_tui(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    mut app: App,
) -> std::io::Result<AppResult> {
    terminal.draw(|f| app.draw(f))?;

    loop {
        // If a status message is showing, wake up when it expires.
        // Otherwise block forever until a key arrives.
        let timeout = app.status_msg
            .as_ref()
            .map(|(_, t)| STATUS_DURATION.saturating_sub(t.elapsed()))
            .unwrap_or(Duration::from_secs(3600));

        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if let Some(result) = app.handle_key(key.code, key.modifiers) {
                        return Ok(result);
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        // Redraw after every event (key or timeout)
        terminal.draw(|f| app.draw(f))?;
    }
}

// ── Clipboard ─────────────────────────────────────────────────

fn try_copy_to_clipboard(text: &str) -> bool {
    let cmds: &[(&str, &[&str])] = &[
        ("wl-copy", &[]),
        ("xclip",   &["-selection", "clipboard"]),
        ("xsel",    &["--clipboard", "--input"]),
        ("pbcopy",  &[]),
    ];
    for (cmd, args) in cmds {
        if let Ok(mut child) = std::process::Command::new(cmd)
            .args(*args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if child.wait().map(|s| s.success()).unwrap_or(false) {
                return true;
            }
        }
    }
    false
}