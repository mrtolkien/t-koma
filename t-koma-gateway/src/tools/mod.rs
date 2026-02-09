pub mod change_directory;
pub mod context;
pub mod create_file;
pub mod file_edit;
pub mod find_files;
pub mod knowledge_get;
pub mod knowledge_search;
pub mod list_dir;
pub mod load_skill;
pub mod manager;
pub mod note_write;
pub mod read_file;
pub mod reference_import;
pub mod reference_manage;
pub mod reference_write;
pub mod search;
pub mod shell;
pub mod web_fetch;
pub mod web_search;
pub use context::{ApprovalReason, ToolContext};

pub use manager::ToolManager;

use serde_json::Value;

/// Controls when a tool is visible in the tool list sent to the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolVisibility {
    /// Available in both interactive chat and background jobs.
    Always,
    /// Only available in background jobs (heartbeat, reflection).
    BackgroundOnly,
}

/// Trait that all tools must implement
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    /// Name of the tool (must match regex `^[a-zA-Z0-9_-]{1,64}$`)
    fn name(&self) -> &str;

    /// Description of what the tool does
    fn description(&self) -> &str;

    /// JSON Schema for the tool's input
    fn input_schema(&self) -> Value;

    /// When this tool should appear in the tool list. Default: Always.
    fn visibility(&self) -> ToolVisibility {
        ToolVisibility::Always
    }

    /// Execute the tool with the given arguments
    async fn execute(&self, args: Value, context: &mut ToolContext) -> Result<String, String>;
}
