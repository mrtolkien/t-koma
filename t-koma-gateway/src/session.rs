use tracing::{error, info};

use crate::models::anthropic::history::{build_api_messages, ApiMessage};
use crate::models::prompt::{build_system_prompt, SystemBlock};
use crate::models::provider::{
    extract_all_text, has_tool_uses, Provider, ProviderContentBlock, ProviderResponse,
};
use crate::prompt::SystemPrompt;
use crate::tools::ToolManager;
use t_koma_db::{ContentBlock as DbContentBlock, GhostDbPool, MessageRole, SessionRepository};

/// Errors that can occur during session chat
#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    #[error("Database error: {0}")]
    Database(#[from] t_koma_db::DbError),

    #[error("Anthropic API error: {0}")]
    Api(String),

    #[error("Session not found or access denied")]
    SessionNotFound,

    #[error("Tool execution error: {0}")]
    ToolExecution(String),
}

/// High-level chat interface that hides tools and conversation complexity
///
/// This is the SINGLE interface that Discord, WebSocket, and other transports
/// should use. It handles everything: history, system prompts, tool loops, etc.
pub struct SessionChat {
    pub(crate) tool_manager: ToolManager,
}

impl SessionChat {
    /// Create a new SessionChat instance
    pub fn new() -> Self {
        Self {
            tool_manager: ToolManager::new(),
        }
    }

    /// Send an operator message and get the GHOST response
    ///
    /// This method handles the ENTIRE conversation flow:
    /// 1. Verifies session ownership
    /// 2. Saves the operator message to the database
    /// 3. Fetches conversation history
    /// 4. Builds system prompt with all available tools
    /// 5. Sends to the provider with full tool use loop support
    /// 6. Saves the ghost response to the database
    /// 7. Returns the final text response
    ///
    /// # Arguments
    /// * `ghost_db` - The GHOST database pool for this ghost
    /// * `session_id` - The session ID to chat in
    /// * `operator_id` - The operator ID (for session ownership verification)
    /// * `message` - The operator's message content
    ///
    /// # Returns
    /// The final text response from the provider
    #[allow(clippy::too_many_arguments)]
    pub async fn chat(
        &self,
        ghost_db: &GhostDbPool,
        provider: &dyn Provider,
        provider_name: &str,
        model: &str,
        session_id: &str,
        operator_id: &str,
        message: &str,
    ) -> Result<String, ChatError> {
        // Verify session exists and belongs to operator
        let session = SessionRepository::get_by_id(ghost_db.pool(), session_id)
            .await?
            .ok_or(ChatError::SessionNotFound)?;

        if session.operator_id != operator_id {
            return Err(ChatError::SessionNotFound);
        }

        info!(
            "[session:{}] Chat message from operator {}",
            session_id, operator_id
        );

        // Save operator message to database
        let user_content = vec![DbContentBlock::Text {
            text: message.to_string(),
        }];
        SessionRepository::add_message(
            ghost_db.pool(),
            session_id,
            MessageRole::Operator,
            user_content,
            None,
        )
        .await?;

        // Fetch conversation history
        let history = SessionRepository::get_messages(ghost_db.pool(), session_id).await?;

        // Build system prompt with tools
        let tools = self.tool_manager.get_tools();
        let system_prompt = SystemPrompt::with_tools(&tools);
        let system_blocks = build_system_prompt(&system_prompt);

        // Build API messages from history
        let api_messages = build_api_messages(&history, Some(50));

        // Send to provider with tool loop
        let response = self
            .send_with_tool_loop(
                ghost_db,
                provider,
                provider_name,
                model,
                session_id,
                system_blocks,
                api_messages,
                message,
            )
            .await?;

        Ok(response)
    }

    /// Internal method: Send conversation to the provider with full tool use loop
    #[allow(clippy::too_many_arguments)]
    async fn send_with_tool_loop(
        &self,
        ghost_db: &GhostDbPool,
        provider: &dyn Provider,
        provider_name: &str,
        model: &str,
        session_id: &str,
        system_blocks: Vec<SystemBlock>,
        api_messages: Vec<ApiMessage>,
        new_message: &str,
    ) -> Result<String, ChatError> {
        let tools = self.tool_manager.get_tools();

        // Initial request to the provider
        let mut response = provider
            .send_conversation(
                Some(system_blocks.clone()),
                api_messages.clone(),
                tools.clone(),
                Some(new_message),
                None,
                None,
            )
            .await
            .map_err(|e| ChatError::Api(e.to_string()))?;

        // Handle tool use loop (max 5 iterations to prevent infinite loops)
        for iteration in 0..5 {
            let has_tool_use = has_tool_uses(&response);

            if !has_tool_use {
                break;
            }

            info!(
                "[session:{}] Tool use detected (iteration {})",
                session_id,
                iteration + 1
            );

            // Save ghost message with tool_use blocks
            self.save_ghost_response(ghost_db, session_id, model, &response)
                .await;

            // Execute tools and get results
            let tool_results = self.execute_tools_from_response(session_id, &response).await;

            // Save tool results to database
            self.save_tool_results(ghost_db, session_id, &tool_results).await;

            // Build new API messages including the tool results
            let history = SessionRepository::get_messages(ghost_db.pool(), session_id).await?;
            let new_api_messages = build_api_messages(&history, Some(50));

            // Send tool results back to the provider
            response = provider
                .send_conversation(
                    Some(system_blocks.clone()),
                    new_api_messages,
                    tools.clone(),
                    None,
                    None,
                    None,
                )
                .await
                .map_err(|e| ChatError::Api(e.to_string()))?;
        }

        // Extract and save final text response
        let text = self
            .finalize_response(ghost_db, session_id, provider_name, model, &response)
            .await;

        Ok(text)
    }

    /// Save a ghost response (with tool_use blocks) to the database
    async fn save_ghost_response(
        &self,
        ghost_db: &GhostDbPool,
        session_id: &str,
        model: &str,
        response: &ProviderResponse,
    ) {
        let ghost_content: Vec<DbContentBlock> = response
            .content
            .iter()
            .map(|block| match block {
                ProviderContentBlock::Text { text } => {
                    DbContentBlock::Text { text: text.clone() }
                }
                ProviderContentBlock::ToolUse { id, name, input } => DbContentBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                },
                ProviderContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => DbContentBlock::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    content: content.clone(),
                    is_error: *is_error,
                },
            })
            .collect();

        if let Err(e) = SessionRepository::add_message(
            ghost_db.pool(),
            session_id,
            MessageRole::Ghost,
            ghost_content,
            Some(model),
        )
        .await
        {
            error!(
                "[session:{}] Failed to save ghost message: {}",
                session_id, e
            );
        }
    }

    /// Execute all tool_use blocks from a response and return the results
    async fn execute_tools_from_response(
        &self,
        session_id: &str,
        response: &ProviderResponse,
    ) -> Vec<DbContentBlock> {
        let mut tool_results = Vec::new();

        for block in &response.content {
            let ProviderContentBlock::ToolUse { id, name, input } = block else {
                continue;
            };

            info!("[session:{}] Executing tool: {} (id: {})", session_id, name, id);

            let result = self.tool_manager.execute(name, input.clone()).await;

            let content = match result {
                Ok(output) => output,
                Err(e) => format!("Error: {}", e),
            };

            tool_results.push(DbContentBlock::ToolResult {
                tool_use_id: id.clone(),
                content,
                is_error: None,
            });
        }

        tool_results
    }

    /// Save tool results to the database
    async fn save_tool_results(
        &self,
        ghost_db: &GhostDbPool,
        session_id: &str,
        tool_results: &[DbContentBlock],
    ) {
        if let Err(e) = SessionRepository::add_message(
            ghost_db.pool(),
            session_id,
            MessageRole::Operator,
            tool_results.to_vec(),
            None,
        )
        .await
        {
            error!(
                "[session:{}] Failed to save tool results: {}",
                session_id, e
            );
        }
    }

    /// Extract final text response and save it to the database
    async fn finalize_response(
        &self,
        ghost_db: &GhostDbPool,
        session_id: &str,
        provider_name: &str,
        model: &str,
        response: &ProviderResponse,
    ) -> String {
        let text = extract_all_text(response);

        info!(
            "[session:{}] GHOST final response ({} / {}): {}",
            session_id,
            provider_name,
            model,
            if text.len() > 100 { &text[..100] } else { &text }
        );

        let final_content = vec![DbContentBlock::Text { text: text.clone() }];
        if let Err(e) = SessionRepository::add_message(
            ghost_db.pool(),
            session_id,
            MessageRole::Ghost,
            final_content,
            Some(model),
        )
        .await
        {
            error!(
                "[session:{}] Failed to save final ghost message: {}",
                session_id, e
            );
        }

        text
    }
}

impl Default for SessionChat {
    fn default() -> Self {
        Self::new()
    }
}
