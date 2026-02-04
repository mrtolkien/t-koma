# T-KOMA TUI - Full Implementation Spec

## Overview

A ratatui-based cyberpunk TUI for managing the T-KOMA gateway. This is a
from-scratch implementation guide.

**Goal**: Three-pane layout with functional database operations, WebSocket
integration, and external editor support.

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

**Critical**: The TUI works independently of the gateway. All CRUD operations go
directly to the database. WebSocket is ONLY for:

- Log tailing (when gateway is running)
- Chat functionality (when gateway is running)

---

## Part 1: Layout System

### Requirements

- **Header**: 2 lines (title, status, model info)
- **Main**: Three columns
  - Categories: as wide as necessary
  - Options: as wide as necessary (not present for gate)
  - Content: remaining width

### Look and feel

- Use borders for all main sections
- Keep it very cold and technical, give it a cyberpunk vibe
- Use nerd icons wherever it makes sense to improve looks

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
2. **Focus highlighting**: Each pane needs `has_focus` field updated in
   `draw_with_areas()`

### Navigation Keys

| Key        | Action                 |
| ---------- | ---------------------- |
| ↑/↓ or j/k | Navigate within pane   |
| ←/→ or h/l | Switch focus           |
| Tab        | Cycle focus forward    |
| Enter      | Activate selected item |

---

## Part 2: Config Pane

### Structure

- **Options (left)**: Add Model, Set Default, Toggle Discord, Edit in Editor,
  Reload, Save
- **Content (right)**: Syntax-highlighted TOML

---

## Part 3: Operators Pane

### Structure

- **Options (left)**: List All, Add Operator, Pending Approvals
- **Content (right)**: Operator list with status icons

---

## Part 4: Ghosts Pane

### Structure

- **Options (left)**: List All, New Ghost, Delete
- **Content (right)**: Ghost list with model info

---

## Part 5: Chat Pane (Full-Screen Overlay)

### Behavior

- **Activation**: Full-screen overlay on top of everything
- **Exit**: q or Esc
- **Messages**: Read session from database: can attach to existing session (use
  polling) or create a new one

---

## Part 6: Gate Pane

### Structure

- No options sidebar (uses full content area)
- Status header
- Restart button
- Log tailing area, taking the whole width for more content
  - This might require a logging rewrite: this is entirely ok

### Key Handlers

| Key   | Action                                      |
| ----- | ------------------------------------------- |
| r     | Restart gateway                             |
| /     | Enter search mode                           |
| Esc   | Exit search mode                            |
| Space | Pause/resume                                |
| c     | Clear logs                                  |
| 1-3   | Filter by source (gateway, ghost, operator) |

---

## wart 7: Testing

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
