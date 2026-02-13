#!/usr/bin/env rust-script
//! E2E Knowledge Test - Full system integration test
//!
//! This script exercises the complete T-KOMA system end-to-end:
//! - Creates operator and ghost
//! - Runs a conversation
//! - Triggers reflection
//! - Outputs detailed JSON report
//!
//! Usage:
//!   cargo run --example e2e_knowledge_test -- <MODEL_ALIAS>
//!
//! Example:
//!   cargo run --example e2e_knowledge_test -- kimi25
//!
//! The model must be defined in ~/.config/t-koma/config.toml:
//!   [models.kimi25]
//!   provider = "openrouter"
//!   model = "moonshotai/kimi-k2.5"
//!
//! Required environment:
//!   API keys for the provider (e.g., OPENROUTER_API_KEY)

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use serde::{Deserialize, Serialize};

// Import from workspace crates
use t_koma_core::config::{Config, KnowledgeSettings};
use t_koma_core::message::ProviderType;
use t_koma_db::{
    ContentBlock, GhostRepository, JobKind, JobLogRepository, KomaDbPool, Message, MessageRole,
    OperatorAccessLevel, OperatorRepository, Platform, SessionRepository, TranscriptEntry,
};
use t_koma_gateway::chat::compaction::CompactionConfig;
use t_koma_gateway::providers::anthropic::AnthropicClient;
use t_koma_gateway::providers::gemini::GeminiClient;
use t_koma_gateway::providers::openai_compatible::OpenAiCompatibleClient;
use t_koma_gateway::providers::openrouter::OpenRouterClient;
use t_koma_gateway::state::{AppState, ModelEntry};
use t_koma_knowledge::{KnowledgeSearchQuery, KnowledgeSearchResult, OwnershipScope};

/// Terminal styling
mod style {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const CYAN: &str = "\x1b[36m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const RED: &str = "\x1b[31m";
    pub const BLUE: &str = "\x1b[34m";
}

/// Test configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestConfig {
    operator_name: String,
    ghost_name: String,
    output_dir: PathBuf,
    temp_dir: PathBuf,
}

impl TestConfig {
    fn new() -> Self {
        let base_dir = std::env::var("E2E_OUTPUT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("e2e-output"));

        // Each run gets its own timestamped folder
        let run_id = Utc::now().format("%Y%m%dT%H%M%S").to_string();
        let output_dir = base_dir.join(&run_id);

        Self {
            operator_name: "OMEGA".to_string(),
            ghost_name: "CLANKER".to_string(),
            output_dir,
            temp_dir: std::env::temp_dir().join(format!("e2e-{}", uuid::Uuid::new_v4())),
        }
    }
}

/// Complete test report
#[derive(Debug, Serialize, Deserialize)]
struct TestReport {
    started_at: String,
    completed_at: String,
    duration_seconds: f64,
    config: TestConfig,
    model_used: String,
    operator: OperatorInfo,
    ghost: GhostInfo,
    session: SessionInfo,
    /// Full conversation with tool calls and content blocks
    conversation: Vec<DetailedMessage>,
    /// Reflection with full transcript
    reflection: Option<DetailedReflection>,
    /// Direct knowledge search verification
    knowledge_verification: Option<KnowledgeVerification>,
    /// All files in data directory (ghost workspace + shared)
    data_files: Vec<DataFile>,
    usage: UsageStats,
}

#[derive(Debug, Serialize, Deserialize)]
struct OperatorInfo {
    id: String,
    name: String,
    platform: String,
    access_level: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct GhostInfo {
    id: String,
    name: String,
    workspace_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionInfo {
    id: String,
    created_at: String,
    message_count: usize,
}

/// Detailed message with full content blocks (includes tool calls)
#[derive(Debug, Serialize, Deserialize)]
struct DetailedMessage {
    role: String,
    /// All content blocks including text, tool_use, tool_result
    content: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
}

/// Detailed reflection with full transcript
#[derive(Debug, Serialize, Deserialize)]
struct DetailedReflection {
    job_id: String,
    status: String,
    started_at: String,
    finished_at: String,
    /// Full transcript with all content blocks
    transcript: Vec<TranscriptEntryDetailed>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TranscriptEntryDetailed {
    role: String,
    content: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DataFile {
    path: String,
    size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    preview: Option<String>,
}

/// Knowledge search verification result
#[derive(Debug, Serialize, Deserialize)]
struct KnowledgeVerification {
    /// The query extracted from the ghost's first knowledge_search tool call
    query_used: String,
    /// Number of notes returned
    notes_count: usize,
    /// Number of diary entries returned
    diary_count: usize,
    /// Number of reference results returned
    references_count: usize,
    /// Number of topics returned
    topics_count: usize,
    /// Total results across all categories
    total_results: usize,
    /// Snippet previews of top results
    top_results: Vec<String>,
}

impl KnowledgeVerification {
    fn from_results(query: String, results: &KnowledgeSearchResult) -> Self {
        let notes_count = results.notes.len();
        let diary_count = results.diary.len();
        let references_count = results.references.results.len();
        let topics_count = results.topics.len();

        let mut top_results = Vec::new();
        for n in results.notes.iter().take(3) {
            top_results.push(format!("[note] {}: {}", n.summary.title, n.summary.snippet));
        }
        for d in results.diary.iter().take(2) {
            top_results.push(format!("[diary] {}: {}", d.date, d.snippet));
        }
        for r in results.references.results.iter().take(3) {
            top_results.push(format!("[ref] {}: {}", r.summary.title, r.summary.snippet));
        }
        for t in results.topics.iter().take(2) {
            top_results.push(format!("[topic] {}", t.title));
        }

        Self {
            query_used: query,
            notes_count,
            diary_count,
            references_count,
            topics_count,
            total_results: notes_count + diary_count + references_count + topics_count,
            top_results,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct UsageStats {
    total_messages: usize,
    operator_messages: usize,
    ghost_messages: usize,
}

// UI helpers
fn header(title: &str) {
    let width = 70;
    println!("\n{}{}{}", style::CYAN, style::BOLD, "━".repeat(width));
    println!("  {}", title);
    println!("{}{}{}\n", "━".repeat(width), style::RESET, style::RESET);
}

fn step(n: u8, total: u8, msg: &str) {
    println!(
        "{}[{}/{}]{} {}{}",
        style::CYAN,
        n,
        total,
        style::RESET,
        style::BOLD,
        msg
    );
    println!();
}

fn success(msg: &str) {
    println!("{}✓{} {}\n", style::GREEN, style::RESET, msg);
}

fn info(label: &str, value: &str) {
    println!("  {}:{} {}", style::DIM, style::RESET, label);
    println!("    {}{}", style::CYAN, value);
    println!("{}", style::RESET);
}

fn chat_msg(_role: &str, content: &str, is_ghost: bool) {
    let color = if is_ghost {
        style::MAGENTA
    } else {
        style::BLUE
    };
    let label = if is_ghost { "GHOST" } else { "OPERATOR" };

    println!(
        "{}{}[{}]{} {}",
        color,
        style::BOLD,
        label,
        style::RESET,
        style::DIM
    );

    for line in content.lines() {
        let mut current = String::new();
        for word in line.split_whitespace() {
            if current.len() + word.len() + 1 > 60 {
                println!("  {}", current);
                current = word.to_string();
            } else {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            println!("  {}", current);
        }
    }
    println!("{}", style::RESET);
    println!();
}

/// Parse command line arguments to get model alias
fn parse_args() -> String {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "\n{}Error: Model alias required{}\n",
            style::RED,
            style::RESET
        );
        eprintln!("Usage: cargo run --example e2e_knowledge_test -- <MODEL_ALIAS>\n");
        eprintln!("Example: cargo run --example e2e_knowledge_test -- kimi25\n");
        std::process::exit(1);
    }
    args[1].clone()
}

/// Convert ContentBlock to JSON Value for serialization
fn content_block_to_json(block: &ContentBlock) -> serde_json::Value {
    match block {
        ContentBlock::Text { text } => {
            serde_json::json!({"type": "text", "text": text})
        }
        ContentBlock::ToolUse { id, name, input } => {
            serde_json::json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input
            })
        }
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            serde_json::json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": content,
                "is_error": is_error
            })
        }
        ContentBlock::Image {
            path,
            mime_type,
            filename,
        } => {
            serde_json::json!({
                "type": "image",
                "path": path,
                "mime_type": mime_type,
                "filename": filename
            })
        }
        ContentBlock::File {
            path,
            filename,
            size,
        } => {
            serde_json::json!({
                "type": "file",
                "path": path,
                "filename": filename,
                "size": size
            })
        }
    }
}

/// Convert TranscriptEntry to detailed JSON
fn transcript_entry_to_detailed(entry: &TranscriptEntry) -> TranscriptEntryDetailed {
    TranscriptEntryDetailed {
        role: match entry.role {
            MessageRole::Operator => "operator".to_string(),
            MessageRole::Ghost => "ghost".to_string(),
        },
        content: entry.content.iter().map(content_block_to_json).collect(),
        model: entry.model.clone(),
    }
}

/// Convert Message to detailed message
fn message_to_detailed(msg: &Message, default_model: &str) -> DetailedMessage {
    DetailedMessage {
        role: match msg.role {
            MessageRole::Operator => "operator".to_string(),
            MessageRole::Ghost => "ghost".to_string(),
        },
        content: msg.content.iter().map(content_block_to_json).collect(),
        model: msg
            .model
            .clone()
            .or_else(|| Some(default_model.to_string())),
    }
}

/// Load real config and create model registry from it
fn create_models_from_config(model_alias: &str) -> (HashMap<String, ModelEntry>, String) {
    // Load the real config from ~/.config/t-koma/config.toml
    let config = Config::load().unwrap_or_else(|e| {
        eprintln!(
            "\n{}Error: Failed to load config{}\n",
            style::RED,
            style::RESET
        );
        eprintln!("Make sure ~/.config/t-koma/config.toml exists\n");
        eprintln!("Error details: {}\n", e);
        std::process::exit(1);
    });

    // Get the model config for the requested alias
    let model_config = config.model_config(model_alias).unwrap_or_else(|| {
        eprintln!(
            "\n{}Error: Model '{}' not found in config{}\n",
            style::RED,
            model_alias,
            style::RESET
        );
        eprintln!("Available models:\n");
        for alias in config.settings.models.keys() {
            eprintln!("  - {}", alias);
        }
        eprintln!();
        std::process::exit(1);
    });

    // Get API key for this model
    let api_key = config.api_key_for_alias(model_alias).unwrap_or_else(|e| {
        eprintln!(
            "\n{}Error: No API key for model '{}'{}\n",
            style::RED,
            model_alias,
            style::RESET
        );
        eprintln!("Error: {}\n", e);
        std::process::exit(1);
    });

    let api_key = api_key.unwrap_or_else(|| {
        eprintln!(
            "\n{}Error: No API key configured for model '{}'{}\n",
            style::RED,
            model_alias,
            style::RESET
        );
        std::process::exit(1);
    });

    let mut models = HashMap::new();

    // Create the provider client based on the config
    let client: Arc<dyn t_koma_gateway::providers::provider::Provider> = match model_config.provider
    {
        ProviderType::Anthropic => Arc::new(AnthropicClient::new(api_key, &model_config.model)),
        ProviderType::Gemini => Arc::new(GeminiClient::new(api_key, &model_config.model)),
        ProviderType::OpenRouter => Arc::new(OpenRouterClient::new(
            api_key,
            &model_config.model,
            model_config.base_url.clone(),
            config.settings.openrouter.http_referer.clone(),
            config.settings.openrouter.app_name.clone(),
            model_config.routing.clone(),
        )),
        ProviderType::OpenAiCompatible => {
            let base_url = model_config.base_url.clone().unwrap_or_else(|| {
                eprintln!(
                    "\n{}Error: openai_compatible model requires base_url{}\n",
                    style::RED,
                    style::RESET
                );
                std::process::exit(1);
            });
            Arc::new(OpenAiCompatibleClient::new(
                base_url,
                Some(api_key),
                &model_config.model,
                "openai_compatible",
            ))
        }
        ProviderType::KimiCode => {
            let base_url = model_config
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.kimi.com/coding/v1".to_string());
            Arc::new(OpenAiCompatibleClient::new(
                base_url,
                Some(api_key),
                &model_config.model,
                "kimi_code",
            ))
        }
    };

    models.insert(
        model_alias.to_string(),
        ModelEntry {
            alias: model_alias.to_string(),
            provider: model_config.provider.to_string(),
            model: model_config.model.clone(),
            client,
            context_window: model_config.context_window,
            retry_on_empty: model_config.retry_on_empty.unwrap_or(0),
        },
    );

    println!(
        "{}Using model:{}{} {} ({} via {})\n",
        style::GREEN,
        style::RESET,
        style::CYAN,
        model_alias,
        model_config.model,
        model_config.provider
    );

    (models, model_alias.to_string())
}

// Setup
async fn setup_env(config: &TestConfig, model_alias: &str) -> (Arc<AppState>, KomaDbPool) {
    fs::create_dir_all(&config.temp_dir).expect("Failed to create temp dir");

    unsafe {
        std::env::set_var("T_KOMA_DATA_DIR", &config.temp_dir);
    }

    let koma_db = create_db().await;
    let (models, default_alias) = create_models_from_config(model_alias);

    // Read embedding config from the real config.toml [tools.knowledge]
    let real_config = Config::load().expect("Failed to load config for knowledge settings");
    let mut knowledge_settings = KnowledgeSettings::from(&real_config.settings.tools.knowledge);

    // Override paths for test isolation
    knowledge_settings.knowledge_db_path_override = Some(config.temp_dir.join("knowledge.sqlite3"));
    knowledge_settings.data_root_override = Some(config.temp_dir.clone());
    knowledge_settings.reconcile_seconds = 999_999;

    let knowledge_engine = Arc::new(
        t_koma_knowledge::KnowledgeEngine::open(knowledge_settings)
            .await
            .expect("Failed to open knowledge engine"),
    );

    let state = Arc::new(AppState::new(
        vec![default_alias],
        models,
        koma_db.clone(),
        knowledge_engine,
        vec![],
        CompactionConfig {
            threshold: 0.85,
            keep_window: 20,
            mask_preview_chars: 100,
        },
    ));

    (state, koma_db)
}

async fn create_db() -> KomaDbPool {
    use sqlx::sqlite::SqlitePoolOptions;

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create pool");

    sqlx::migrate!("../t-koma-db/migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    KomaDbPool::from_pool(pool)
}

// Test execution
async fn create_entities(
    pool: &KomaDbPool,
    config: &TestConfig,
) -> (t_koma_db::Operator, t_koma_db::Ghost) {
    let operator = OperatorRepository::create_new(
        pool.pool(),
        &config.operator_name,
        Platform::Cli,
        OperatorAccessLevel::PuppetMaster,
    )
    .await
    .expect("Failed to create operator");

    let operator = OperatorRepository::approve(pool.pool(), &operator.id)
        .await
        .expect("Failed to approve operator");

    let ghost = GhostRepository::create(pool.pool(), &operator.id, &config.ghost_name)
        .await
        .expect("Failed to create ghost");

    success(&format!(
        "Created operator: {} ({})",
        operator.name, operator.id
    ));
    success(&format!("Created ghost: {} ({})", ghost.name, ghost.id));

    // Initialize ghost workspace - per AGENTS.md, SOUL.md should be minimal
    let workspace = t_koma_db::ghosts::ghost_workspace_path(&ghost.name).unwrap();
    fs::create_dir_all(&workspace).unwrap();

    // Per AGENTS.md: Ghost initialization should only set the name
    let soul = format!("I am called {}.\n", ghost.name);
    fs::write(workspace.join("SOUL.md"), &soul).unwrap();
    success("Initialized ghost workspace");

    (operator, ghost)
}

/// Run the conversation and capture full messages from DB
async fn run_conversation(
    state: &AppState,
    pool: &KomaDbPool,
    operator: &t_koma_db::Operator,
    ghost: &t_koma_db::Ghost,
) -> Vec<DetailedMessage> {
    let session = SessionRepository::create(pool.pool(), &ghost.id, &operator.id)
        .await
        .expect("Failed to create session");

    success(&format!("Created session: {}", session.id));
    state.set_active_ghost(&operator.id, &ghost.name).await;

    let default_model = state.default_model().model.clone();

    // Message 1
    chat_msg("OMEGA", "Hello, my name is OMEGA.", false);
    state
        .chat(
            &ghost.name,
            &session.id,
            &operator.id,
            "Hello, my name is OMEGA.",
        )
        .await
        .expect("Chat failed");

    // Fetch all messages from DB after chat (includes tool calls)
    let messages_after_1 = SessionRepository::list_messages(pool.pool(), &session.id)
        .await
        .expect("Failed to list messages");

    // Print ghost response for UI
    if let Some(last) = messages_after_1.last()
        && last.role == MessageRole::Ghost
        && let Some(ContentBlock::Text { text }) = last.content.first()
    {
        chat_msg(&ghost.name, text, true);
    }

    // Message 2
    let q = "I want to buy a new 3d printer. Enclosed, for home use. What do you recommend?";
    chat_msg("OMEGA", q, false);
    state
        .chat(&ghost.name, &session.id, &operator.id, q)
        .await
        .expect("Chat failed");

    // Fetch all messages from DB after second chat
    let messages_after_2 = SessionRepository::list_messages(pool.pool(), &session.id)
        .await
        .expect("Failed to list messages");

    // Print ghost response for UI
    if let Some(last) = messages_after_2.last()
        && last.role == MessageRole::Ghost
        && let Some(ContentBlock::Text { text }) = last.content.first()
    {
        chat_msg(&ghost.name, text, true);
    }

    // Convert all messages to detailed format
    messages_after_2
        .iter()
        .map(|m| message_to_detailed(m, &default_model))
        .collect()
}

/// Run reflection and capture full transcript
async fn run_reflection(
    state: &Arc<AppState>,
    pool: &KomaDbPool,
    operator: &t_koma_db::Operator,
    ghost: &t_koma_db::Ghost,
) -> Option<DetailedReflection> {
    use t_koma_gateway::reflection::run_reflection_now;

    let started = Utc::now();
    let session = SessionRepository::get_active(pool.pool(), &ghost.id, &operator.id)
        .await
        .ok()
        .flatten()
        .expect("No active session");

    run_reflection_now(
        state,
        &ghost.name,
        &ghost.id,
        &session.id,
        &operator.id,
        None,
    )
    .await;
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Get the full job log with transcript
    let logs = JobLogRepository::list_for_ghost(pool.pool(), &ghost.id, 5)
        .await
        .ok()?;
    let log = logs.iter().find(|l| l.job_kind == JobKind::Reflection)?;

    // Fetch full log with transcript
    let full_log = JobLogRepository::get(pool.pool(), &log.id)
        .await
        .ok()
        .flatten()?;

    success(&format!(
        "Reflection completed: {}",
        log.status.as_deref().unwrap_or("unknown")
    ));

    Some(DetailedReflection {
        job_id: log.id.clone(),
        status: log.status.clone().unwrap_or_default(),
        started_at: started.to_rfc3339(),
        finished_at: Utc::now().to_rfc3339(),
        transcript: full_log
            .transcript
            .iter()
            .map(transcript_entry_to_detailed)
            .collect(),
    })
}

/// Extract the query text from the first knowledge_search tool call in the conversation
fn extract_knowledge_query(messages: &[Message]) -> Option<String> {
    for msg in messages {
        for block in &msg.content {
            if let ContentBlock::ToolUse { name, input, .. } = block
                && name == "knowledge_search"
            {
                return input
                    .get("query")
                    .and_then(|v| v.as_str())
                    .map(String::from);
            }
        }
    }
    None
}

/// Verify knowledge search by replaying the ghost's query directly against the engine
async fn verify_knowledge_search(
    state: &AppState,
    pool: &KomaDbPool,
    ghost: &t_koma_db::Ghost,
    operator: &t_koma_db::Operator,
) -> Option<KnowledgeVerification> {
    let session = SessionRepository::get_active(pool.pool(), &ghost.id, &operator.id)
        .await
        .ok()
        .flatten()?;

    let messages = SessionRepository::list_messages(pool.pool(), &session.id)
        .await
        .ok()?;

    let query_text = match extract_knowledge_query(&messages) {
        Some(q) => q,
        None => {
            println!(
                "{}⚠{} Ghost never called knowledge_search — skipping verification\n",
                style::YELLOW,
                style::RESET
            );
            return None;
        }
    };

    info("Replaying query", &query_text);

    let query = KnowledgeSearchQuery {
        query: query_text.clone(),
        categories: None,
        scope: OwnershipScope::All,
        topic: None,
        archetype: None,
        options: Default::default(),
    };

    let results = state
        .knowledge_engine()
        .knowledge_search(&ghost.name, query)
        .await
        .ok()?;

    let verification = KnowledgeVerification::from_results(query_text, &results);

    println!(
        "{}🔍 Knowledge search returned:{} {} results\n",
        style::GREEN,
        style::RESET,
        verification.total_results
    );

    for preview in &verification.top_results {
        println!("  {}•{} {}", style::DIM, style::RESET, preview);
    }
    println!();

    Some(verification)
}

/// Collect ALL files from data root (includes ghost workspace + shared notes/references)
fn collect_data_files(data_root: &PathBuf) -> Vec<DataFile> {
    let mut files = Vec::new();

    if data_root.exists() {
        for entry in walkdir::WalkDir::new(data_root)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                let path = entry.path();
                let rel_path = path.strip_prefix(data_root).unwrap_or(path);
                let path_str = rel_path.to_string_lossy().to_string();

                let metadata = fs::metadata(path).ok();
                let size = metadata.map(|m| m.len()).unwrap_or(0);

                // Preview for markdown files
                let preview = if path_str.ends_with(".md") {
                    fs::read_to_string(path)
                        .ok()
                        .map(|c| c.chars().take(200).collect())
                } else {
                    None
                };

                files.push(DataFile {
                    path: path_str,
                    size_bytes: size,
                    preview,
                });
            }
        }
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

/// Collected results from the test run, passed to `write_report`.
struct ReportInput {
    model_used: String,
    operator: t_koma_db::Operator,
    ghost: t_koma_db::Ghost,
    session: t_koma_db::Session,
    conversation: Vec<DetailedMessage>,
    reflection: Option<DetailedReflection>,
    knowledge_verification: Option<KnowledgeVerification>,
    data_files: Vec<DataFile>,
}

/// Generate and write report
fn write_report(config: &TestConfig, input: ReportInput, started: Instant) -> PathBuf {
    fs::create_dir_all(&config.output_dir).expect("Failed to create output dir");

    let usage = UsageStats {
        total_messages: input.conversation.len(),
        operator_messages: input
            .conversation
            .iter()
            .filter(|m| m.role == "operator")
            .count(),
        ghost_messages: input
            .conversation
            .iter()
            .filter(|m| m.role == "ghost")
            .count(),
    };

    let report = TestReport {
        started_at: (Utc::now()
            - chrono::Duration::milliseconds(started.elapsed().as_millis() as i64))
        .to_rfc3339(),
        completed_at: Utc::now().to_rfc3339(),
        duration_seconds: started.elapsed().as_secs_f64(),
        config: config.clone(),
        model_used: input.model_used,
        operator: OperatorInfo {
            id: input.operator.id,
            name: input.operator.name,
            platform: "cli".to_string(),
            access_level: "puppet_master".to_string(),
        },
        ghost: GhostInfo {
            id: input.ghost.id.clone(),
            name: input.ghost.name.clone(),
            workspace_path: t_koma_db::ghosts::ghost_workspace_path(&input.ghost.name)
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
        },
        session: SessionInfo {
            id: input.session.id,
            created_at: Utc::now().to_rfc3339(),
            message_count: input.conversation.len(),
        },
        conversation: input.conversation,
        reflection: input.reflection,
        knowledge_verification: input.knowledge_verification,
        data_files: input.data_files,
        usage,
    };

    let path = config.output_dir.join("report.json");
    fs::write(&path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    path
}

/// Print final summary
fn print_summary(path: &std::path::Path, ghost: &t_koma_db::Ghost, data_root: &std::path::Path) {
    let report: TestReport = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();

    header("E2E TEST COMPLETE");

    println!(
        "{}📊 Report:{}{} {}",
        style::CYAN,
        style::RESET,
        style::BOLD,
        path.display()
    );
    println!(
        "{}⏱️  Duration:{}{} {:.2}s\n",
        style::YELLOW,
        style::RESET,
        style::BOLD,
        report.duration_seconds
    );

    println!(
        "{}💬 Conversation:{}{}",
        style::MAGENTA,
        style::RESET,
        style::BOLD
    );
    println!("   Operator: {} messages", report.usage.operator_messages);
    println!("   Ghost: {} messages\n", report.usage.ghost_messages);

    // Count tool uses
    let tool_uses: usize = report
        .conversation
        .iter()
        .map(|m| {
            m.content
                .iter()
                .filter(|b| b.get("type") == Some(&serde_json::json!("tool_use")))
                .count()
        })
        .sum();
    println!("   Tool calls: {}\n", tool_uses);

    println!(
        "{}📝 Data Files:{}{}",
        style::GREEN,
        style::RESET,
        style::BOLD
    );
    println!("   Total files: {}", report.data_files.len());

    let ghost_files = report
        .data_files
        .iter()
        .filter(|f| f.path.starts_with(&format!("ghosts/{}/", ghost.name)))
        .count();
    let shared_files = report
        .data_files
        .iter()
        .filter(|f| f.path.starts_with("shared/"))
        .count();
    println!("   Ghost workspace: {}", ghost_files);
    println!("   Shared: {}\n", shared_files);

    if let Some(refl) = &report.reflection {
        println!(
            "{}🔄 Reflection:{}{}",
            style::YELLOW,
            style::RESET,
            style::BOLD
        );
        println!("   Status: {}", refl.status);
        println!("   Transcript entries: {}\n", refl.transcript.len());
    }

    if let Some(kv) = &report.knowledge_verification {
        println!(
            "{}🔍 Knowledge Verification:{}{}",
            style::GREEN,
            style::RESET,
            style::BOLD
        );
        println!("   Query: \"{}\"", kv.query_used);
        println!("   Total results: {}", kv.total_results);
        println!(
            "   Breakdown: {} notes, {} diary, {} refs, {} topics\n",
            kv.notes_count, kv.diary_count, kv.references_count, kv.topics_count
        );
    }

    // Copy entire data directory for inspection
    let data_dst = path.parent().unwrap().join("data");
    if data_root.exists() {
        copy_dir(data_root, &data_dst).ok();
        println!(
            "{}📁 Data directory:{}{} {}",
            style::CYAN,
            style::RESET,
            style::BOLD,
            data_dst.display()
        );
    }

    println!("\n{}", style::RESET);
    println!("{}", "━".repeat(70));
    println!(
        "\n  Inspect: {}cat {} | jq .conversation{}",
        style::CYAN,
        path.display(),
        style::RESET
    );
    println!();
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let s = entry.path();
        let d = dst.join(entry.file_name());
        if s.is_dir() {
            copy_dir(&s, &d)?;
        } else {
            fs::copy(&s, &d)?;
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    t_koma_core::load_dotenv();

    let model_alias = parse_args();
    let started = Instant::now();
    let config = TestConfig::new();

    header("E2E KNOWLEDGE TEST");
    info("Model", &model_alias);
    info("Operator", &config.operator_name);
    info("Ghost", &config.ghost_name);
    info("Output", &config.output_dir.to_string_lossy());

    step(1, 6, "Setting up environment");
    let (state, pool) = setup_env(&config, &model_alias).await;
    success("Environment ready");

    step(2, 6, "Creating entities");
    let (operator, ghost) = create_entities(&pool, &config).await;

    step(3, 6, "Running conversation");
    let conversation = run_conversation(&state, &pool, &operator, &ghost).await;

    step(4, 6, "Running reflection");
    let reflection = run_reflection(&state, &pool, &operator, &ghost).await;

    step(5, 6, "Verifying knowledge search");
    let knowledge_verification = verify_knowledge_search(&state, &pool, &ghost, &operator).await;

    step(6, 6, "Generating report");
    let session = SessionRepository::get_active(pool.pool(), &ghost.id, &operator.id)
        .await
        .ok()
        .flatten()
        .expect("No active session");

    // Collect ALL files from data root (not just ghost workspace)
    let data_files = collect_data_files(&config.temp_dir);

    let path = write_report(
        &config,
        ReportInput {
            model_used: model_alias.clone(),
            operator,
            ghost: ghost.clone(),
            session,
            conversation,
            reflection,
            knowledge_verification,
            data_files,
        },
        started,
    );
    success("Report generated");

    print_summary(&path, &ghost, &config.temp_dir);
}
