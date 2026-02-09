use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use t_koma_db::ContentBlock;

use crate::tui::{
    state::{Category, FocusPane},
    theme,
};

use super::super::{
    TuiApp,
    state::ContentView,
    util::{border_glow, highlight_toml_with_diff, markdown_to_lines},
};

impl TuiApp {
    pub(super) fn draw_content(&self, frame: &mut Frame, area: Rect) {
        let title = self.content_title();
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_glow(
                self.focus == FocusPane::Content,
                self.anim_tick,
            ));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        match &self.content_view {
            ContentView::List => self.draw_list_content(frame, inner),
            ContentView::JobDetail { .. } => self.draw_job_detail(frame, inner),
            ContentView::KnowledgeDetail { .. } => self.draw_knowledge_detail(frame, inner),
            ContentView::GhostSessions { ghost_name, .. } => {
                self.draw_ghost_sessions(frame, inner, ghost_name)
            }
            ContentView::SessionMessages { .. } => self.draw_session_messages(frame, inner),
        }
    }

    fn content_title(&self) -> String {
        match &self.content_view {
            ContentView::List => "Content".to_string(),
            ContentView::JobDetail { .. } => "Job Detail".to_string(),
            ContentView::KnowledgeDetail { .. } => {
                let title = self
                    .knowledge_view
                    .detail_title
                    .as_deref()
                    .unwrap_or("Note");
                format!("Knowledge: {}", title)
            }
            ContentView::GhostSessions { ghost_name, .. } => {
                format!("Sessions: {}", ghost_name)
            }
            ContentView::SessionMessages { ghost_name, .. } => {
                format!("Messages: {}", ghost_name)
            }
        }
    }

    fn draw_list_content(&self, frame: &mut Frame, inner: Rect) {
        match self.selected_category() {
            Category::Config => self.draw_config_content(frame, inner),
            Category::Operators => self.draw_operators_content(frame, inner),
            Category::Ghosts => self.draw_ghosts_content(frame, inner),
            Category::Gate => self.draw_gate_content(frame, inner),
            Category::Jobs => self.draw_jobs_list(frame, inner),
            Category::Knowledge => self.draw_knowledge_list(frame, inner),
        }
    }

    fn draw_config_content(&self, frame: &mut Frame, inner: Rect) {
        let mut lines = vec![];
        if self.settings_dirty {
            lines.push(Line::from(Span::styled(
                "Unsaved changes. Use option: Save (required after changes).",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        }

        lines.extend(highlight_toml_with_diff(
            &self.settings_toml,
            &self.disk_toml,
        ));

        let text = Text::from(lines);
        let p = Paragraph::new(text)
            .scroll((self.config_scroll, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(p, inner);
    }

    fn draw_operators_content(&self, frame: &mut Frame, inner: Rect) {
        let items: Vec<ListItem> = self
            .operators
            .iter()
            .enumerate()
            .map(|(idx, op)| {
                let icon = match op.status {
                    t_koma_db::OperatorStatus::Approved => "OK",
                    t_koma_db::OperatorStatus::Pending => "PD",
                    t_koma_db::OperatorStatus::Denied => "NO",
                };
                let access = match op.access_level {
                    t_koma_db::OperatorAccessLevel::PuppetMaster => "PM",
                    t_koma_db::OperatorAccessLevel::Standard => "STD",
                };
                let rate = match (op.rate_limit_5m_max, op.rate_limit_1h_max) {
                    (None, None) => "RL:none".to_string(),
                    (Some(rate_5m), Some(rate_1h)) => {
                        format!("RL:{}/5m {}/1h", rate_5m, rate_1h)
                    }
                    (Some(rate_5m), None) => format!("RL:{}/5m off", rate_5m),
                    (None, Some(rate_1h)) => format!("RL:off {}/1h", rate_1h),
                };
                let escape = if op.access_level == t_koma_db::OperatorAccessLevel::PuppetMaster
                    || op.allow_workspace_escape
                {
                    "WE:allow"
                } else {
                    "WE:block"
                };
                let text = format!(
                    "{} {} [{}] {} {} {} {}",
                    icon, op.name, op.platform, access, rate, escape, op.id
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

    fn draw_ghosts_content(&self, frame: &mut Frame, inner: Rect) {
        let items: Vec<ListItem> = self
            .ghosts
            .iter()
            .enumerate()
            .map(|(idx, ghost)| {
                let heartbeat = ghost.heartbeat.clone().unwrap_or_else(|| "-".to_string());
                let mut item = ListItem::new(format!(
                    "{} | owner={} | heartbeat={} | cwd={}",
                    ghost.ghost.name,
                    ghost.ghost.owner_operator_id,
                    heartbeat,
                    ghost.ghost.cwd.clone().unwrap_or_else(|| "-".to_string())
                ));
                if idx == self.content_idx && self.focus == FocusPane::Content {
                    item = item.style(theme::selected());
                }
                item
            })
            .collect();
        frame.render_widget(List::new(items), inner);
    }

    fn draw_gate_content(&self, frame: &mut Frame, inner: Rect) {
        let lines = self.filtered_gate_lines_colored();
        let p = Paragraph::new(Text::from(lines))
            .scroll((self.gate_scroll, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(p, inner);
    }

    // ── Jobs ─────────────────────────────────────────────────────────

    fn draw_jobs_list(&self, frame: &mut Frame, inner: Rect) {
        if self.job_view.summaries.is_empty() {
            let p = Paragraph::new("No job logs found").style(Style::default().fg(Color::DarkGray));
            frame.render_widget(p, inner);
            return;
        }

        let items: Vec<ListItem> = self
            .job_view
            .summaries
            .iter()
            .enumerate()
            .map(|(idx, job)| {
                let (status_icon, status_color) = job_status_style(job.status.as_deref());
                let kind_str = format!("{:?}", job.job_kind);
                let dur_str = job
                    .finished_at
                    .map(|f| {
                        let secs = (f - job.started_at) as f64 / 1000.0;
                        format!("{:.1}s", secs)
                    })
                    .unwrap_or_default();
                let ghost = self
                    .ghosts
                    .iter()
                    .find(|g| g.ghost.id == job.ghost_id)
                    .map(|g| g.ghost.name.as_str())
                    .unwrap_or("?");
                let sess_short = &job.session_id[..12.min(job.session_id.len())];
                let preview = job
                    .last_message
                    .as_deref()
                    .map(|m| truncate_snippet(m, 60))
                    .unwrap_or_default();

                let line1 = format!(
                    "{} {:12} {:12} {} {:8} {}",
                    status_icon,
                    kind_str,
                    ghost,
                    sess_short,
                    dur_str,
                    job.status.as_deref().unwrap_or("-"),
                );
                let mut lines = vec![Line::from(line1)];
                if !preview.is_empty() {
                    lines.push(Line::styled(
                        format!("          \"{}\"", preview),
                        Style::default().fg(Color::DarkGray),
                    ));
                }

                let mut item = ListItem::new(Text::from(lines));
                if idx == self.content_idx && self.focus == FocusPane::Content {
                    item = item.style(theme::selected());
                } else {
                    item = item.style(Style::default().fg(status_color));
                }
                item
            })
            .collect();

        frame.render_widget(List::new(items), inner);
    }

    fn draw_job_detail(&self, frame: &mut Frame, inner: Rect) {
        let Some(job) = &self.job_view.detail else {
            let p = Paragraph::new("Loading job...").style(Style::default().fg(Color::DarkGray));
            frame.render_widget(p, inner);
            return;
        };

        let ghost = self
            .ghosts
            .iter()
            .find(|g| g.ghost.id == job.ghost_id)
            .map(|g| g.ghost.name.as_str())
            .unwrap_or("?");

        let (_, status_color) = job_status_style(job.status.as_deref());
        let dur_str = job
            .finished_at
            .map(|f| {
                let secs = (f - job.started_at) as f64 / 1000.0;
                format!("{:.1}s", secs)
            })
            .unwrap_or_else(|| "in-progress".to_string());

        let mut lines: Vec<Line> = vec![
            Line::styled(
                format!("─── JOB: {:?} ─── {} ───", job.job_kind, ghost,),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::styled(
                format!(
                    "Status: {}  Duration: {}  Session: {}",
                    job.status.as_deref().unwrap_or("-"),
                    dur_str,
                    &job.session_id[..16.min(job.session_id.len())],
                ),
                Style::default().fg(status_color),
            ),
            Line::from(""),
        ];

        for entry in &job.transcript {
            let role_str = format!("{:?}", entry.role);
            let model_suffix = entry
                .model
                .as_deref()
                .map(|m| format!(" ({})", m))
                .unwrap_or_default();
            let role_color = match entry.role {
                t_koma_db::MessageRole::Operator => Color::Yellow,
                t_koma_db::MessageRole::Ghost => Color::Cyan,
            };

            lines.push(Line::styled(
                format!("─── {}{} ───", role_str.to_uppercase(), model_suffix),
                Style::default().fg(role_color).add_modifier(Modifier::BOLD),
            ));

            for block in &entry.content {
                match block {
                    ContentBlock::Text { text } => {
                        for line in text.lines() {
                            lines.push(Line::from(line.to_string()));
                        }
                    }
                    ContentBlock::ToolUse { name, input, .. } => {
                        let input_str = serde_json::to_string(input).unwrap_or_default();
                        let short_input = truncate_snippet(&input_str, 80);
                        lines.push(Line::styled(
                            format!("  ⚙ {}({})", name, short_input),
                            Style::default().fg(Color::Magenta),
                        ));
                    }
                    ContentBlock::ToolResult {
                        content, is_error, ..
                    } => {
                        let prefix = if *is_error == Some(true) {
                            "  ✗ "
                        } else {
                            "  ┆ "
                        };
                        let color = if *is_error == Some(true) {
                            Color::Red
                        } else {
                            Color::DarkGray
                        };
                        let short = truncate_snippet(content, 120);
                        lines.push(Line::styled(
                            format!("{}{}", prefix, short),
                            Style::default().fg(color),
                        ));
                    }
                }
            }
            lines.push(Line::from(""));
        }

        let p = Paragraph::new(Text::from(lines))
            .scroll((self.job_detail_scroll, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(p, inner);
    }

    // ── Knowledge ────────────────────────────────────────────────────

    fn draw_knowledge_list(&self, frame: &mut Frame, inner: Rect) {
        if self.knowledge_view.notes.is_empty() {
            let p =
                Paragraph::new("No knowledge entries").style(Style::default().fg(Color::DarkGray));
            frame.render_widget(p, inner);
            return;
        }

        let items: Vec<ListItem> = self
            .knowledge_view
            .notes
            .iter()
            .enumerate()
            .map(|(idx, note)| {
                let tags_str = if note.tags.is_empty() {
                    String::new()
                } else {
                    format!(
                        " {}",
                        note.tags
                            .iter()
                            .map(|t| format!("#{}", t))
                            .collect::<Vec<_>>()
                            .join(" ")
                    )
                };
                let text = format!(
                    "[{}] {}{}\n        {}",
                    note.entry_type.to_uppercase(),
                    note.title,
                    tags_str,
                    truncate_snippet(&note.snippet, 60),
                );
                let mut item = ListItem::new(Text::from(text));
                if idx == self.content_idx && self.focus == FocusPane::Content {
                    item = item.style(theme::selected());
                }
                item
            })
            .collect();

        frame.render_widget(List::new(items), inner);
    }

    fn draw_knowledge_detail(&self, frame: &mut Frame, inner: Rect) {
        let body = self
            .knowledge_view
            .detail_body
            .as_deref()
            .unwrap_or("Loading...");

        let lines = markdown_to_lines(body);
        let p = Paragraph::new(Text::from(lines))
            .scroll((self.knowledge_view.scroll, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(p, inner);
    }

    // ── Sessions ─────────────────────────────────────────────────────

    fn draw_ghost_sessions(&self, frame: &mut Frame, inner: Rect, ghost_name: &str) {
        if self.session_view.sessions.is_empty() {
            let p = Paragraph::new(format!("No sessions for {}", ghost_name))
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(p, inner);
            return;
        }

        let items: Vec<ListItem> = self
            .session_view
            .sessions
            .iter()
            .enumerate()
            .map(|(idx, sess)| {
                let active_marker = if sess.is_active { "▶" } else { " " };
                let text = format!(
                    "{} {}  {} msgs",
                    active_marker,
                    &sess.id[..16.min(sess.id.len())],
                    sess.message_count,
                );
                let style = if idx == self.content_idx && self.focus == FocusPane::Content {
                    theme::selected()
                } else if sess.is_active {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(text).style(style)
            })
            .collect();

        frame.render_widget(List::new(items), inner);
    }

    fn draw_session_messages(&self, frame: &mut Frame, inner: Rect) {
        if self.session_view.messages.is_empty() {
            let p = Paragraph::new("No messages").style(Style::default().fg(Color::DarkGray));
            frame.render_widget(p, inner);
            return;
        }

        let mut lines: Vec<Line> = Vec::new();
        for msg in &self.session_view.messages {
            let role_color = match msg.role {
                t_koma_db::MessageRole::Operator => Color::Yellow,
                t_koma_db::MessageRole::Ghost => Color::Cyan,
            };
            let model_suffix = msg
                .model
                .as_deref()
                .map(|m| format!(" ({})", m))
                .unwrap_or_default();

            lines.push(Line::styled(
                format!("─── {:?}{} ───", msg.role, model_suffix),
                Style::default().fg(role_color).add_modifier(Modifier::BOLD),
            ));

            for block in &msg.content {
                match block {
                    ContentBlock::Text { text } => {
                        for line in text.lines() {
                            lines.push(Line::from(line.to_string()));
                        }
                    }
                    ContentBlock::ToolUse { name, input, .. } => {
                        let input_str = serde_json::to_string(input).unwrap_or_default();
                        let short = truncate_snippet(&input_str, 80);
                        lines.push(Line::styled(
                            format!("  ⚙ {}({})", name, short),
                            Style::default().fg(Color::Magenta),
                        ));
                    }
                    ContentBlock::ToolResult {
                        content, is_error, ..
                    } => {
                        let prefix = if *is_error == Some(true) {
                            "  ✗ "
                        } else {
                            "  ┆ "
                        };
                        let color = if *is_error == Some(true) {
                            Color::Red
                        } else {
                            Color::DarkGray
                        };
                        let short = truncate_snippet(content, 120);
                        lines.push(Line::styled(
                            format!("{}{}", prefix, short),
                            Style::default().fg(color),
                        ));
                    }
                }
            }
            lines.push(Line::from(""));
        }

        let p = Paragraph::new(Text::from(lines))
            .scroll((self.session_view.scroll, 0))
            .wrap(Wrap { trim: false });
        frame.render_widget(p, inner);
    }
}

fn job_status_style(status: Option<&str>) -> (&'static str, Color) {
    match status {
        Some("ran") | Some("ok") => ("✓", Color::Green),
        Some(s) if s.starts_with("error") => ("✗", Color::Red),
        Some("skipped") | Some("suppressed") => ("·", Color::Yellow),
        _ => ("?", Color::DarkGray),
    }
}

fn truncate_snippet(s: &str, max: usize) -> String {
    let first_line = s.lines().next().unwrap_or("");
    let chars: Vec<char> = first_line.chars().collect();
    if chars.len() <= max {
        first_line.to_string()
    } else {
        let kept: String = chars.into_iter().take(max - 1).collect();
        format!("{kept}…")
    }
}
