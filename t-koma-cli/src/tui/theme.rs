use ratatui::style::{Color, Modifier, Style};

pub fn border(has_focus: bool) -> Style {
    if has_focus {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

pub fn selected() -> Style {
    Style::default()
        .bg(Color::Rgb(0, 60, 90))
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}

pub fn header_title() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

pub fn status_ok() -> Style {
    Style::default().fg(Color::Green)
}

pub fn status_err() -> Style {
    Style::default().fg(Color::Red)
}
