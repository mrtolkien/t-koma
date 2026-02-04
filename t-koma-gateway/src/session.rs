use tracing::{error, info};

use crate::models::anthropic::history::{ApiMessage, build_api_messages};
use crate::models::prompt::{SystemBlock, build_system_prompt};
use crate::models::provider::{
    Provider, ProviderContentBlock, ProviderResponse, extract_all_text, has_tool_uses,
};
use crate::prompt::SystemPrompt;
use crate::tools::context::approval_required_path;
use crate::tools::{ToolContext, ToolManager};
use serde_json::Value;
use t_koma_db::{
    ContentBlock as DbContentBlock, GhostDbPool, GhostRepository, KomaDbPool, MessageRole,
    SessionRepository,
};

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

    #[error("Tool approval required")]
    ToolApprovalRequired(PendingToolApproval),

    #[error("Tool loop limit reached")]
    ToolLoopLimitReached(PendingToolContinuation),
}

#[derive(Debug, Clone)]
pub struct PendingToolUse {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone)]
pub struct PendingToolApproval {
    pub pending_tool_uses: Vec<PendingToolUse>,
    pub completed_results: Vec<DbContentBlock>,
    pub requested_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PendingToolContinuation {
    pub pending_tool_uses: Vec<PendingToolUse>,
}

pub const DEFAULT_TOOL_LOOP_LIMIT: usize = 10;
pub const DEFAULT_TOOL_LOOP_EXTRA: usize = 50;

#[derive(Debug, Clone, Copy)]
pub enum ToolApprovalDecision {
    Approve,
    Deny,
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
        koma_db: &KomaDbPool,
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
                koma_db,
                provider,
                provider_name,
                model,
                session_id,
                system_blocks,
                api_messages,
                Some(message),
                DEFAULT_TOOL_LOOP_LIMIT,
            )
            .await?;

        Ok(response)
    }

    /// Internal method: Send conversation to the provider with full tool use loop
    #[allow(clippy::too_many_arguments)]
    async fn send_with_tool_loop(
        &self,
        ghost_db: &GhostDbPool,
        koma_db: &KomaDbPool,
        provider: &dyn Provider,
        provider_name: &str,
        model: &str,
        session_id: &str,
        system_blocks: Vec<SystemBlock>,
        api_messages: Vec<ApiMessage>,
        new_message: Option<&str>,
        max_iterations: usize,
    ) -> Result<String, ChatError> {
        let tools = self.tool_manager.get_tools();

        // Initial request to the provider
        let mut response = provider
            .send_conversation(
                Some(system_blocks.clone()),
                api_messages.clone(),
                tools.clone(),
                new_message,
                None,
                None,
            )
            .await
            .map_err(|e| ChatError::Api(e.to_string()))?;

        // Handle tool use loop (bounded to prevent infinite loops)
        let mut tool_context = self.load_tool_context(koma_db, ghost_db).await?;
        for iteration in 0..max_iterations {
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

            if iteration + 1 == max_iterations {
                return Err(ChatError::ToolLoopLimitReached(PendingToolContinuation {
                    pending_tool_uses: collect_pending_tool_uses(&response),
                }));
            }

            // Execute tools and get results
            let tool_results = match self
                .execute_tools_from_response(session_id, &response, koma_db, &mut tool_context)
                .await
            {
                Ok(results) => results,
                Err(ChatError::ToolLoopLimitReached(pending)) => {
                    return Err(ChatError::ToolLoopLimitReached(pending));
                }
                Err(e) => return Err(e),
            };

            // Save tool results to database
            self.save_tool_results(ghost_db, session_id, &tool_results)
                .await;

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
                ProviderContentBlock::Text { text } => DbContentBlock::Text { text: text.clone() },
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
        koma_db: &KomaDbPool,
        tool_context: &mut ToolContext,
    ) -> Result<Vec<DbContentBlock>, ChatError> {
        let tool_uses = collect_pending_tool_uses(response);

        let mut tool_results = Vec::new();
        self.execute_tool_uses(
            session_id,
            &tool_uses,
            koma_db,
            tool_context,
            &mut tool_results,
        )
        .await?;

        Ok(tool_results)
    }

    async fn execute_tool_uses(
        &self,
        session_id: &str,
        tool_uses: &[PendingToolUse],
        koma_db: &KomaDbPool,
        tool_context: &mut ToolContext,
        tool_results: &mut Vec<DbContentBlock>,
    ) -> Result<(), ChatError> {
        for (index, tool_use) in tool_uses.iter().enumerate() {
            info!(
                "[session:{}] Executing tool: {} (id: {})",
                session_id, tool_use.name, tool_use.id
            );

            let result = self
                .tool_manager
                .execute_with_context(&tool_use.name, tool_use.input.clone(), tool_context)
                .await;

            let content = match result {
                Ok(output) => output,
                Err(e) => {
                    if let Some(path) = approval_required_path(&e) {
                        return Err(ChatError::ToolApprovalRequired(PendingToolApproval {
                            pending_tool_uses: tool_uses[index..].to_vec(),
                            completed_results: tool_results.clone(),
                            requested_path: Some(path.to_string()),
                        }));
                    }
                    format!("Error: {}", e)
                }
            };

            tool_results.push(DbContentBlock::ToolResult {
                tool_use_id: tool_use.id.clone(),
                content,
                is_error: None,
            });

            if let Err(e) = self.persist_tool_context(koma_db, tool_context).await {
                error!(
                    "[session:{}] Failed to persist tool context: {}",
                    session_id, e
                );
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn resume_tool_approval(
        &self,
        ghost_db: &GhostDbPool,
        koma_db: &KomaDbPool,
        provider: &dyn Provider,
        provider_name: &str,
        model: &str,
        session_id: &str,
        pending: PendingToolApproval,
        decision: ToolApprovalDecision,
    ) -> Result<String, ChatError> {
        let mut tool_context = self.load_tool_context(koma_db, ghost_db).await?;

        let mut tool_results = pending.completed_results;
        match decision {
            ToolApprovalDecision::Approve => {
                tool_context.set_allow_outside_workspace(true);
                self.persist_tool_context(koma_db, &mut tool_context)
                    .await?;
                self.execute_tool_uses(
                    session_id,
                    &pending.pending_tool_uses,
                    koma_db,
                    &mut tool_context,
                    &mut tool_results,
                )
                .await?;
            }
            ToolApprovalDecision::Deny => {
                for (index, tool_use) in pending.pending_tool_uses.iter().enumerate() {
                    let content = if index == 0 {
                        "Error: Operator denied approval to leave the workspace.".to_string()
                    } else {
                        "Error: Skipped because approval was denied.".to_string()
                    };
                    tool_results.push(DbContentBlock::ToolResult {
                        tool_use_id: tool_use.id.clone(),
                        content,
                        is_error: None,
                    });
                }
            }
        }

        self.save_tool_results(ghost_db, session_id, &tool_results)
            .await;

        self.resume_after_tool_results(
            ghost_db,
            koma_db,
            provider,
            provider_name,
            model,
            session_id,
            DEFAULT_TOOL_LOOP_LIMIT,
        )
        .await
    }

    pub async fn resume_tool_loop(
        &self,
        ghost_db: &GhostDbPool,
        koma_db: &KomaDbPool,
        provider: &dyn Provider,
        provider_name: &str,
        model: &str,
        session_id: &str,
        pending: PendingToolContinuation,
        extra_iterations: usize,
    ) -> Result<String, ChatError> {
        let mut tool_context = self.load_tool_context(koma_db, ghost_db).await?;
        let mut tool_results = Vec::new();
        self.execute_tool_uses(
            session_id,
            &pending.pending_tool_uses,
            koma_db,
            &mut tool_context,
            &mut tool_results,
        )
        .await?;

        self.save_tool_results(ghost_db, session_id, &tool_results)
            .await;

        self.resume_after_tool_results(
            ghost_db,
            koma_db,
            provider,
            provider_name,
            model,
            session_id,
            extra_iterations,
        )
        .await
    }

    async fn resume_after_tool_results(
        &self,
        ghost_db: &GhostDbPool,
        koma_db: &KomaDbPool,
        provider: &dyn Provider,
        provider_name: &str,
        model: &str,
        session_id: &str,
        max_iterations: usize,
    ) -> Result<String, ChatError> {
        let history = SessionRepository::get_messages(ghost_db.pool(), session_id).await?;

        let tools = self.tool_manager.get_tools();
        let system_prompt = SystemPrompt::with_tools(&tools);
        let system_blocks = build_system_prompt(&system_prompt);
        let api_messages = build_api_messages(&history, Some(50));

        self.send_with_tool_loop(
            ghost_db,
            koma_db,
            provider,
            provider_name,
            model,
            session_id,
            system_blocks,
            api_messages,
            None,
            max_iterations,
        )
        .await
    }

    async fn load_tool_context(
        &self,
        koma_db: &KomaDbPool,
        ghost_db: &GhostDbPool,
    ) -> Result<ToolContext, ChatError> {
        let ghost_name = ghost_db.ghost_name().to_string();
        let tool_state =
            GhostRepository::get_tool_state_by_name(koma_db.pool(), &ghost_name).await?;

        let workspace_root = ghost_db.workspace_path().to_path_buf();
        let mut cwd = tool_state
            .cwd
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| workspace_root.clone());

        if !cwd.is_absolute() {
            cwd = workspace_root.join(cwd);
        }

        let mut context = ToolContext::new(
            ghost_name,
            workspace_root.clone(),
            cwd,
            false,
        );

        let cwd_missing = tokio::fs::metadata(context.cwd()).await.is_err();
        if cwd_missing {
            context.set_cwd(workspace_root);
        }

        if context.is_dirty() {
            self.persist_tool_context(koma_db, &mut context).await?;
        }

        Ok(context)
    }

    async fn persist_tool_context(
        &self,
        koma_db: &KomaDbPool,
        context: &mut ToolContext,
    ) -> Result<(), ChatError> {
        if !context.is_dirty() {
            return Ok(());
        }

        let cwd = context.cwd().to_string_lossy().to_string();
        GhostRepository::update_tool_state_by_name(
            koma_db.pool(),
            context.ghost_name(),
            &cwd,
        )
        .await?;
        context.clear_dirty();
        Ok(())
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
            if text.len() > 100 {
                &text[..100]
            } else {
                &text
            }
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

fn collect_pending_tool_uses(response: &ProviderResponse) -> Vec<PendingToolUse> {
    response
        .content
        .iter()
        .filter_map(|block| match block {
            ProviderContentBlock::ToolUse { id, name, input } => Some(PendingToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            }),
            _ => None,
        })
        .collect()
}
