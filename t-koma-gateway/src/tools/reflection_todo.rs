//! Structured TODO list for reflection jobs.
//!
//! The reflection agent uses this tool to plan work, track progress, and
//! provide real-time observability via the TUI. The TODO list is persisted
//! to the `job_logs.todo_list` column after each mutation.

use serde::Deserialize;
use serde_json::{Value, json};
use t_koma_db::job_logs::TodoStatus;

use super::{Tool, ToolContext};

#[derive(Debug, Deserialize)]
struct ReflectionTodoInput {
    action: String,
    items: Option<Vec<TodoPlanItem>>,
    index: Option<usize>,
    status: Option<String>,
    note: Option<String>,
    title: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TodoPlanItem {
    title: String,
    description: Option<String>,
}

pub struct ReflectionTodoTool;

impl ReflectionTodoTool {
    fn format_todo_list(todos: &[t_koma_db::job_logs::TodoItem]) -> String {
        if todos.is_empty() {
            return "No TODO items.".to_string();
        }
        let total = todos.len();
        let done = todos
            .iter()
            .filter(|t| t.status == TodoStatus::Done || t.status == TodoStatus::Skipped)
            .count();

        let mut out = format!("TODO [{}/{}]\n", done, total);
        for (i, item) in todos.iter().enumerate() {
            let marker = match item.status {
                TodoStatus::Pending => "○",
                TodoStatus::InProgress => "◉",
                TodoStatus::Done => "✓",
                TodoStatus::Skipped => "–",
            };
            out.push_str(&format!("[{}/{}] {} {}", i + 1, total, marker, item.title));
            if let Some(note) = &item.note {
                out.push_str(&format!(" ({})", note));
            }
            out.push('\n');
        }
        out
    }

    fn parse_status(s: &str) -> Result<TodoStatus, String> {
        match s {
            "pending" => Ok(TodoStatus::Pending),
            "in_progress" => Ok(TodoStatus::InProgress),
            "done" => Ok(TodoStatus::Done),
            "skipped" => Ok(TodoStatus::Skipped),
            other => Err(format!(
                "Invalid status '{}'. Use: pending, in_progress, done, skipped",
                other
            )),
        }
    }
}

#[async_trait::async_trait]
impl Tool for ReflectionTodoTool {
    fn name(&self) -> &str {
        "reflection_todo"
    }

    fn description(&self) -> &str {
        "Manage the reflection TODO list. Plan work, track progress, add discovered items."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["plan", "update", "add"],
                    "description": "plan: create/replace the full TODO list. update: change status of an item. add: append a new item."
                },
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "title": {"type": "string"},
                            "description": {"type": "string"}
                        },
                        "required": ["title"]
                    },
                    "description": "Items for 'plan' action."
                },
                "index": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "1-based item index for 'update' action."
                },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "done", "skipped"],
                    "description": "New status for 'update' action."
                },
                "note": {
                    "type": "string",
                    "description": "Optional note for 'update' action."
                },
                "title": {
                    "type": "string",
                    "description": "Title for 'add' action."
                },
                "description": {
                    "type": "string",
                    "description": "Optional description for 'add' action."
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String> {
        let input: ReflectionTodoInput = serde_json::from_value(args).map_err(|e| e.to_string())?;

        let handle = context
            .job_handle
            .as_mut()
            .ok_or("reflection_todo is only available during background jobs")?;

        match input.action.as_str() {
            "plan" => {
                let items = input.items.ok_or("'items' is required for plan action")?;

                handle.todos = items
                    .into_iter()
                    .map(|item| t_koma_db::job_logs::TodoItem {
                        title: item.title,
                        description: item.description,
                        status: TodoStatus::Pending,
                        note: None,
                    })
                    .collect();

                handle.persist_todos().await?;
                Ok(Self::format_todo_list(&handle.todos))
            }
            "update" => {
                let index = input.index.ok_or("'index' is required for update action")?;
                let status_str = input
                    .status
                    .ok_or("'status' is required for update action")?;
                let status = Self::parse_status(&status_str)?;

                if index == 0 || index > handle.todos.len() {
                    return Err(format!(
                        "Index {} out of range (1-{})",
                        index,
                        handle.todos.len()
                    ));
                }

                let item = &mut handle.todos[index - 1];
                item.status = status;
                if let Some(note) = input.note {
                    item.note = Some(note);
                }

                handle.persist_todos().await?;
                Ok(Self::format_todo_list(&handle.todos))
            }
            "add" => {
                let title = input.title.ok_or("'title' is required for add action")?;

                handle.todos.push(t_koma_db::job_logs::TodoItem {
                    title,
                    description: input.description,
                    status: TodoStatus::Pending,
                    note: None,
                });

                handle.persist_todos().await?;
                Ok(Self::format_todo_list(&handle.todos))
            }
            other => Err(format!(
                "Unknown action '{}'. Use 'plan', 'update', or 'add'.",
                other
            )),
        }
    }
}
