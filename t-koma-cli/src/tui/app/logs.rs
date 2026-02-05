use chrono::Utc;
use futures::StreamExt;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;

use crate::tui::state::GateFilter;

use super::{
    state::{GateEvent, GateRow},
    util::{markdown_to_lines, truncate_for_cell, truncate_for_message},
    TuiApp,
};

impl TuiApp {
    pub(super) fn start_logs_stream(&mut self) {
        let logs_url = self.settings.ws_url().replace("/ws", "/logs");
        let (tx, rx) = mpsc::unbounded_channel();
        self.gate_rx = Some(rx);

        tokio::spawn(async move {
            loop {
                let connection = connect_async(&logs_url).await;
                let Ok((stream, _)) = connection else {
                    let _ = tx.send(GateEvent::Status(false));
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
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

                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });
    }

    pub(super) fn filtered_gate_lines_colored(&self) -> Vec<Line<'static>> {
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

        let mut lines = vec![Line::from(vec![Span::styled(
            "filters: [1]all [2]gateway [3]ghost [4]operator [5]transport [6]warn/error",
            Style::default().fg(Color::DarkGray),
        )])];

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
