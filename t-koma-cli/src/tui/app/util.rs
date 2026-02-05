use std::fs;

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use t_koma_core::Settings;

pub(super) fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Percentage((100 - percent_y) / 2),
            ratatui::layout::Constraint::Percentage(percent_y),
            ratatui::layout::Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Horizontal)
        .constraints([
            ratatui::layout::Constraint::Percentage((100 - percent_x) / 2),
            ratatui::layout::Constraint::Percentage(percent_x),
            ratatui::layout::Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

pub(super) fn ws_url_for_cli(ws_url: &str) -> String {
    match url::Url::parse(ws_url) {
        Ok(mut url) => {
            url.query_pairs_mut().append_pair("client", "cli");
            url.to_string()
        }
        Err(_) => ws_url.to_string(),
    }
}

pub(super) fn load_disk_config() -> Option<String> {
    let path = Settings::config_path().ok()?;
    fs::read_to_string(path).ok()
}

pub(super) fn shell_quote(value: &str) -> String {
    let escaped = value.replace('\'', "'\\''");
    format!("'{}'", escaped)
}

pub(super) fn glow_color(tick: usize) -> Color {
    let phase = tick % 200;
    let up = if phase <= 100 { phase } else { 200 - phase } as u8;
    let boost = (up as u16 * 90 / 100) as u8;
    Color::Rgb(0, 160 + boost, 170 + boost / 2)
}

pub(super) fn pulse_red(tick: usize) -> Color {
    let phase = tick % 200;
    let up = if phase <= 100 { phase } else { 200 - phase } as u8;
    let boost = (up as u16 * 130 / 100) as u8;
    Color::Rgb(120 + boost, 10, 10)
}

pub(super) fn border_glow(has_focus: bool, tick: usize) -> Style {
    if has_focus {
        Style::default()
            .fg(glow_color(tick))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Rgb(45, 60, 68))
    }
}

pub(super) fn marquee_text(text: &str, width: usize, offset: usize) -> String {
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

pub(super) fn highlight_toml(content: &str) -> Vec<Line<'static>> {
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

pub(super) fn highlight_toml_with_diff(content: &str, disk_content: &str) -> Vec<Line<'static>> {
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

        let mut rendered = highlight_toml(line)
            .into_iter()
            .next()
            .unwrap_or_else(|| Line::from(""));
        rendered.spans.insert(
            0,
            Span::styled(format!("{:>4} {} ", idx + 1, marker), marker_style),
        );
        lines.push(rendered);
    }

    lines
}

pub(super) fn markdown_to_lines(message: &str) -> Vec<Line<'static>> {
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

pub(super) fn truncate_for_cell(s: &str, max_chars: usize) -> String {
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

pub(super) fn truncate_for_message(s: &str, max_chars: usize) -> String {
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
