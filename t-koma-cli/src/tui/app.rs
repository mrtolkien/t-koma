use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use chrono::Utc;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{
    Frame, Terminal,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use tempfile::NamedTempFile;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;

use t_koma_core::{ModelConfig, ProviderType, Settings, WsMessage, WsResponse};
use t_koma_db::{
    Ghost, GhostDbPool, GhostRepository, KomaDbPool, Operator, OperatorRepository, OperatorStatus,
    Platform, SessionRepository,
};

use crate::{
    client::WsClient,
    tui::{
        layout::{content_layout, main_layout, sidebar_layout},
        state::{Category, FocusPane, GateFilter},
        theme,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperatorView {
    All,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptKind {
    AddOperator,
    AddModel,
    SetDefaultModel,
    NewGhost,
    DeleteGhostConfirmOne,
    DeleteGhostConfirmTwo,
    GateSearch,
}

#[derive(Debug, Default)]
struct PromptState {
    kind: Option<PromptKind>,
    buffer: String,
    target_ghost: Option<String>,
}

#[derive(Debug, Clone)]
struct GateRow {
    time: String,
    level: String,
    source: String,
    core: String,
    message: String,
}

#[derive(Debug)]
enum GateEvent {
    Status(bool),
    Log(GateRow),
}

#[derive(Debug, Default, Clone)]
struct Metrics {
    operator_count: usize,
    ghost_count: usize,
    recent_message_count: i64,
}

pub struct TuiApp {
    focus: FocusPane,
    category_idx: usize,
    options_idx: usize,
    content_idx: usize,
    should_exit: bool,
    status: String,

    settings: Settings,
    settings_toml: String,
    disk_toml: String,
    settings_dirty: bool,
    backup_path: Option<PathBuf>,

    db: Option<KomaDbPool>,
    operators: Vec<Operator>,
    ghosts: Vec<Ghost>,
    operator_view: OperatorView,
    config_scroll: u16,

    prompt: PromptState,

    gate_connected: bool,
    gate_paused: bool,
    gate_rows: Vec<GateRow>,
    gate_filter: GateFilter,
    gate_search: Option<String>,
    gate_rx: Option<mpsc::UnboundedReceiver<GateEvent>>,
    gate_scroll: u16,

    metrics: Metrics,
    metrics_last_refresh: Instant,
    anim_tick: usize,
}

impl TuiApp {
    pub async fn new() -> Self {
        let settings = Settings::load().unwrap_or_default();
        let settings_toml = settings.to_toml().unwrap_or_default();
        let disk_toml = load_disk_config().unwrap_or_else(|| settings_toml.clone());

        let db = KomaDbPool::new().await.ok();

        let mut app = Self {
            focus: FocusPane::Categories,
            category_idx: 0,
            options_idx: 0,
            content_idx: 0,
            should_exit: false,
            status: "TUI ready".to_string(),

            settings,
            settings_toml,
            disk_toml,
            settings_dirty: false,
            backup_path: None,

            db,
            operators: Vec::new(),
            ghosts: Vec::new(),
            operator_view: OperatorView::All,
            config_scroll: 0,

            prompt: PromptState::default(),

            gate_connected: false,
            gate_paused: false,
            gate_rows: Vec::new(),
            gate_filter: GateFilter::All,
            gate_search: None,
            gate_rx: None,
            gate_scroll: 0,

            metrics: Metrics::default(),
            metrics_last_refresh: Instant::now() - Duration::from_secs(30),
            anim_tick: 0,
        };

        app.start_logs_stream();
        app.sync_selection().await;
        app.refresh_metrics().await;
        app
    }

    pub async fn run(
        &mut self,
        terminal: &mut Terminal<impl Backend>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        while !self.should_exit {
            terminal.draw(|f| self.draw(f))?;
            self.tick().await;
        }

        Ok(())
    }

    fn selected_category(&self) -> Category {
        Category::ALL[self.category_idx]
    }

    fn options_for(&self, category: Category) -> Vec<String> {
        match category {
            Category::Gate => vec![],
            Category::Config => vec![
                "Add Model".to_string(),
                "Set Default".to_string(),
                "Toggle Discord".to_string(),
                "Edit in Editor".to_string(),
                "Reload".to_string(),
                if self.settings_dirty {
                    "Save (required after changes)".to_string()
                } else {
                    "Save".to_string()
                },
                "Restore Backup".to_string(),
            ],
            Category::Operators => vec![
                "List All".to_string(),
                "Add Operator".to_string(),
                "Pending Approvals".to_string(),
            ],
            Category::Ghosts => vec!["List All".to_string(), "New Ghost".to_string(), "Delete".to_string()],
        }
    }

    async fn tick(&mut self) {
        self.anim_tick = self.anim_tick.wrapping_add(1);
        let mut drained = Vec::new();
        if let Some(rx) = &mut self.gate_rx {
            for _ in 0..200 {
                match rx.try_recv() {
                    Ok(event) => drained.push(event),
                    Err(_) => break,
                }
            }
        }

        for event in drained {
            match event {
                GateEvent::Status(connected) => {
                    self.gate_connected = connected;
                }
                GateEvent::Log(row) => {
                    if !self.gate_paused {
                        self.gate_rows.push(row);
                        if self.gate_rows.len() > 2500 {
                            self.gate_rows.drain(..500);
                        }
                    }
                }
            }
        }

        if self.metrics_last_refresh.elapsed() > Duration::from_secs(8) {
            self.refresh_metrics().await;
        }

        if event::poll(Duration::from_millis(50)).unwrap_or(false)
            && let Ok(Event::Key(key)) = event::read()
            && key.kind == KeyEventKind::Press
        {
            self.handle_key(key).await;
        }
    }

    fn draw(&self, frame: &mut Frame) {
        let pulse = glow_color(self.anim_tick);
        let outer = Block::default()
            .title("╼ T-KOMA CYBERDECK ╾")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(pulse).add_modifier(Modifier::BOLD));
        let inner_area = outer.inner(frame.area());
        frame.render_widget(outer, frame.area());

        let (header, main) = main_layout(inner_area);
        self.draw_header(frame, header);

        let (categories_area, right_area) = sidebar_layout(main);
        self.draw_categories(frame, categories_area);

        if self.selected_category() == Category::Gate {
            self.draw_content(frame, right_area);
        } else {
            let (options_area, content_area) = content_layout(right_area);
            self.draw_options(frame, options_area);
            self.draw_content(frame, content_area);
        }

        if let Some(kind) = self.prompt.kind {
            self.draw_prompt_overlay(frame, kind);
        }
    }

    fn draw_header(&self, frame: &mut Frame, area: Rect) {
        let pulse = glow_color(self.anim_tick);
        let dot_color = pulse_red(self.anim_tick);
        let marquee = marquee_text("ようこそ、パペットマスター様", 36, self.anim_tick / 4);
        let model = if self.settings.default_model.is_empty() {
            "(unset)"
        } else {
            &self.settings.default_model
        };

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(40), Constraint::Length(18)])
            .split(area);

        let top = Line::from(vec![
            Span::styled("T-KOMA CONTROL PLANE", theme::header_title()),
            Span::raw(" | "),
            Span::styled(format!("󰀄 {}", self.metrics.operator_count), Style::default().fg(Color::Green)),
            Span::raw(" | "),
            Span::styled(format!("󰊠 {}", self.metrics.ghost_count), Style::default().fg(Color::Cyan)),
            Span::raw(" | "),
            Span::styled(
                format!("󰭻/5m {}", self.metrics.recent_message_count),
                Style::default().fg(Color::Yellow),
            ),
        ]);

        let gate_style = if self.gate_connected {
            theme::status_ok()
        } else {
            theme::status_err()
        };

        let second = Line::from(vec![
            Span::styled(
                if self.gate_connected {
                    "Gateway ONLINE"
                } else {
                    "Gateway OFFLINE"
                },
                gate_style.add_modifier(Modifier::BOLD),
            ),
            Span::raw(" | "),
            Span::styled(format!("󰒓 {}", model), Style::default().fg(Color::Magenta)),
            Span::raw(" | "),
            Span::styled(
                format!("󰍹 {}", self.selected_category().label()),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw(" | "),
            Span::styled(marquee, Style::default().fg(Color::LightBlue)),
        ]);

        let p = Paragraph::new(vec![top, second]).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(pulse)),
        );
        frame.render_widget(p, chunks[0]);

        let dot_style = if self.gate_connected {
            Style::default().fg(dot_color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let pulse_lines = vec![
            Line::from(Span::styled("   ╭──────╮   ", dot_style)),
            Line::from(Span::styled("   │ ████ │   ", dot_style)),
            Line::from(Span::styled("   │ ████ │   ", dot_style)),
            Line::from(Span::styled("   ╰──────╯   ", dot_style)),
        ];
        let pulse_widget = Paragraph::new(pulse_lines)
            .alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().fg(dot_color).add_modifier(Modifier::BOLD));
        frame.render_widget(pulse_widget, chunks[1]);
    }

    fn draw_categories(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = Category::ALL
            .iter()
            .enumerate()
            .map(|(idx, cat)| {
                let mut item = ListItem::new(format!(" {}", cat.label()));
                if idx == self.category_idx {
                    item = item.style(
                        Style::default()
                            .fg(glow_color(self.anim_tick))
                            .add_modifier(Modifier::BOLD),
                    );
                }
                item
            })
            .collect();

        let block = Block::default()
            .title("Categories")
            .borders(Borders::ALL)
            .border_style(border_glow(self.focus == FocusPane::Categories, self.anim_tick));
        frame.render_widget(List::new(items).block(block), area);
    }

    fn draw_options(&self, frame: &mut Frame, area: Rect) {
        let options = self.options_for(self.selected_category());
        let items: Vec<ListItem> = options
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let mut item_widget = ListItem::new(item.clone());
                if idx == self.options_idx && self.focus == FocusPane::Options {
                    item_widget = item_widget.style(theme::selected());
                }
                item_widget
            })
            .collect();

        let block = Block::default()
            .title("Options")
            .borders(Borders::ALL)
            .border_style(border_glow(self.focus == FocusPane::Options, self.anim_tick));
        frame.render_widget(List::new(items).block(block), area);
    }

    fn draw_content(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title("Content")
            .borders(Borders::ALL)
            .border_style(border_glow(self.focus == FocusPane::Content, self.anim_tick));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        match self.selected_category() {
            Category::Config => {
                let mut lines = vec![];
                if self.settings_dirty {
                    lines.push(Line::from(Span::styled(
                        "Unsaved changes. Use option: Save (required after changes).",
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )));
                }

                lines.extend(highlight_toml_with_diff(&self.settings_toml, &self.disk_toml));

                let text = Text::from(lines);
                let p = Paragraph::new(text)
                    .scroll((self.config_scroll, 0))
                    .wrap(Wrap { trim: false });
                frame.render_widget(p, inner);
            }
            Category::Operators => {
                let items: Vec<ListItem> = self
                    .operators
                    .iter()
                    .enumerate()
                    .map(|(idx, op)| {
                        let icon = match op.status {
                            OperatorStatus::Approved => "OK",
                            OperatorStatus::Pending => "PD",
                            OperatorStatus::Denied => "NO",
                        };
                        let text = format!(
                            "{} {} [{}] {}",
                            icon, op.name, op.platform, op.id
                        );
                        let mut item = ListItem::new(text);
                        if idx == self.content_idx && self.focus == FocusPane::Content {
                            item = item.style(theme::selected());
                        }
                        item
                    })
                    .collect();
                frame.render_widget(List::new(items), inner);
            }
            Category::Ghosts => {
                let items: Vec<ListItem> = self
                    .ghosts
                    .iter()
                    .enumerate()
                    .map(|(idx, ghost)| {
                        let mut item = ListItem::new(format!(
                            "{} | owner={} | cwd={}",
                            ghost.name,
                            ghost.owner_operator_id,
                            ghost.cwd.clone().unwrap_or_else(|| "-".to_string())
                        ));
                        if idx == self.content_idx && self.focus == FocusPane::Content {
                            item = item.style(theme::selected());
                        }
                        item
                    })
                    .collect();
                frame.render_widget(List::new(items), inner);
            }
            Category::Gate => {
                let lines = self.filtered_gate_lines_colored();
                let p = Paragraph::new(Text::from(lines))
                    .scroll((self.gate_scroll, 0))
                    .wrap(Wrap { trim: false });
                frame.render_widget(p, inner);
            }
        }

    }

    fn draw_prompt_overlay(&self, frame: &mut Frame, kind: PromptKind) {
        let area = centered_rect(70, 30, frame.area());
        frame.render_widget(Clear, area);
        let block = Block::default()
            .title("Input")
            .borders(Borders::ALL)
            .border_style(theme::border(true));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let title = match kind {
            PromptKind::AddOperator => "Operator name",
            PromptKind::AddModel => "alias,provider,model",
            PromptKind::SetDefaultModel => "Default model alias",
            PromptKind::NewGhost => "owner_operator_id,ghost_name",
            PromptKind::DeleteGhostConfirmOne => "Type DELETE",
            PromptKind::DeleteGhostConfirmTwo => "Type ghost name",
            PromptKind::GateSearch => "Search logs (blank clears)",
        };
        let p = Paragraph::new(format!("{}\n\n{}", title, self.prompt.buffer));
        frame.render_widget(p, inner);
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        if self.prompt.kind.is_some() {
            self.handle_prompt_key(key).await;
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.should_exit = true,
            KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => self.should_exit = true,
            KeyCode::Esc => self.should_exit = true,
            KeyCode::Tab => self.focus = self.focus.next(self.selected_category().has_options()),
            KeyCode::Left | KeyCode::Char('h') => {
                self.focus = self.focus.prev(self.selected_category().has_options())
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.focus = self.focus.next(self.selected_category().has_options())
            }
            KeyCode::Up | KeyCode::Char('k') => self.navigate_up().await,
            KeyCode::Down | KeyCode::Char('j') => self.navigate_down().await,
            KeyCode::Enter => self.activate().await,
            _ => self.handle_gate_shortcuts(key).await,
        }
    }

    async fn navigate_up(&mut self) {
        match self.focus {
            FocusPane::Categories => {
                if self.category_idx > 0 {
                    self.category_idx -= 1;
                    self.options_idx = 0;
                    self.content_idx = 0;
                    self.sync_selection().await;
                }
            }
            FocusPane::Options => {
                if self.options_idx > 0 {
                    self.options_idx -= 1;
                    self.sync_selection().await;
                }
            }
            FocusPane::Content => match self.selected_category() {
                Category::Config => self.config_scroll = self.config_scroll.saturating_sub(1),
                Category::Gate => self.gate_scroll = self.gate_scroll.saturating_sub(1),
                _ => {
                    if self.content_idx > 0 {
                        self.content_idx -= 1;
                    }
                }
            },
        }
    }

    async fn navigate_down(&mut self) {
        match self.focus {
            FocusPane::Categories => {
                if self.category_idx + 1 < Category::ALL.len() {
                    self.category_idx += 1;
                    self.options_idx = 0;
                    self.content_idx = 0;
                    self.sync_selection().await;
                }
            }
            FocusPane::Options => {
                let len = self.options_for(self.selected_category()).len();
                if self.options_idx + 1 < len {
                    self.options_idx += 1;
                    self.sync_selection().await;
                }
            }
            FocusPane::Content => match self.selected_category() {
                Category::Config => self.config_scroll = self.config_scroll.saturating_add(1),
                Category::Gate => self.gate_scroll = self.gate_scroll.saturating_add(1),
                Category::Operators => {
                    if self.content_idx + 1 < self.operators.len() {
                        self.content_idx += 1;
                    }
                }
                Category::Ghosts => {
                    if self.content_idx + 1 < self.ghosts.len() {
                        self.content_idx += 1;
                    }
                }
            },
        }
    }

    async fn activate(&mut self) {
        match self.focus {
            FocusPane::Categories => {
                self.focus = if self.selected_category().has_options() {
                    FocusPane::Options
                } else {
                    FocusPane::Content
                };
            }
            FocusPane::Options => self.activate_option().await,
            FocusPane::Content => {
                if self.selected_category() == Category::Operators
                    && self.operator_view == OperatorView::Pending
                {
                    self.approve_selected_operator().await;
                }
            }
        }
    }

    async fn activate_option(&mut self) {
        match self.selected_category() {
            Category::Config => match self.options_idx {
                0 => self.begin_prompt(PromptKind::AddModel, None),
                1 => self.begin_prompt(PromptKind::SetDefaultModel, None),
                2 => {
                    self.settings.discord.enabled = !self.settings.discord.enabled;
                    self.settings_dirty = true;
                    self.refresh_settings_toml();
                    self.status = format!("discord.enabled={}", self.settings.discord.enabled);
                }
                3 => {
                    if let Err(e) = self.edit_in_editor() {
                        self.status = format!("Editor failed: {}", e);
                    }
                }
                4 => self.reload_settings(),
                5 => self.save_settings(),
                6 => self.restore_backup(),
                _ => {}
            },
            Category::Operators => match self.options_idx {
                0 => {
                    self.operator_view = OperatorView::All;
                    self.refresh_operators().await;
                }
                1 => self.begin_prompt(PromptKind::AddOperator, None),
                2 => {
                    self.operator_view = OperatorView::Pending;
                    self.refresh_operators().await;
                }
                _ => {}
            },
            Category::Ghosts => match self.options_idx {
                0 => self.refresh_ghosts().await,
                1 => self.begin_prompt(PromptKind::NewGhost, None),
                2 => {
                    if let Some(name) = self.ghosts.get(self.content_idx).map(|g| g.name.clone()) {
                        self.begin_prompt(PromptKind::DeleteGhostConfirmOne, Some(name));
                    } else {
                        self.status = "No ghost selected".to_string();
                    }
                }
                _ => {}
            },
            Category::Gate => {}
        }

        self.refresh_metrics().await;
    }

    async fn handle_gate_shortcuts(&mut self, key: KeyEvent) {
        if self.selected_category() != Category::Gate {
            if self.selected_category() == Category::Operators
                && self.focus == FocusPane::Content
                && self.operator_view == OperatorView::Pending
            {
                match key.code {
                    KeyCode::Char('a') => self.approve_selected_operator().await,
                    KeyCode::Char('d') => self.deny_selected_operator().await,
                    _ => {}
                }
            }
            return;
        }

        match key.code {
            KeyCode::Char('r') => self.restart_gateway().await,
            KeyCode::Char('/') => self.begin_prompt(PromptKind::GateSearch, None),
            KeyCode::Char(' ') => {
                self.gate_paused = !self.gate_paused;
                self.status = if self.gate_paused {
                    "Log stream paused".to_string()
                } else {
                    "Log stream resumed".to_string()
                };
            }
            KeyCode::Char('c') => {
                self.gate_rows.clear();
                self.status = "Logs cleared".to_string();
            }
            KeyCode::Char('1') => self.gate_filter = GateFilter::All,
            KeyCode::Char('2') => self.gate_filter = GateFilter::Gateway,
            KeyCode::Char('3') => self.gate_filter = GateFilter::Ghost,
            KeyCode::Char('4') => self.gate_filter = GateFilter::Operator,
            KeyCode::Char('5') => self.gate_filter = GateFilter::Transport,
            KeyCode::Char('6') => self.gate_filter = GateFilter::Error,
            KeyCode::Esc => self.gate_search = None,
            _ => {}
        }
    }

    fn begin_prompt(&mut self, kind: PromptKind, target_ghost: Option<String>) {
        self.prompt.kind = Some(kind);
        self.prompt.buffer.clear();
        self.prompt.target_ghost = target_ghost;
    }

    async fn handle_prompt_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.prompt = PromptState::default();
            }
            KeyCode::Backspace => {
                self.prompt.buffer.pop();
            }
            KeyCode::Char(c) => {
                self.prompt.buffer.push(c);
            }
            KeyCode::Enter => {
                let kind = self.prompt.kind;
                let input = self.prompt.buffer.trim().to_string();
                let target = self.prompt.target_ghost.clone();
                self.prompt = PromptState::default();

                match kind {
                    Some(PromptKind::AddOperator) => self.add_operator(&input).await,
                    Some(PromptKind::AddModel) => self.add_model(&input),
                    Some(PromptKind::SetDefaultModel) => self.set_default_model(&input),
                    Some(PromptKind::NewGhost) => self.add_ghost(&input).await,
                    Some(PromptKind::DeleteGhostConfirmOne) => {
                        if input == "DELETE" {
                            self.begin_prompt(PromptKind::DeleteGhostConfirmTwo, target);
                        } else {
                            self.status = "Delete aborted".to_string();
                        }
                    }
                    Some(PromptKind::DeleteGhostConfirmTwo) => {
                        self.delete_ghost_confirmed(target.as_deref(), &input).await;
                    }
                    Some(PromptKind::GateSearch) => {
                        self.gate_search = if input.is_empty() { None } else { Some(input) };
                    }
                    None => {}
                }
            }
            _ => {}
        }
    }

    async fn sync_selection(&mut self) {
        match self.selected_category() {
            Category::Operators => {
                self.operator_view = if self.options_idx == 2 {
                    OperatorView::Pending
                } else {
                    OperatorView::All
                };
                self.refresh_operators().await;
            }
            Category::Ghosts => self.refresh_ghosts().await,
            Category::Config => {}
            Category::Gate => {}
        }
        self.refresh_metrics().await;
    }

    async fn refresh_metrics(&mut self) {
        self.metrics_last_refresh = Instant::now();

        let Some(db) = &self.db else {
            self.metrics = Metrics::default();
            return;
        };

        let operator_count = OperatorRepository::list_all(db.pool())
            .await
            .map(|list| list.len())
            .unwrap_or(0);

        let ghosts = GhostRepository::list_all(db.pool()).await.unwrap_or_default();
        let ghost_count = ghosts.len();

        let mut recent_message_count = 0;
        let since = Utc::now().timestamp() - 300;
        for ghost in &ghosts {
            if let Ok(ghost_db) = GhostDbPool::new(&ghost.name).await
                && let Ok(count) = SessionRepository::count_messages_since(ghost_db.pool(), since).await
            {
                recent_message_count += count;
            }
        }

        self.metrics = Metrics {
            operator_count,
            ghost_count,
            recent_message_count,
        };
    }

    async fn refresh_operators(&mut self) {
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        let res = match self.operator_view {
            OperatorView::All => OperatorRepository::list_all(db.pool()).await,
            OperatorView::Pending => {
                OperatorRepository::list_by_status(db.pool(), OperatorStatus::Pending, None).await
            }
        };

        match res {
            Ok(list) => {
                self.operators = list;
                if self.content_idx >= self.operators.len() {
                    self.content_idx = self.operators.len().saturating_sub(1);
                }
            }
            Err(e) => self.status = format!("Operators refresh failed: {}", e),
        }
    }

    async fn refresh_ghosts(&mut self) {
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        match GhostRepository::list_all(db.pool()).await {
            Ok(list) => {
                self.ghosts = list;
                if self.content_idx >= self.ghosts.len() {
                    self.content_idx = self.ghosts.len().saturating_sub(1);
                }
            }
            Err(e) => self.status = format!("Ghost refresh failed: {}", e),
        }
    }

    async fn add_operator(&mut self, input: &str) {
        if input.is_empty() {
            self.status = "Operator name is required".to_string();
            return;
        }
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        match OperatorRepository::create_new(db.pool(), input, Platform::Api).await {
            Ok(op) => match OperatorRepository::approve(db.pool(), &op.id).await {
                Ok(_) => {
                    self.status = format!("Created approved operator {}", op.id);
                    self.refresh_operators().await;
                    self.refresh_metrics().await;
                }
                Err(e) => self.status = format!("Approval failed: {}", e),
            },
            Err(e) => self.status = format!("Create operator failed: {}", e),
        }
    }

    async fn approve_selected_operator(&mut self) {
        let Some(operator) = self.operators.get(self.content_idx) else {
            return;
        };
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        if let Err(e) = OperatorRepository::approve(db.pool(), &operator.id).await {
            self.status = format!("Approve failed: {}", e);
            return;
        }
        self.status = format!("Approved {}", operator.id);
        self.refresh_operators().await;
    }

    async fn deny_selected_operator(&mut self) {
        let Some(operator) = self.operators.get(self.content_idx) else {
            return;
        };
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        if let Err(e) = OperatorRepository::deny(db.pool(), &operator.id).await {
            self.status = format!("Deny failed: {}", e);
            return;
        }
        self.status = format!("Denied {}", operator.id);
        self.refresh_operators().await;
    }

    async fn add_ghost(&mut self, input: &str) {
        let parts: Vec<&str> = input.split(',').map(|v| v.trim()).collect();
        if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
            self.status = "Use: owner_operator_id,ghost_name".to_string();
            return;
        }
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        match GhostRepository::create(db.pool(), parts[0], parts[1]).await {
            Ok(_) => {
                self.status = format!("Created ghost {}", parts[1]);
                self.refresh_ghosts().await;
                self.refresh_metrics().await;
            }
            Err(e) => self.status = format!("Create ghost failed: {}", e),
        }
    }

    async fn delete_ghost_confirmed(&mut self, target: Option<&str>, typed_name: &str) {
        let Some(ghost_name) = target else {
            self.status = "Delete failed: no selected ghost".to_string();
            return;
        };
        if typed_name != ghost_name {
            self.status = "Delete aborted: name mismatch".to_string();
            return;
        }
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        match GhostRepository::delete_by_name(db.pool(), ghost_name).await {
            Ok(()) => {
                if let Ok(path) = GhostDbPool::workspace_path_for(ghost_name)
                    && path.exists()
                {
                    let _ = fs::remove_dir_all(&path);
                }
                self.status = format!("Deleted ghost {}", ghost_name);
                self.refresh_ghosts().await;
                self.refresh_metrics().await;
            }
            Err(e) => self.status = format!("Delete failed: {}", e),
        }
    }

    fn add_model(&mut self, input: &str) {
        let parts: Vec<&str> = input.split(',').map(|v| v.trim()).collect();
        if parts.len() != 3 {
            self.status = "Use: alias,provider,model".to_string();
            return;
        }

        let provider = match parts[1].parse::<ProviderType>() {
            Ok(provider) => provider,
            Err(e) => {
                self.status = format!("Invalid provider: {}", e);
                return;
            }
        };

        self.settings.models.insert(
            parts[0].to_string(),
            ModelConfig {
                provider,
                model: parts[2].to_string(),
            },
        );
        self.settings_dirty = true;
        self.refresh_settings_toml();
        self.status = format!("Added model alias {}", parts[0]);
    }

    fn set_default_model(&mut self, alias: &str) {
        if !self.settings.models.contains_key(alias) {
            self.status = format!("Unknown alias {}", alias);
            return;
        }

        self.settings.default_model = alias.to_string();
        self.settings_dirty = true;
        self.refresh_settings_toml();
        self.status = format!("Default model set to {}", alias);
    }

    fn refresh_settings_toml(&mut self) {
        if let Ok(toml) = self.settings.to_toml() {
            self.settings_toml = toml;
        }
    }

    fn reload_settings(&mut self) {
        match Settings::load() {
            Ok(settings) => {
                self.settings = settings;
                self.refresh_settings_toml();
                self.disk_toml = load_disk_config().unwrap_or_else(|| self.settings_toml.clone());
                self.settings_dirty = false;
                self.status = "Reloaded settings".to_string();
            }
            Err(e) => self.status = format!("Reload failed: {}", e),
        }
    }

    fn save_settings(&mut self) {
        match self.settings.save() {
            Ok(()) => {
                self.settings_dirty = false;
                self.disk_toml = self.settings_toml.clone();
                self.status = "Settings saved".to_string();
            }
            Err(e) => self.status = format!("Save failed: {}", e),
        }
    }

    fn restore_backup(&mut self) {
        let Some(path) = self.backup_path.clone() else {
            self.status = "No backup available".to_string();
            return;
        };

        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => {
                self.status = format!("Backup read failed: {}", e);
                return;
            }
        };

        let parsed = match Settings::from_toml(&content) {
            Ok(parsed) => parsed,
            Err(e) => {
                self.status = format!("Backup parse failed: {}", e);
                return;
            }
        };

        let config_path = match Settings::config_path() {
            Ok(path) => path,
            Err(e) => {
                self.status = format!("Config path failed: {}", e);
                return;
            }
        };

        if let Err(e) = fs::write(&config_path, content) {
            self.status = format!("Restore write failed: {}", e);
            return;
        }

        self.settings = parsed;
        self.refresh_settings_toml();
        self.disk_toml = self.settings_toml.clone();
        self.settings_dirty = false;
        self.status = format!("Restored backup {}", path.display());
    }

    fn edit_in_editor(&mut self) -> Result<(), String> {
        let config_path = Settings::config_path().map_err(|e| e.to_string())?;
        let current_content = fs::read_to_string(&config_path).unwrap_or_else(|_| self.settings_toml.clone());

        let mut temp_file = NamedTempFile::new().map_err(|e| e.to_string())?;
        temp_file
            .write_all(current_content.as_bytes())
            .map_err(|e| e.to_string())?;
        let temp_path = temp_file.path().to_path_buf();

        terminal::disable_raw_mode().map_err(|e| e.to_string())?;
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);

        let escaped_path = shell_quote(temp_path.to_string_lossy().as_ref());
        let editor_cmd = format!("${{EDITOR:-vi}} {}", escaped_path);
        let status = Command::new("sh")
            .arg("-lc")
            .arg(editor_cmd)
            .status()
            .map_err(|e| e.to_string())?;

        let _ = execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture);
        terminal::enable_raw_mode().map_err(|e| e.to_string())?;
        let _ = execute!(io::stdout(), terminal::Clear(ClearType::All));

        if !status.success() {
            return Err("Editor exited with non-zero status".to_string());
        }

        let edited = fs::read_to_string(&temp_path).map_err(|e| e.to_string())?;
        match Settings::from_toml(&edited) {
            Ok(parsed) => {
                self.settings = parsed;
                self.refresh_settings_toml();
                self.settings_dirty = self.settings_toml != self.disk_toml;
                self.status = "Edited config loaded. Press Save to persist.".to_string();
                Ok(())
            }
            Err(e) => {
                let stamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|err| err.to_string())?
                    .as_millis();
                let backup_name = format!("config.toml.bak.{}", stamp);
                let backup_path = config_path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join(backup_name);
                if config_path.exists() {
                    fs::copy(&config_path, &backup_path).map_err(|copy_err| copy_err.to_string())?;
                    self.backup_path = Some(backup_path.clone());
                    self.status = format!(
                        "Invalid TOML rejected. Backup saved at {}",
                        backup_path.display()
                    );
                }
                Err(e.to_string())
            }
        }
    }

    async fn restart_gateway(&mut self) {
        let ws_url = ws_url_for_cli(&self.settings.ws_url());
        let (tx, mut rx) = match WsClient::connect(&ws_url).await {
            Ok(pair) => pair,
            Err(e) => {
                self.status = format!("Gateway connect failed: {}", e);
                self.gate_connected = false;
                return;
            }
        };

        if tx.send(WsMessage::RestartGateway).is_err() {
            self.status = "Restart command send failed".to_string();
            return;
        }

        match tokio::time::timeout(Duration::from_secs(3), rx.next()).await {
            Ok(Some(WsResponse::GatewayRestarting)) => {
                self.status = "Gateway restarting...".to_string();
            }
            Ok(Some(WsResponse::Error { message })) => {
                self.status = format!("Restart failed: {}", message);
            }
            _ => {
                self.status = "Restart requested".to_string();
            }
        }
    }

    fn start_logs_stream(&mut self) {
        let logs_url = self.settings.ws_url().replace("/ws", "/logs");
        let (tx, rx) = mpsc::unbounded_channel();
        self.gate_rx = Some(rx);

        tokio::spawn(async move {
            loop {
                let connection = connect_async(&logs_url).await;
                let Ok((stream, _)) = connection else {
                    let _ = tx.send(GateEvent::Status(false));
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                };

                let _ = tx.send(GateEvent::Status(true));
                let (_write, mut read) = stream.split();

                loop {
                    match read.next().await {
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                            if let Some(row) = parse_gate_row(text.as_str()) {
                                let _ = tx.send(GateEvent::Log(row));
                            }
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => {
                            let _ = tx.send(GateEvent::Status(false));
                            break;
                        }
                        Some(Err(_)) | None => {
                            let _ = tx.send(GateEvent::Status(false));
                            break;
                        }
                        _ => {}
                    }
                }

                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }

    fn filtered_gate_lines_colored(&self) -> Vec<Line<'static>> {
        let mut rows: Vec<&GateRow> = self
            .gate_rows
            .iter()
            .filter(|row| match self.gate_filter {
                GateFilter::All => true,
                GateFilter::Gateway => {
                    row.source == "gateway" || row.source == "trace" || row.source == "route"
                }
                GateFilter::Ghost => row.source == "ghost",
                GateFilter::Operator => row.source == "operator",
                GateFilter::Transport => row.source == "ws" || row.source == "http",
                GateFilter::Error => row.level == "ERROR" || row.level == "WARN",
            })
            .collect();

        if let Some(search) = &self.gate_search {
            let s = search.to_lowercase();
            rows.retain(|row| row.message.to_lowercase().contains(&s));
        }

        let mut lines = vec![Line::from(vec![
            Span::styled(
                "filters: [1]all [2]gateway [3]ghost [4]operator [5]transport [6]warn/error",
                Style::default().fg(Color::DarkGray),
            ),
        ])];

        if rows.is_empty() {
            lines.push(Line::from(Span::styled(
                "No log data",
                Style::default().fg(Color::DarkGray),
            )));
            return lines;
        }

        for row in rows {
            let source_style = match row.source.as_str() {
                "ghost" => Style::default().fg(Color::Cyan),
                "operator" => Style::default().fg(Color::Yellow),
                "route" => Style::default().fg(Color::LightMagenta),
                "ws" | "http" => Style::default().fg(Color::Magenta),
                "trace" => Style::default().fg(Color::LightBlue),
                _ => Style::default().fg(Color::White),
            };
            let level_style = match row.level.as_str() {
                "ERROR" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                "WARN" => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                "INFO" => Style::default().fg(Color::Green),
                _ => Style::default().fg(Color::DarkGray),
            };

            lines.push(Line::from(vec![
                Span::styled(format!("{} ", row.time), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:>5} ", row.level), level_style),
                Span::styled(format!("{:>9} ", row.source), source_style),
                Span::styled(
                    format!(" {}", truncate_for_cell(&row.core, 120)),
                    Style::default().fg(Color::LightBlue),
                ),
            ]));

            let body_style = match row.source.as_str() {
                "operator" => Style::default().fg(Color::Yellow),
                "ghost" => Style::default().fg(Color::Cyan),
                "route" => Style::default().fg(Color::LightMagenta),
                _ => Style::default().fg(Color::White),
            };
            let marker = match row.source.as_str() {
                "operator" => "  ↳ ",
                "ghost" => "  ⇢ ",
                "route" => "  ⤷ ",
                _ => "  · ",
            };
            for msg_line in markdown_to_lines(&row.message) {
                let mut spans = vec![Span::styled(marker, body_style)];
                spans.extend(msg_line.spans.into_iter().map(|s| {
                    if s.style == Style::default() {
                        Span::styled(s.content.to_string(), body_style)
                    } else {
                        s
                    }
                }));
                lines.push(Line::from(spans));
            }
        }

        lines
    }
}

fn parse_gate_row(text: &str) -> Option<GateRow> {
    let json: serde_json::Value = serde_json::from_str(text).ok()?;
    if json.get("type") == Some(&serde_json::Value::String("connected".to_string())) {
        return None;
    }

    if json.get("type") == Some(&serde_json::Value::String("log_entry".to_string())) {
        let entry = json.get("entry")?;
        let kind = entry.get("kind").and_then(|v| v.as_str()).unwrap_or("info");
        if kind == "discord_response" {
            return None;
        }
        let level = entry
            .get("level")
            .and_then(|v| v.as_str())
            .unwrap_or("INFO")
            .to_string();
        let source = match kind {
            "discord_message" => "operator",
            "discord_response" => "ghost",
            "operator_message" => "operator",
            "ghost_message" => "ghost",
            "routing" => "route",
            "web_socket" => "ws",
            "http_request" => "http",
            "trace" => "trace",
            _ => "gateway",
        }
        .to_string();

        let (core, message) = match kind {
            "discord_message" => {
                let user = entry.get("user").and_then(|v| v.as_str()).unwrap_or("user");
                let channel = entry
                    .get("channel")
                    .and_then(|v| v.as_str())
                    .unwrap_or("channel");
                let content = entry
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                (format!("@{} #{}", user, channel), content.to_string())
            }
            "discord_response" => {
                let user = entry.get("user").and_then(|v| v.as_str()).unwrap_or("user");
                let content = entry
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                (format!("to @{}", user), content.to_string())
            }
            "operator_message" => {
                let ghost_name = entry
                    .get("ghost_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let content = entry
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                (format!("to @{}", ghost_name), content.to_string())
            }
            "ghost_message" => {
                let ghost_name = entry
                    .get("ghost_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ghost");
                let content = entry
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                (format!("from {}", ghost_name), content.to_string())
            }
            "web_socket" => {
                let event = entry.get("event").and_then(|v| v.as_str()).unwrap_or("event");
                let client = entry
                    .get("client_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("client");
                (client.to_string(), event.to_string())
            }
            "http_request" => {
                let method = entry.get("method").and_then(|v| v.as_str()).unwrap_or("-");
                let path = entry.get("path").and_then(|v| v.as_str()).unwrap_or("-");
                let status = entry.get("status").and_then(|v| v.as_i64()).unwrap_or(0);
                (format!("{} {}", method, path), status.to_string())
            }
            "trace" => {
                let target = entry.get("target").and_then(|v| v.as_str()).unwrap_or("");
                let message = entry.get("message").and_then(|v| v.as_str()).unwrap_or("");
                (target.to_string(), message.to_string())
            }
            "routing" => {
                let operator_id = entry
                    .get("operator_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("operator");
                let ghost_name = entry
                    .get("ghost_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("ghost");
                let session_id = entry
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("session");
                (
                    format!("{} -> @{}", operator_id, ghost_name),
                    format!("session {}", session_id),
                )
            }
            _ => (
                kind.to_string(),
                entry
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| entry.to_string()),
            ),
        };

        return Some(GateRow {
            time: Utc::now().format("%H:%M:%S").to_string(),
            level,
            source,
            core,
            message: truncate_for_message(&message, 4000),
        });
    }

    Some(GateRow {
        time: Utc::now().format("%H:%M:%S").to_string(),
        level: "INFO".to_string(),
        source: "gateway".to_string(),
        core: "raw".to_string(),
        message: truncate_for_message(text, 4000),
    })
}

fn highlight_toml(content: &str) -> Vec<Line<'static>> {
    content
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                return Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::DarkGray),
                ));
            }

            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                return Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ));
            }

            if let Some((key, value)) = line.split_once('=') {
                let key_span = Span::styled(key.to_string(), Style::default().fg(Color::Yellow));
                let eq_span = Span::raw("=");
                let value_style = if value.trim().starts_with('"') {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Magenta)
                };
                let value_span = Span::styled(value.to_string(), value_style);
                return Line::from(vec![key_span, eq_span, value_span]);
            }

            Line::from(Span::raw(line.to_string()))
        })
        .collect()
}

fn highlight_toml_with_diff(content: &str, disk_content: &str) -> Vec<Line<'static>> {
    let current: Vec<&str> = content.lines().collect();
    let disk: Vec<&str> = disk_content.lines().collect();
    let mut lines = Vec::with_capacity(current.len());

    for (idx, line) in current.iter().enumerate() {
        let changed = disk.get(idx).copied() != Some(*line);
        let marker = if changed { "▋" } else { " " };
        let marker_style = if changed {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let mut rendered = highlight_toml(line).into_iter().next().unwrap_or_else(|| Line::from(""));
        rendered.spans.insert(0, Span::styled(format!("{:>4} {} ", idx + 1, marker), marker_style));
        lines.push(rendered);
    }

    lines
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn ws_url_for_cli(ws_url: &str) -> String {
    match url::Url::parse(ws_url) {
        Ok(mut url) => {
            url.query_pairs_mut().append_pair("client", "cli");
            url.to_string()
        }
        Err(_) => ws_url.to_string(),
    }
}

fn load_disk_config() -> Option<String> {
    let path = Settings::config_path().ok()?;
    fs::read_to_string(path).ok()
}

fn shell_quote(value: &str) -> String {
    let escaped = value.replace('\'', "'\\''");
    format!("'{}'", escaped)
}

fn glow_color(tick: usize) -> Color {
    let phase = tick % 200;
    let up = if phase <= 100 { phase } else { 200 - phase } as u8;
    let boost = (up as u16 * 90 / 100) as u8;
    Color::Rgb(0, 160 + boost, 170 + boost / 2)
}

fn pulse_red(tick: usize) -> Color {
    let phase = tick % 200;
    let up = if phase <= 100 { phase } else { 200 - phase } as u8;
    let boost = (up as u16 * 130 / 100) as u8;
    Color::Rgb(120 + boost, 10, 10)
}

fn border_glow(has_focus: bool, tick: usize) -> Style {
    if has_focus {
        Style::default()
            .fg(glow_color(tick))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Rgb(45, 60, 68))
    }
}

fn marquee_text(text: &str, width: usize, offset: usize) -> String {
    let mut chars: Vec<char> = format!("{}   ", text).chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    let len = chars.len();
    let start = offset % len;
    chars.rotate_left(start);
    let visible: String = chars.into_iter().take(width).collect();
    format!("{visible:width$}")
}

fn markdown_to_lines(message: &str) -> Vec<Line<'static>> {
    message
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("### ") || trimmed.starts_with("## ") || trimmed.starts_with("# ")
            {
                return Line::from(Span::styled(
                    trimmed.trim_start_matches('#').trim().to_string(),
                    Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD),
                ));
            }
            if let Some(rest) = trimmed.strip_prefix("- ") {
                let mut spans = vec![Span::styled("• ", Style::default().fg(Color::Magenta))];
                spans.extend(parse_inline_markdown(rest));
                return Line::from(spans);
            }
            Line::from(parse_inline_markdown(trimmed))
        })
        .collect()
}

fn parse_inline_markdown(input: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut rest = input;

    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix("**")
            && let Some(end) = stripped.find("**")
        {
            let bold = &stripped[..end];
            spans.push(Span::styled(
                bold.to_string(),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ));
            rest = &stripped[end + 2..];
            continue;
        }

        if let Some(stripped) = rest.strip_prefix('`')
            && let Some(end) = stripped.find('`')
        {
            let code = &stripped[..end];
            spans.push(Span::styled(
                code.to_string(),
                Style::default().fg(Color::Yellow).bg(Color::Rgb(20, 30, 45)),
            ));
            rest = &stripped[end + 1..];
            continue;
        }

        let next_bold = rest.find("**");
        let next_code = rest.find('`');
        let next = match (next_bold, next_code) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => rest.len(),
        };

        let plain = &rest[..next];
        spans.push(Span::raw(plain.to_string()));
        rest = &rest[next..];
    }

    if spans.is_empty() {
        vec![Span::raw(String::new())]
    } else {
        spans
    }
}

fn truncate_for_cell(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else if max_chars > 1 {
        let kept: String = chars.into_iter().take(max_chars - 1).collect();
        format!("{kept}…")
    } else {
        "…".to_string()
    }
}

fn truncate_for_message(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else {
        let kept: String = chars.into_iter().take(max_chars).collect();
        format!("{kept}\n…[truncated]")
    }
}

#[cfg(test)]
mod tests {
    use super::ws_url_for_cli;

    #[test]
    fn test_ws_url_for_cli_adds_client_query() {
        assert_eq!(
            ws_url_for_cli("ws://127.0.0.1:3000/ws"),
            "ws://127.0.0.1:3000/ws?client=cli"
        );
    }
}
