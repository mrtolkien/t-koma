use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::state::{Category, FocusPane, GateFilter};

use super::{
    TuiApp,
    state::{ContentView, PromptKind},
};

impl TuiApp {
    pub(super) async fn handle_key(&mut self, key: KeyEvent) {
        if self.modal.is_some() {
            self.handle_modal_key(key).await;
            return;
        }

        if self.prompt.kind.is_some() {
            self.handle_prompt_key(key).await;
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.should_exit = true,
            KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => self.should_exit = true,
            KeyCode::Esc => self.handle_esc(),
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
            _ => self.handle_category_shortcuts(key).await,
        }
    }

    fn handle_esc(&mut self) {
        if self.content_view != ContentView::List {
            self.pop_content_view();
            return;
        }
        self.should_exit = true;
    }

    fn pop_content_view(&mut self) {
        let parent = match std::mem::take(&mut self.content_view) {
            ContentView::SessionMessages { ghost_name, .. } => {
                let ghost_id = self
                    .ghosts
                    .iter()
                    .find(|g| g.ghost.name == ghost_name)
                    .map(|g| g.ghost.id.clone())
                    .unwrap_or_default();
                ContentView::GhostSessions {
                    ghost_id,
                    ghost_name,
                }
            }
            _ => ContentView::List,
        };
        self.content_view = parent;
        self.content_idx = 0;
        self.session_view.scroll = 0;
        self.knowledge_view.scroll = 0;
        self.job_detail_scroll = 0;
    }

    fn scroll_detail_up(&mut self) {
        match &self.content_view {
            ContentView::JobDetail { .. } => {
                self.job_detail_scroll = self.job_detail_scroll.saturating_sub(1);
            }
            ContentView::KnowledgeDetail { .. } => {
                self.knowledge_view.scroll = self.knowledge_view.scroll.saturating_sub(1);
            }
            _ => {}
        }
    }

    fn scroll_detail_down(&mut self) {
        match &self.content_view {
            ContentView::JobDetail { .. } => {
                self.job_detail_scroll = self.job_detail_scroll.saturating_add(1);
            }
            ContentView::KnowledgeDetail { .. } => {
                self.knowledge_view.scroll = self.knowledge_view.scroll.saturating_add(1);
            }
            _ => {}
        }
    }

    async fn handle_modal_key(&mut self, key: KeyEvent) {
        let Some(modal) = &mut self.modal else {
            return;
        };

        match key.code {
            KeyCode::Esc => {
                self.modal = None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if modal.selected_idx > 0 {
                    modal.selected_idx -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if modal.selected_idx + 1 < modal.items.len() {
                    modal.selected_idx += 1;
                }
            }
            KeyCode::Enter => {
                let modal = self.modal.take().unwrap();
                self.handle_modal_selection(modal).await;
            }
            _ => {}
        }
    }

    async fn navigate_up(&mut self) {
        match self.focus {
            FocusPane::Categories => {
                if self.category_idx > 0 {
                    self.category_idx -= 1;
                    self.options_idx = 0;
                    self.content_idx = 0;
                    self.content_view = ContentView::List;
                    self.sync_selection().await;
                }
            }
            FocusPane::Options => {
                if self.options_idx > 0 {
                    self.options_idx -= 1;
                    self.sync_selection().await;
                }
            }
            FocusPane::Content => match &self.content_view {
                ContentView::List => match self.selected_category() {
                    Category::Config => self.config_scroll = self.config_scroll.saturating_sub(1),
                    Category::Gate => self.gate_scroll = self.gate_scroll.saturating_sub(1),
                    _ => {
                        if self.content_idx > 0 {
                            self.content_idx -= 1;
                        }
                    }
                },
                ContentView::JobDetail { .. } | ContentView::KnowledgeDetail { .. } => {
                    self.scroll_detail_up();
                }
                ContentView::SessionMessages { .. } => {
                    self.session_view.scroll = self.session_view.scroll.saturating_sub(1);
                }
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
                    self.content_view = ContentView::List;
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
            FocusPane::Content => match &self.content_view {
                ContentView::List => match self.selected_category() {
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
                    Category::Jobs => {
                        if self.content_idx + 1 < self.job_view.summaries.len() {
                            self.content_idx += 1;
                        }
                    }
                    Category::Knowledge => {
                        if self.content_idx + 1 < self.knowledge_view.notes.len() {
                            self.content_idx += 1;
                        }
                    }
                },
                ContentView::JobDetail { .. } | ContentView::KnowledgeDetail { .. } => {
                    self.scroll_detail_down();
                }
                ContentView::GhostSessions { .. } => {
                    if self.content_idx + 1 < self.session_view.sessions.len() {
                        self.content_idx += 1;
                    }
                }
                ContentView::SessionMessages { .. } => {
                    self.session_view.scroll = self.session_view.scroll.saturating_add(1);
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
            FocusPane::Content => self.activate_content().await,
        }
    }

    async fn activate_content(&mut self) {
        match &self.content_view {
            ContentView::GhostSessions { .. } => {
                self.drill_into_session_messages().await;
                return;
            }
            ContentView::List => {}
            _ => return,
        }

        match self.selected_category() {
            Category::Operators if self.operator_view == super::state::OperatorView::Pending => {
                self.approve_selected_operator().await;
            }
            Category::Ghosts if self.options_idx == 0 => {
                self.drill_into_ghost_sessions().await;
            }
            Category::Jobs => {
                self.drill_into_job().await;
            }
            Category::Knowledge => {
                self.drill_into_knowledge_entry().await;
            }
            _ => {}
        }
    }

    async fn activate_option(&mut self) {
        match self.selected_category() {
            Category::Config => match self.options_idx {
                0 => self.begin_prompt(PromptKind::AddModel, None, None),
                1 => self.begin_prompt(PromptKind::SetDefaultModel, None, None),
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
                    self.operator_view = super::state::OperatorView::All;
                    self.refresh_operators().await;
                }
                1 => self.begin_prompt(PromptKind::AddOperator, None, None),
                2 => {
                    self.operator_view = super::state::OperatorView::Pending;
                    self.refresh_operators().await;
                }
                3 => {
                    if let Some(op) = self.operators.get(self.content_idx) {
                        self.open_access_level_modal(
                            op.id.clone(),
                            op.name.clone(),
                            op.access_level,
                        );
                    } else {
                        self.status = "No operator selected".to_string();
                    }
                }
                4 => {
                    if let Some(op) = self.operators.get(self.content_idx) {
                        self.begin_prompt(
                            PromptKind::SetOperatorRateLimits,
                            None,
                            Some(op.id.clone()),
                        );
                    } else {
                        self.status = "No operator selected".to_string();
                    }
                }
                5 => {
                    if let Some(op) = self.operators.get(self.content_idx) {
                        let operator_id = op.id.clone();
                        self.disable_operator_rate_limits(&operator_id).await;
                    } else {
                        self.status = "No operator selected".to_string();
                    }
                }
                6 => {
                    if let Some(op) = self.operators.get(self.content_idx) {
                        if op.access_level == t_koma_db::OperatorAccessLevel::PuppetMaster {
                            self.status = "Puppet Master is always allowed to escape the workspace"
                                .to_string();
                            return;
                        }
                        let operator_id = op.id.clone();
                        let allow = !op.allow_workspace_escape;
                        self.set_operator_workspace_escape(&operator_id, allow)
                            .await;
                    } else {
                        self.status = "No operator selected".to_string();
                    }
                }
                _ => {}
            },
            Category::Ghosts => match self.options_idx {
                0 => {
                    self.drill_into_ghost_sessions().await;
                }
                1 => self.refresh_ghosts().await,
                2 => self.begin_prompt(PromptKind::NewGhost, None, None),
                3 => {
                    if let Some(name) = self
                        .ghosts
                        .get(self.content_idx)
                        .map(|g| g.ghost.name.clone())
                    {
                        self.begin_prompt(PromptKind::DeleteGhostConfirmOne, Some(name), None);
                    } else {
                        self.status = "No ghost selected".to_string();
                    }
                }
                _ => {}
            },
            Category::Jobs => match self.options_idx {
                0 => self.refresh_jobs(None).await,
                idx => {
                    let ghost_id = self.ghosts.get(idx - 1).map(|g| g.ghost.id.clone());
                    self.refresh_jobs(ghost_id.as_deref()).await;
                }
            },
            Category::Knowledge => match self.options_idx {
                0 => self.refresh_knowledge_recent().await,
                1 => self.begin_prompt(PromptKind::KnowledgeSearch, None, None),
                _ => {}
            },
            Category::Gate => {}
        }

        self.refresh_metrics().await;
    }

    async fn handle_category_shortcuts(&mut self, key: KeyEvent) {
        if self.selected_category() != Category::Gate {
            if self.selected_category() == Category::Operators
                && self.focus == FocusPane::Content
                && self.operator_view == super::state::OperatorView::Pending
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
            KeyCode::Char('/') => self.begin_prompt(PromptKind::GateSearch, None, None),
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

    fn begin_prompt(
        &mut self,
        kind: PromptKind,
        target_ghost: Option<String>,
        target_operator_id: Option<String>,
    ) {
        self.prompt.kind = Some(kind);
        self.prompt.buffer.clear();
        self.prompt.target_ghost = target_ghost;
        self.prompt.target_operator_id = target_operator_id;
    }

    async fn handle_prompt_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.prompt = super::state::PromptState::default();
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
                let target_operator_id = self.prompt.target_operator_id.clone();
                self.prompt = super::state::PromptState::default();

                match kind {
                    Some(PromptKind::AddOperator) => self.add_operator(&input).await,
                    Some(PromptKind::AddModel) => self.add_model(&input),
                    Some(PromptKind::SetDefaultModel) => self.set_default_model(&input),
                    Some(PromptKind::NewGhost) => self.add_ghost(&input).await,
                    Some(PromptKind::DeleteGhostConfirmOne) => {
                        if input == "DELETE" {
                            self.begin_prompt(PromptKind::DeleteGhostConfirmTwo, target, None);
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
                    Some(PromptKind::SetOperatorRateLimits) => {
                        if let Some(operator_id) = target_operator_id {
                            self.set_operator_rate_limits(&operator_id, &input).await;
                        } else {
                            self.status = "No operator selected".to_string();
                        }
                    }
                    Some(PromptKind::KnowledgeSearch) => {
                        self.search_knowledge(&input).await;
                    }
                    None => {}
                }
            }
            _ => {}
        }
    }

    pub(super) async fn sync_selection(&mut self) {
        match self.selected_category() {
            Category::Operators => {
                self.operator_view = if self.options_idx == 2 {
                    super::state::OperatorView::Pending
                } else {
                    super::state::OperatorView::All
                };
                self.refresh_operators().await;
            }
            Category::Ghosts => self.refresh_ghosts().await,
            Category::Jobs => self.refresh_jobs(None).await,
            Category::Knowledge => self.refresh_knowledge_recent().await,
            Category::Config | Category::Gate => {}
        }
        self.refresh_metrics().await;
    }
}
