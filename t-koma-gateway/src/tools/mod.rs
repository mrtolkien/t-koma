pub mod shell;
pub mod file_edit;

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
    async fn execute(&self, args: Value) -> Result<String, String>;
}
