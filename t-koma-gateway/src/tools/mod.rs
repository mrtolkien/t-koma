pub mod change_directory;
pub mod context;
pub mod create_file;
pub mod file_edit;
pub mod find_files;
pub mod list_dir;
pub mod manager;
pub mod memory_capture;
pub mod memory_get;
pub mod memory_search;
pub mod read_file;
pub mod reference_search;
pub mod search;
pub mod web_fetch;
pub mod web_search;
pub use context::ToolContext;

pub use manager::ToolManager;
pub mod load_skill;
pub mod shell;

use serde_json::Value;

/// Trait that all tools must implement
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    /// Name of the tool (must match regex `^[a-zA-Z0-9_-]{1,64}$`)
    fn name(&self) -> &str;

    /// Description of what the tool does
    fn description(&self) -> &str;

    /// JSON Schema for the tool's input
    fn input_schema(&self) -> Value;

    /// Execute the tool with the given arguments
    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String>;

    /// Optional prompt/instructions for how to use this tool.
    ///
    /// Returns `Some(&str)` with detailed instructions, or `None` if no
    /// additional prompt is needed. These prompts are automatically composed
    /// into the system prompt when using `SystemPrompt::with_tools()`.
    ///
    /// Default implementation returns `None`.
    fn prompt(&self) -> Option<&'static str> {
        None
    }
}
