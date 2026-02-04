# T-KOMA TUI - Full Implementation Spec

## Overview
A ratatui-based cyberpunk TUI for managing the T-KOMA gateway. This is a from-scratch implementation guide.

**Goal**: Three-pane layout with functional database operations, WebSocket integration, and external editor support.

---

## Architecture Overview

### Data Flow
```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   TUI App   │────▶│  Database   │◀────│   Gateway   │
│  (ratatui)  │     │  (SQLite)   │     │  (optional) │
└─────────────┘     └─────────────┘     └─────────────┘
       │                                            
       │ WebSocket (logs/chat only)                
       ▼                                            
┌─────────────┐                                     
│   Gateway   │                                     
│  (if running)│                                    
└─────────────┘                                     
```

**Critical**: The TUI works independently of the gateway. All CRUD operations go directly to the database. WebSocket is ONLY for:
- Log tailing (when gateway is running)
- Chat functionality (when gateway is running)

---

## Part 1: Layout System

### Requirements
- **Header**: 2 lines (title, status, model info)
- **Main**: Three columns
  - Categories: 22 chars wide, fixed
  - Options: 30 chars wide, fixed (not present for Gate)
  - Content: remaining width

### Implementation
```rust
// layout/mod.rs
pub fn main_layout(area: Rect) -> (Rect, Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(10)])
        .split(area);
    (chunks[0], chunks[1])
}

pub fn sidebar_layout(area: Rect) -> (Rect, Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(22), Constraint::Min(20)])
        .split(area);
    (chunks[0], chunks[1])
}

pub fn content_layout(area: Rect) -> (Rect, Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(30), Constraint::Min(20)])
        .split(area);
    (chunks[0], chunks[1])
}
```

### Common Issues
1. **Border bleeding**: Always use `block.inner(area)` to get inner content area
2. **Focus highlighting**: Each pane needs `has_focus` field updated in `draw_with_areas()`

---

## Part 2: State Management

### AppState Fields
```rust
pub struct AppState {
    pub category: Category,           // Current category
    pub connection_status: ConnectionStatus, // WebSocket status
    pub gateway_url: String,          // ws://127.0.0.1:3000/ws
    pub ws_tx: Option<mpsc::UnboundedSender<WsMessage>>,
    pub should_exit: bool,
    pub show_help: bool,
    pub operators: Vec<OperatorInfo>, // Loaded from DB
    pub ghosts: Vec<GhostInfo>,       // Loaded from DB
    pub pending_count: u32,           // Pending operators
    pub config_content: String,       // TOML string
    pub bind_addr: String,            // "127.0.0.1:3000"
    pub current_model: String,        // "kimi:kimi25"
}
```

### Database Loading (CRITICAL)
**Where**: `App::load_initial_data()`

```rust
async fn load_initial_data(&mut self) {
    match t_koma_db::KomaDbPool::new().await {
        Ok(db) => {
            // OPERATORS: Use list_all(), NOT list_by_status(Approved)
            match t_koma_db::OperatorRepository::list_all(db.pool()).await {
                Ok(ops) => { /* convert to OperatorInfo */ }
                Err(e) => warn!("Failed to load operators: {}", e),
            }
            
            // GHOSTS: Use list_all()
            match t_koma_db::GhostRepository::list_all(db.pool()).await {
                Ok(ghosts) => { /* convert to GhostInfo */ }
                Err(e) => warn!("Failed to load ghosts: {}", e),
            }
        }
        Err(e) => warn!("Database connection failed: {}", e),
    }
}
```

**Issue**: Using `list_by_status(Approved)` returns empty if no operators are approved.

### Config Loading (CRITICAL)
**Where**: `App::load_config()`

```rust
async fn load_config(&mut self) {
    match t_koma_core::Settings::load() {
        Ok(settings) => {
            self.state.current_model = settings.default_model.clone();
            self.state.bind_addr = settings.gateway.host.clone() + ":" + &settings.gateway.port.to_string();
            
            // Use to_toml(), NOT format!("{:?}", settings)
            self.state.config_content = settings.to_toml();
        }
        Err(e) => warn!("Failed to load config: {}", e),
    }
}
```

**Issue**: `format!("{:?}", settings)` produces debug output, not valid TOML.

---

## Part 3: Event Handling

### Focus System
```rust
pub enum Focus {
    Left,   // Categories
    Middle, // Options (Config/Operators/Ghosts)
    Right,  // Content
}
```

### Event Routing
```rust
async fn handle_event(&mut self, event: Event) -> Result<bool, Box<dyn Error>> {
    // Global shortcuts first
    if let Event::Key(key) = &event {
        if key.kind == KeyEventKind::Press {
            match key.code {
                KeyCode::Char('q') if self.focus == Focus::Left => {
                    self.state.exit();
                    return Ok(true);
                }
                KeyCode::Char('?') => {
                    self.state.show_help = !self.state.show_help;
                    return Ok(false);
                }
                _ => {}
            }
        }
    }
    
    // Route to focused pane
    match self.focus {
        Focus::Left => self.categories_pane.handle_event(&event),
        Focus::Middle => self.handle_middle_event(&event).await,
        Focus::Right => self.handle_right_event(&event).await,
    }
}
```

### Navigation Keys
| Key | Action |
|-----|--------|
| ↑/↓ or j/k | Navigate within pane |
| ←/→ or h/l | Switch focus |
| Tab | Cycle focus forward |
| Enter | Activate selected item |
| g/c/o/h | Direct category jump |

---

## Part 4: Config Pane

### Structure
- **Options (left)**: Add Model, Set Default, Toggle Discord, Edit in Editor, Reload, Save
- **Content (right)**: Syntax-highlighted TOML

### Syntax Highlighting
```rust
// syntax.rs - Use syntect
use syntect::easy::HighlightLines;
use syntect::parsing::SyntaxSet;
use syntect::highlighting::{ThemeSet, Style};
use syntect::util::LinesWithEndings;

pub fn highlight_toml(content: &str) -> Vec<Line<'_>> {
    let syntax = SYNTAX_SET.find_syntax_by_extension("toml")
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
    let theme = &THEME_SET.themes["InspiredGitHub"];
    let mut highlighter = HighlightLines::new(syntax, theme);
    
    LinesWithEndings::from(content)
        .map(|line| {
            let highlighted = highlighter.highlight_line(line, &SYNTAX_SET).unwrap_or_default();
            Line::from(highlighted.into_iter().map(|(style, text)| {
                Span::styled(text.to_string(), syntect_style_to_ratatui(style))
            }).collect::<Vec<_>>())
        })
        .collect()
}
```

### Editor Integration (CRITICAL)
**This is where things break easily.**

```rust
async fn edit_config_in_editor(&mut self) {
    let config_path = t_koma_core::Settings::config_path().unwrap();
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    
    // STEP 1: Save terminal state
    self.terminal.flush().ok();
    crossterm::terminal::disable_raw_mode().ok();
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    ).ok();
    
    // STEP 2: Run editor
    let status = tokio::process::Command::new(&editor)
        .arg(&config_path)
        .status()
        .await;
    
    // STEP 3: Restore terminal
    crossterm::terminal::enable_raw_mode().ok();
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    ).ok();
    
    // STEP 4: Clear and redraw
    self.terminal.clear().ok();
    
    // STEP 5: Reload config
    if status.is_ok() {
        self.load_config().await;
    }
}
```

**Issue**: Without proper terminal restoration, the screen is garbled after editor exits.

---

## Part 5: Operators Pane

### Structure
- **Options (left)**: List All, Add Operator, Pending Approvals
- **Content (right)**: Operator list with status icons

### OperatorInfo
```rust
pub struct OperatorInfo {
    pub id: String,
    pub name: String,
    pub status: OpStatus,  // Approved, Pending, Denied
    pub platform: String,
}
```

### Actions
```rust
async fn approve_selected_operator(&mut self) {
    if let Some(op) = self.operators_pane.selected_operator() {
        match t_koma_db::KomaDbPool::new().await {
            Ok(db) => {
                let _ = t_koma_db::OperatorRepository::update_status(
                    db.pool(),
                    &op.id,
                    t_koma_db::OperatorStatus::Approved
                ).await;
                self.load_initial_data().await; // Refresh
            }
            Err(_) => {}
        }
    }
}

async fn delete_selected_operator(&mut self) {
    if let Some(op) = self.operators_pane.selected_operator() {
        match t_koma_db::KomaDbPool::new().await {
            Ok(db) => {
                let _ = t_koma_db::OperatorRepository::delete(
                    db.pool(),
                    &op.id
                ).await;
                self.load_initial_data().await; // Refresh
            }
            Err(_) => {}
        }
    }
}
```

---

## Part 6: Ghosts Pane

### Structure
- **Options (left)**: List All, New Ghost, Delete
- **Content (right)**: Ghost list with model info

### GhostInfo
```rust
pub struct GhostInfo {
    pub id: String,
    pub name: String,
    pub session_count: usize,
    pub model: Option<String>,
}
```

### Chat Integration
When Enter pressed on a ghost:
```rust
async fn start_chat_with_selected_ghost(&mut self) {
    if let Some(ghost) = self.ghosts_pane.selected_ghost() {
        self.chat_pane.start_chat(&ghost.name);
        // WebSocket will be used for actual chat
    }
}
```

---

## Part 7: Chat Pane (Full-Screen Overlay)

### Behavior
- **Activation**: Full-screen overlay on top of everything
- **Input**: 'i' to enter typing mode, Enter to send
- **Exit**: q or Esc
- **Messages**: Append to list with timestamps

### Implementation
```rust
pub struct ChatPane {
    active: bool,
    typing: bool,
    ghost_name: String,
    input: String,
    messages: Vec<ChatMessage>,
}

pub fn handle_event(&mut self, event: &Event) -> bool {
    if !self.active { return false; }
    
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => {
                    self.end_chat();
                    true
                }
                KeyCode::Char('i') if !self.typing => {
                    self.typing = true;
                    true
                }
                KeyCode::Enter if self.typing => {
                    self.send_message();
                    true
                }
                KeyCode::Backspace if self.typing => {
                    self.input.pop();
                    true
                }
                KeyCode::Char(c) if self.typing => {
                    self.input.push(c);
                    true
                }
                _ => false
            }
        }
        _ => false
    }
}
```

### WebSocket Integration
```rust
fn send_message(&mut self) {
    if !self.input.is_empty() {
        let content = self.input.clone();
        self.input.clear();
        
        // Add to local messages immediately
        self.add_message("You", &content);
        
        // Send via WebSocket if connected
        if let Some(tx) = &self.ws_tx {
            let msg = WsMessage::Chat {
                ghost_name: self.ghost_name.clone(),
                session_id: None, // Create new session
                content,
            };
            let _ = tx.send(msg);
        }
    }
}
```

---

## Part 8: Gate Pane

### Structure
- No options sidebar (uses full content area)
- Status header
- Restart button
- Log tailing area

### Log Tailing
```rust
pub struct GatePane {
    logs: Vec<LogEntry>,
    paused: bool,
    search_mode: bool,
    search_query: String,
    source_filter: Option<LogSource>,
    scroll_offset: usize,
}
```

### Key Handlers
| Key | Action |
|-----|--------|
| r | Restart gateway |
| / | Enter search mode |
| Esc | Exit search mode |
| Space | Pause/resume |
| c | Clear logs |
| 1-5 | Filter by source |

---

## Part 9: WebSocket Client

### Connection
```rust
pub async fn connect(url: &str) -> Result<(WsTx, WsRx), WsClientError> {
    let (ws_stream, _) = connect_async(url).await?;
    let (write, read) = ws_stream.split();
    let (tx, rx) = mpsc::unbounded_channel();
    
    // Spawn writer task
    tokio::spawn(async move {
        let mut write = write;
        let mut rx = rx;
        while let Some(msg) = rx.recv().await {
            let _ = write.send(msg).await;
        }
    });
    
    Ok((tx, read))
}
```

### Message Handling
```rust
fn handle_ws_message(&mut self, msg: Result<Message, tungstenite::Error>) {
    match msg {
        Ok(Message::Text(text)) => {
            if let Ok(response) = serde_json::from_str::<WsResponse>(&text) {
                match response {
                    WsResponse::LogEntry(entry) => {
                        self.gate_pane.add_log(entry);
                    }
                    WsResponse::ChatResponse { content, .. } => {
                        self.chat_pane.add_message("Ghost", &content);
                    }
                    _ => {}
                }
            }
        }
        Ok(Message::Close(_)) => {
            self.state.connection_status = ConnectionStatus::Disconnected;
        }
        Err(_) => {
            self.state.connection_status = ConnectionStatus::Disconnected;
        }
        _ => {}
    }
}
```

---

## Part 10: Logging Setup (CRITICAL)

**Issue**: Tracing to stdout interferes with ratatui's terminal control.

**Solution**: Log to file in TUI mode:

```rust
// main.rs
fn setup_logging(is_tui_mode: bool) {
    if is_tui_mode {
        let log_path = std::path::PathBuf::from(
            std::env::var("TEMP")
                .or_else(|_| std::env::var("TMP"))
                .unwrap_or_else(|_| "/tmp".to_string())
        ).join("t-koma-cli.log");
        
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .expect("Failed to open log file");
            
        tracing_subscriber::fmt()
            .with_env_filter("info")
            .with_writer(move || -> Box<dyn std::io::Write + Send> {
                Box::new(log_file.try_clone().expect("Failed to clone log file"))
            })
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter("warn")
            .init();
    }
}
```

---

## Part 11: Testing

### Unit Tests
Each pane should have tests for:
- Navigation (next/prev)
- Selection state
- Event handling

Example:
```rust
#[test]
fn test_categories_navigation() {
    let mut pane = CategoriesPane::new();
    assert_eq!(pane.selected(), Category::Gate);
    
    pane.next();
    assert_eq!(pane.selected(), Category::Config);
    
    pane.prev();
    assert_eq!(pane.selected(), Category::Gate);
}
```

### Integration Tests
- Config loading
- Database connectivity
- Theme/category enum correctness

---

## Common Pitfalls

### 1. Database Not Connected
- **Symptom**: Empty operator/ghost lists
- **Fix**: Check `KomaDbPool::new().await` result, log errors

### 2. Terminal Garbled After Editor
- **Symptom**: Screen messed up after closing $EDITOR
- **Fix**: Proper disable/enable raw mode, leave/enter alternate screen

### 3. Logs Appearing in UI
- **Symptom**: Log messages over UI
- **Fix**: Use file-based logging for TUI mode

### 4. Focus Not Visible
- **Symptom**: Can't tell which pane is focused
- **Fix**: Update `has_focus` in `draw_with_areas()`, use different border colors

### 5. Enter Key Not Working
- **Symptom**: Enter does nothing
- **Fix**: Add explicit Enter handler that calls `activate_selected()`

### 6. Config Shows Debug Format
- **Symptom**: Config shows `Settings { ... }` instead of TOML
- **Fix**: Use `settings.to_toml()`, not `format!("{:?}", settings)`

---

## File Structure

```
t-koma-cli/src/
├── main.rs           # Entry point, logging setup
├── app.rs            # Main app loop, event routing
├── app_state.rs      # AppState struct
├── client.rs         # WebSocket client
├── layout/
│   └── mod.rs        # Layout helpers
├── syntax.rs         # TOML highlighting
├── theme.rs          # Colors, icons, Category enum
├── which_key.rs      # Help overlay
├── gateway_process.rs # Gateway control
└── panes/
    ├── mod.rs        # Pane trait, Focus enum
    ├── categories.rs # Category sidebar
    ├── config.rs     # Config pane
    ├── operators.rs  # Operators pane
    ├── ghosts.rs     # Ghosts pane
    ├── gate.rs       # Gate/log pane
    └── chat.rs       # Chat overlay
```

---

## Dependencies

```toml
[dependencies]
ratatui = "0.29"
crossterm = "0.28"
tokio = { version = "1", features = ["full"] }
tokio-tungstenite = "0.24"
syntect = "0.23"
tracing = "0.1"
tracing-subscriber = "0.3"

# Workspace dependencies
t-koma-core = { path = "../t-koma-core" }
t-koma-db = { path = "../t-koma-db" }
```

---

## Checklist for Implementation

- [ ] Layout system works (three panes visible)
- [ ] Categories render with correct icons
- [ ] Header shows connection status
- [ ] Database loads operators on startup
- [ ] Database loads ghosts on startup
- [ ] Config loads and displays as TOML
- [ ] Config has syntax highlighting
- [ ] Focus changes visible (border colors)
- [ ] Navigation works (↑↓←→, hjkl, Tab)
- [ ] Enter activates options
- [ ] Editor opens and returns cleanly
- [ ] WebSocket connects when gateway running
- [ ] Logs appear in Gate pane
- [ ] Chat overlay opens
- [ ] Chat messages send/receive
- [ ] All tests pass
