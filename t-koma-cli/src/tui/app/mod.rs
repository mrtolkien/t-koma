mod actions;
mod input;
mod logs;
mod render;
mod state;
mod util;

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use crossterm::event::{self, Event, KeyEventKind};
use ratatui::prelude::*;
use tokio::sync::mpsc;

use t_koma_core::Settings;
use t_koma_db::{KomaDbPool, Operator};

use crate::tui::state::{Category, FocusPane, GateFilter};

use self::state::{
    ContentView, GateEvent, GhostRow, JobViewState, KnowledgeViewState, Metrics, OperatorView,
    PromptState, SelectionModal, SessionViewState,
};

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
    ghosts: Vec<GhostRow>,
    operator_view: OperatorView,
    config_scroll: u16,

    prompt: PromptState,
    modal: Option<SelectionModal>,
    content_view: ContentView,

    job_view: JobViewState,
    job_detail_scroll: u16,
    session_view: SessionViewState,
    knowledge_view: KnowledgeViewState,

    gate_connected: bool,
    gate_paused: bool,
    gate_rows: Vec<state::GateRow>,
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
        let disk_toml = util::load_disk_config().unwrap_or_else(|| settings_toml.clone());

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
            modal: None,
            content_view: ContentView::default(),

            job_view: JobViewState::default(),
            job_detail_scroll: 0,
            session_view: SessionViewState::default(),
            knowledge_view: KnowledgeViewState::default(),

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
                "Set Access Level".to_string(),
                "Set Rate Limits".to_string(),
                "Disable Rate Limits".to_string(),
                "Toggle Workspace Escape".to_string(),
            ],
            Category::Ghosts => {
                vec![
                    "Sessions".to_string(),
                    "List All".to_string(),
                    "New Ghost".to_string(),
                    "Delete".to_string(),
                ]
            }
            Category::Jobs => {
                let mut opts = vec!["All Recent".to_string()];
                for g in &self.ghosts {
                    opts.push(format!("Ghost: {}", g.ghost.name));
                }
                opts
            }
            Category::Knowledge => {
                vec![
                    "Recent Notes".to_string(),
                    "Search".to_string(),
                    "Index Stats".to_string(),
                ]
            }
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
}
