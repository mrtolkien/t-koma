//! Deterministic, non-model-facing messages for the gateway.

pub mod common {
    pub const OPERATOR_CREATED_AWAITING_APPROVAL: &str =
        "Operator created. Awaiting approval via management CLI.";
    pub const ACCESS_PENDING_DISCORD: &str =
        "Your access request is pending approval. The Puppet Master will review it.";
    pub const ACCESS_DENIED: &str = "Your access request was denied.";
    pub const NO_PENDING_APPROVAL: &str = "There is no pending approval to resolve.";
    pub const NO_PENDING_TOOL_LOOP: &str = "There is no pending tool continuation to resolve.";
    pub const TOOL_LOOP_DENIED: &str = "Tool continuation denied.";

    pub fn approval_required(path: Option<&str>) -> String {
        match path {
            Some(path) => format!(
                "Approval required to leave the workspace (requested: {}). Reply with APPROVE or DENY. Approval applies to the next action only.",
                path
            ),
            None => "Approval required to leave the workspace. Reply with APPROVE or DENY. Approval applies to the next action only.".to_string(),
        }
    }

    pub fn tool_loop_limit_reached(limit: usize, extra: usize) -> String {
        format!(
            "Tool execution reached the safety limit ({} steps). Reply with APPROVE to allow {} more steps, or reply with STEPS <n> to set a maximum.",
            limit, extra
        )
    }
}

pub mod discord {
    use t_koma_db::Ghost;

    pub const INTERFACE_PROMPT: &str = "┌───────────────────────────────────────────────┐\n│ T-KOMA (ティーコマ) // INTERFACE BINDING       │\n├───────────────────────────────────────────────┤\n│ This interface must belong to:                │\n│   - an EXISTING OPERATOR (オペレータ)          │\n│   - a NEW OPERATOR (オペレータ)                │\n│                                               │\n│ Reply with:                                   │\n│   NEW  -> create a new operator               │\n│   EXISTING -> link to an existing operator    │\n└───────────────────────────────────────────────┘";

    pub const EXISTING_OPERATOR_TODO: &str =
        "Linking an existing operator is not implemented yet. TODO.";

    pub const GHOST_NAME_PROMPT: &str = "┌───────────────────────────────────────────────┐\n│ T-KOMA (ティーコマ) // GHOST INITIALIZATION    │\n├───────────────────────────────────────────────┤\n│ PUPPET MASTER ACCESS WARNING                   │\n│ The Puppet Master has full access to this     │\n│ GHOST (ゴースト)'s memory and workspace.      │\n│                                               │\n│ Only proceed if you trust the Puppet Master.  │\n├───────────────────────────────────────────────┤\n│ Enter a GHOST name (letters/numbers, spaces,  │\n│ '-', '_' plus kanji/katakana). Example: ALPHA │\n└───────────────────────────────────────────────┘";

    pub const GHOST_CREATED_HEADER: &str = "┌───────────────────────────────┐\n│ GHOST (ゴースト) ONLINE        │\n└───────────────────────────────┘";

    pub const ERROR_GENERIC: &str = "Sorry, an error occurred. Please try again later.";
    pub const ERROR_FAILED_CREATE_OPERATOR: &str = "Failed to create operator.";
    pub const ERROR_FAILED_CREATE_INTERFACE: &str = "Failed to create interface.";
    pub const ERROR_FAILED_LOAD_OPERATOR: &str = "Failed to load operator.";
    pub const ERROR_FAILED_LOAD_GHOSTS: &str = "Sorry, I failed to load your ghosts.";
    pub const ERROR_FAILED_INIT_GHOST_STORAGE: &str = "Failed to initialize ghost storage.";
    pub const ERROR_FAILED_CREATE_SESSION: &str = "Failed to create a session for your ghost.";
    pub const ERROR_MISSING_BOOTSTRAP: &str =
        "default-prompts/BOOTSTRAP.md missing; cannot initialize ghost.";
    pub const ERROR_GHOST_BOOT_FAILED: &str = "Ghost failed to boot. Try again later.";
    pub const ERROR_INIT_SESSION: &str = "Sorry, an error occurred initializing your session.";
    pub const ERROR_PROCESSING_REQUEST: &str =
        "Sorry, I encountered an error processing your request.";
    pub const INTERFACE_INVALID_OPERATOR: &str = "Interface is not linked to a valid operator.";

    pub fn invalid_ghost_name(error: &str) -> String {
        format!("Invalid ghost name. {}\n\n{}", error, GHOST_NAME_PROMPT)
    }

    pub fn ghost_created_header_with_name(ghost_name: &str) -> String {
        format!("{}\nGHOST: {}", GHOST_CREATED_HEADER, ghost_name)
    }

    pub fn active_ghost_set(ghost_name: &str) -> String {
        format!(
            "Active GHOST set to: {}\n\n{}",
            ghost_name, GHOST_CREATED_HEADER
        )
    }

    pub fn unknown_ghost_name(list: &str) -> String {
        format!("Unknown GHOST name.\n\n{}", list)
    }

    pub fn select_ghost_prompt(list: &str) -> String {
        format!("Select a GHOST by typing: `ghost: NAME`\n\n{}", list)
    }

    pub fn format_ghost_list(ghosts: &[Ghost]) -> String {
        let mut lines = vec![
            "┌───────────────────────────────┐".to_string(),
            "│ AVAILABLE GHOSTS (ゴースト)   │".to_string(),
            "├───────────────────────────────┤".to_string(),
        ];

        for ghost in ghosts {
            lines.push(format!("│ - {}", ghost.name));
        }

        lines.push("└───────────────────────────────┘".to_string());
        lines.join("\n")
    }
}

pub mod server {
    pub const HEALTH_STATUS: &str = "ok";
    pub const HEALTH_KOMA: &str = "running";
    pub const ERROR_INTERNAL_OPERATOR_STATUS: &str = "Internal error checking operator status";
    pub const ACCESS_PENDING: &str = "Your access request is pending approval";
    pub const ACCESS_DENIED: &str = "Your access request was denied";
    pub const UNKNOWN_OPERATOR_STATUS: &str = "Unknown operator status";
    pub const ANONYMOUS_OPERATOR: &str = "Anonymous Operator";
    pub const FAILED_LOAD_INTERFACE: &str = "Failed to load interface";
    pub const INTERFACE_INVALID_OPERATOR: &str = "Interface is not linked to a valid operator";
    pub const FAILED_LOAD_OPERATOR: &str = "Failed to load operator";
    pub const INTERFACE_REQUIRED: &str = "T-KOMA (ティーコマ): This interface must belong to an EXISTING or NEW OPERATOR (オペレータ). Reply with NEW or EXISTING.";
    pub const EXISTING_OPERATOR_TODO: &str = "Existing operator binding not implemented yet. TODO.";
    pub const REPLY_WITH_NEW_OR_EXISTING: &str = "Reply with NEW or EXISTING.";
    pub const FAILED_CREATE_OPERATOR: &str = "Failed to create operator";
    pub const FAILED_CREATE_INTERFACE: &str = "Failed to create interface";
    pub const SELECT_NEW_OR_EXISTING_FIRST: &str = "Select NEW or EXISTING operator first.";
    pub const FAILED_LIST_GHOSTS: &str = "Failed to list ghosts";
    pub const NO_GHOSTS_FOR_OPERATOR: &str =
        "No GHOST (ゴースト) exists for this operator. Create one via Discord first.";
    pub const UNKNOWN_GHOST_NAME: &str = "Unknown ghost name";
    pub const FAILED_LOAD_GHOST: &str = "Failed to load ghost";
    pub const GHOST_NOT_OWNED: &str = "Ghost does not belong to this operator";
    pub const FAILED_INIT_GHOST_SESSION: &str = "Failed to initialize ghost session";
    pub const FAILED_CREATE_SESSION: &str = "Failed to create session";
    pub const NO_ACTIVE_GHOST: &str = "No active ghost selected";
    pub const FAILED_INIT_GHOST_DB: &str = "Failed to initialize ghost DB";
    pub const FAILED_INIT_SESSION: &str = "Failed to initialize session";
    pub const INVALID_SESSION: &str = "Invalid session";
    pub const FAILED_LIST_SESSIONS: &str = "Failed to list sessions";
    pub const FAILED_SWITCH_SESSION: &str = "Failed to switch session";
    pub const FAILED_DELETE_SESSION: &str = "Failed to delete session";
    pub const CONNECTED_PUPPET_MASTER: &str =
        "Connected to T-KOMA (ティーコマ) as the Puppet Master.";
    pub const CONNECTED_LOGS: &str = "Connected to T-KOMA (ティーコマ) logs";

    pub fn model_not_configured(model: &str, provider: &str) -> String {
        format!(
            "Model '{}' for provider '{}' is not configured",
            model, provider
        )
    }

    pub fn no_models_configured(provider: &str) -> String {
        format!("No models configured for provider '{}'", provider)
    }
}
