use crossterm::event::{self, Event, KeyCode, KeyEventKind};

use crate::cli_app::App;

impl App {
    /// Handle events
    pub(crate) async fn handle_events(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(rx) = self.ws_rx_mut()
            && let Ok(msg) = rx.try_recv()
        {
            self.handle_ws_message(msg).await;
        }

        if event::poll(Self::tick_rate())?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            self.handle_key_event(key).await;
        }

        Ok(())
    }

    /// Handle key event
    pub(crate) async fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyModifiers;

        match key.code {
            KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => {
                self.set_should_exit(true);
            }
            KeyCode::Esc => {
                self.set_should_exit(true);
            }
            KeyCode::Enter => {
                self.send_message().await;
            }
            KeyCode::Char(c) => {
                let cursor = self.cursor_position();
                self.input_mut().insert(cursor, c);
                self.set_cursor_position(cursor + 1);
            }
            KeyCode::Backspace => {
                let cursor = self.cursor_position();
                if cursor > 0 {
                    self.set_cursor_position(cursor - 1);
                    let cursor = self.cursor_position();
                    self.input_mut().remove(cursor);
                }
            }
            KeyCode::Left => {
                let cursor = self.cursor_position();
                if cursor > 0 {
                    self.set_cursor_position(cursor - 1);
                }
            }
            KeyCode::Right => {
                let cursor = self.cursor_position();
                if cursor < self.input().len() {
                    self.set_cursor_position(cursor + 1);
                }
            }
            _ => {}
        }
    }
}
