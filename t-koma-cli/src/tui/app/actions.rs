use std::{
    fs,
    io::{self, Write},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::Utc;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{self, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use tempfile::NamedTempFile;

use t_koma_core::{GatewayMessageKind, ModelConfig, ProviderType, Settings, WsMessage, WsResponse};
use t_koma_db::{
    ContentBlock, Ghost, GhostRepository, JobLog, JobLogRepository, Message, OperatorAccessLevel,
    OperatorRepository, OperatorStatus, Platform, SessionRepository, ghosts::ghost_workspace_path,
};

use crate::client::WsClient;

use super::{
    TuiApp,
    state::{
        ContentView, GhostRow, Metrics, OperatorView, PromptKind, SelectionAction, SelectionItem,
        SelectionModal,
    },
    util::{load_disk_config, shell_quote, ws_url_for_cli},
};

const HEARTBEAT_IDLE_SECONDS: i64 = 15 * 60;
const HEARTBEAT_OK_TOKEN: &str = "HEARTBEAT_OK";

fn extract_message_text(message: &Message) -> String {
    let mut parts = Vec::new();
    for block in &message.content {
        if let ContentBlock::Text { text } = block
            && !text.trim().is_empty()
        {
            parts.push(text.trim());
        }
    }
    parts.join("\n")
}

fn strip_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        if ch == '<' {
            in_tag = true;
            out.push(' ');
            continue;
        }
        if ch == '>' {
            in_tag = false;
            out.push(' ');
            continue;
        }
        if !in_tag {
            out.push(ch);
        }
    }
    out = out.replace("&nbsp;", " ");
    out.trim_matches(|c: char| "*`~_".contains(c))
        .trim()
        .to_string()
}

fn is_heartbeat_ok(message: &Message) -> bool {
    let text = extract_message_text(message);
    if text.trim().is_empty() {
        return false;
    }
    let normalized = strip_markup(text.trim());
    normalized == HEARTBEAT_OK_TOKEN
}

fn format_heartbeat_status(next_due: Option<i64>) -> Option<String> {
    let now = Utc::now().timestamp();
    let due = next_due?;
    if due <= now {
        return Some("HEARTBEAT DUE".to_string());
    }
    let remaining = (due - now + 59) / 60;
    Some(format!("HEARTBEAT IN {}m", remaining))
}

impl TuiApp {
    pub(super) async fn refresh_metrics(&mut self) {
        self.metrics_last_refresh = std::time::Instant::now();

        let Some(db) = &self.db else {
            self.metrics = Metrics::default();
            return;
        };

        let operator_count = OperatorRepository::list_all(db.pool())
            .await
            .map(|list| list.len())
            .unwrap_or(0);

        let ghosts = GhostRepository::list_all(db.pool())
            .await
            .unwrap_or_default();
        let ghost_count = ghosts.len();

        let mut recent_message_count = 0;
        let since = Utc::now().timestamp() - 300;
        for ghost in &ghosts {
            if let Ok(count) =
                SessionRepository::count_messages_since(db.pool(), &ghost.id, since).await
            {
                recent_message_count += count;
            }
        }

        self.metrics = Metrics {
            operator_count,
            ghost_count,
            recent_message_count,
        };
    }

    pub(super) async fn refresh_operators(&mut self) {
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        let res = match self.operator_view {
            OperatorView::All => OperatorRepository::list_all(db.pool()).await,
            OperatorView::Pending => {
                OperatorRepository::list_by_status(db.pool(), OperatorStatus::Pending, None).await
            }
        };

        match res {
            Ok(list) => {
                self.operators = list;
                if self.content_idx >= self.operators.len() {
                    self.content_idx = self.operators.len().saturating_sub(1);
                }
            }
            Err(e) => self.status = format!("Operators refresh failed: {}", e),
        }
    }

    pub(super) async fn refresh_ghosts(&mut self) {
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        match GhostRepository::list_all(db.pool()).await {
            Ok(list) => {
                let mut rows = Vec::with_capacity(list.len());
                for ghost in list {
                    let heartbeat = self.compute_ghost_heartbeat_status(&ghost).await;
                    rows.push(GhostRow { ghost, heartbeat });
                }
                self.ghosts = rows;
                if self.content_idx >= self.ghosts.len() {
                    self.content_idx = self.ghosts.len().saturating_sub(1);
                }
            }
            Err(e) => self.status = format!("Ghost refresh failed: {}", e),
        }
    }

    async fn compute_ghost_heartbeat_status(&self, ghost: &Ghost) -> Option<String> {
        let db = self.db.as_ref()?;
        let sessions = SessionRepository::list(db.pool(), &ghost.id, &ghost.owner_operator_id)
            .await
            .ok()?;

        let mut next_due: Option<i64> = None;
        for session in sessions {
            if !session.is_active {
                continue;
            }
            let last_message = SessionRepository::get_last_message(db.pool(), &session.id)
                .await
                .ok()
                .flatten();
            if let Some(message) = &last_message
                && is_heartbeat_ok(message)
            {
                continue;
            }
            let due = session.updated_at + HEARTBEAT_IDLE_SECONDS;
            next_due = Some(match next_due {
                Some(current) => current.min(due),
                None => due,
            });
        }

        format_heartbeat_status(next_due)
    }

    pub(super) async fn add_operator(&mut self, input: &str) {
        if input.is_empty() {
            self.status = "Operator name is required".to_string();
            return;
        }
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        match OperatorRepository::create_new(
            db.pool(),
            input,
            Platform::Api,
            t_koma_db::OperatorAccessLevel::Standard,
        )
        .await
        {
            Ok(op) => match OperatorRepository::approve(db.pool(), &op.id).await {
                Ok(_) => {
                    self.status = format!("Created approved operator {}", op.id);
                    self.refresh_operators().await;
                    self.refresh_metrics().await;
                }
                Err(e) => self.status = format!("Approval failed: {}", e),
            },
            Err(e) => self.status = format!("Create operator failed: {}", e),
        }
    }

    pub(super) async fn approve_selected_operator(&mut self) {
        let Some(operator) = self.operators.get(self.content_idx) else {
            return;
        };
        let operator_id = operator.id.clone();
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        match self.approve_operator_via_gateway(&operator_id).await {
            Ok(notified) => {
                self.status = if notified {
                    format!("Approved {} (Discord prompt sent)", operator_id)
                } else {
                    format!("Approved {}", operator_id)
                };
                self.refresh_operators().await;
                return;
            }
            Err(e) => {
                tracing::warn!(
                    "Gateway approval path unavailable, using DB fallback: {}",
                    e
                );
            }
        }

        if let Err(e) = OperatorRepository::approve(db.pool(), &operator_id).await {
            self.status = format!("Approve failed: {}", e);
            return;
        }
        self.status = format!("Approved {} (local DB)", operator_id);
        self.refresh_operators().await;
    }

    async fn approve_operator_via_gateway(&self, operator_id: &str) -> Result<bool, String> {
        let ws_url = ws_url_for_cli(&self.settings.ws_url());
        let (tx, mut rx) = WsClient::connect(&ws_url)
            .await
            .map_err(|e| e.to_string())?;

        tx.send(WsMessage::ApproveOperator {
            operator_id: operator_id.to_string(),
        })
        .map_err(|_| "Failed to send approve_operator".to_string())?;

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return Err("Timed out waiting for gateway approve_operator response".to_string());
            }
            let remaining = deadline.saturating_duration_since(now);
            match tokio::time::timeout(remaining, rx.next()).await {
                Ok(Some(WsResponse::OperatorApproved {
                    operator_id: approved_id,
                    discord_notified,
                })) => {
                    if approved_id != operator_id {
                        return Err(format!(
                            "Gateway approved unexpected operator id: {}",
                            approved_id
                        ));
                    }
                    return Ok(discord_notified);
                }
                Ok(Some(WsResponse::Response { message, .. }))
                    if message.kind == GatewayMessageKind::Error =>
                {
                    return Err(message.text_fallback);
                }
                Ok(Some(_)) => {
                    // Ignore unrelated bootstrap/welcome responses on fresh WS connects.
                }
                Ok(None) => return Err("Gateway closed connection".to_string()),
                Err(_) => {
                    return Err(
                        "Timed out waiting for gateway approve_operator response".to_string()
                    );
                }
            }
        }
    }

    pub(super) async fn deny_selected_operator(&mut self) {
        let Some(operator) = self.operators.get(self.content_idx) else {
            return;
        };
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        if let Err(e) = OperatorRepository::deny(db.pool(), &operator.id).await {
            self.status = format!("Deny failed: {}", e);
            return;
        }
        self.status = format!("Denied {}", operator.id);
        self.refresh_operators().await;
    }

    pub(super) async fn set_operator_access_level(&mut self, operator_id: &str, input: &str) {
        let level = match input.trim().to_lowercase().as_str() {
            "puppet_master" | "puppet" | "pm" | "admin" => OperatorAccessLevel::PuppetMaster,
            "standard" | "std" => OperatorAccessLevel::Standard,
            _ => {
                self.status = "Access level must be puppet_master or standard".to_string();
                return;
            }
        };

        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        let result = OperatorRepository::set_access_level(db.pool(), operator_id, level).await;
        if let Err(e) = result {
            self.status = format!("Set access level failed: {}", e);
            return;
        }

        if level == OperatorAccessLevel::PuppetMaster {
            let _ = OperatorRepository::set_rate_limits(db.pool(), operator_id, None, None).await;
        }

        self.status = format!("Access level set for {}", operator_id);
        self.refresh_operators().await;
    }

    pub(super) async fn set_operator_rate_limits(&mut self, operator_id: &str, input: &str) {
        let trimmed = input.trim().to_lowercase();
        let (rate_5m, rate_1h) = if trimmed == "none" || trimmed == "off" {
            (None, None)
        } else {
            let parts: Vec<&str> = input.split(',').map(|v| v.trim()).collect();
            if parts.len() != 2 {
                self.status = "Use: 5m,1h or 'none'".to_string();
                return;
            }
            let rate_5m = match parts[0].parse::<i64>() {
                Ok(value) if value > 0 => value,
                _ => {
                    self.status = "5m limit must be a positive integer".to_string();
                    return;
                }
            };
            let rate_1h = match parts[1].parse::<i64>() {
                Ok(value) if value > 0 => value,
                _ => {
                    self.status = "1h limit must be a positive integer".to_string();
                    return;
                }
            };
            (Some(rate_5m), Some(rate_1h))
        };

        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        match OperatorRepository::set_rate_limits(db.pool(), operator_id, rate_5m, rate_1h).await {
            Ok(_) => {
                self.status = format!("Rate limits set for {}", operator_id);
                self.refresh_operators().await;
            }
            Err(e) => self.status = format!("Set rate limits failed: {}", e),
        }
    }

    pub(super) async fn disable_operator_rate_limits(&mut self, operator_id: &str) {
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        match OperatorRepository::set_rate_limits(db.pool(), operator_id, None, None).await {
            Ok(_) => {
                self.status = format!("Rate limits disabled for {}", operator_id);
                self.refresh_operators().await;
            }
            Err(e) => self.status = format!("Disable rate limits failed: {}", e),
        }
    }

    pub(super) async fn set_operator_workspace_escape(&mut self, operator_id: &str, allow: bool) {
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        match OperatorRepository::set_allow_workspace_escape(db.pool(), operator_id, allow).await {
            Ok(_) => {
                self.status = format!(
                    "Workspace escape {} for {}",
                    if allow { "enabled" } else { "disabled" },
                    operator_id
                );
                self.refresh_operators().await;
            }
            Err(e) => self.status = format!("Workspace escape update failed: {}", e),
        }
    }

    pub(super) async fn add_ghost(&mut self, input: &str) {
        let parts: Vec<&str> = input.split(',').map(|v| v.trim()).collect();
        if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
            self.status = "Use: owner_operator_id,ghost_name".to_string();
            return;
        }
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        match GhostRepository::create(db.pool(), parts[0], parts[1]).await {
            Ok(_) => {
                self.status = format!("Created ghost {}", parts[1]);
                self.refresh_ghosts().await;
                self.refresh_metrics().await;
            }
            Err(e) => self.status = format!("Create ghost failed: {}", e),
        }
    }

    pub(super) async fn delete_ghost_confirmed(&mut self, target: Option<&str>, typed_name: &str) {
        let Some(ghost_name) = target else {
            self.status = "Delete failed: no selected ghost".to_string();
            return;
        };
        if typed_name != ghost_name {
            self.status = "Delete aborted: name mismatch".to_string();
            return;
        }
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        match GhostRepository::delete_by_name(db.pool(), ghost_name).await {
            Ok(()) => {
                if let Ok(path) = ghost_workspace_path(ghost_name)
                    && path.exists()
                {
                    let _ = fs::remove_dir_all(&path);
                }
                self.status = format!("Deleted ghost {}", ghost_name);
                self.refresh_ghosts().await;
                self.refresh_metrics().await;
            }
            Err(e) => self.status = format!("Delete failed: {}", e),
        }
    }

    pub(super) fn add_model(&mut self, input: &str) {
        let parts: Vec<&str> = input.split(',').map(|v| v.trim()).collect();
        if parts.len() != 3 {
            self.status = "Use: alias,provider,model".to_string();
            return;
        }

        let provider = match parts[1].parse::<ProviderType>() {
            Ok(provider) => provider,
            Err(e) => {
                self.status = format!("Invalid provider: {}", e);
                return;
            }
        };

        self.settings.models.insert(
            parts[0].to_string(),
            ModelConfig {
                provider,
                model: parts[2].to_string(),
                base_url: None,
                api_key_env: None,
                routing: None,
                context_window: None,
                headers: None,
                retry_on_empty: None,
            },
        );
        self.settings_dirty = true;
        self.refresh_settings_toml();
        self.status = format!("Added model alias {}", parts[0]);
    }

    pub(super) fn set_default_model(&mut self, alias: &str) {
        if !self.settings.models.contains_key(alias) {
            self.status = format!("Unknown alias {}", alias);
            return;
        }

        self.settings.default_model = t_koma_core::ModelAliases::single(alias);
        self.settings_dirty = true;
        self.refresh_settings_toml();
        self.status = format!("Default model set to {}", alias);
    }

    pub(super) fn refresh_settings_toml(&mut self) {
        if let Ok(toml) = self.settings.to_toml() {
            self.settings_toml = toml;
        }
    }

    pub(super) fn reload_settings(&mut self) {
        match Settings::load() {
            Ok(settings) => {
                self.settings = settings;
                self.refresh_settings_toml();
                self.disk_toml = load_disk_config().unwrap_or_else(|| self.settings_toml.clone());
                self.settings_dirty = false;
                self.status = "Reloaded settings".to_string();
            }
            Err(e) => self.status = format!("Reload failed: {}", e),
        }
    }

    pub(super) fn save_settings(&mut self) {
        match self.settings.save() {
            Ok(()) => {
                self.settings_dirty = false;
                self.disk_toml = self.settings_toml.clone();
                self.status = "Settings saved".to_string();
            }
            Err(e) => self.status = format!("Save failed: {}", e),
        }
    }

    pub(super) fn restore_backup(&mut self) {
        let Some(path) = self.backup_path.clone() else {
            self.status = "No backup available".to_string();
            return;
        };

        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => {
                self.status = format!("Backup read failed: {}", e);
                return;
            }
        };

        let parsed = match Settings::from_toml(&content) {
            Ok(parsed) => parsed,
            Err(e) => {
                self.status = format!("Backup parse failed: {}", e);
                return;
            }
        };

        let config_path = match Settings::config_path() {
            Ok(path) => path,
            Err(e) => {
                self.status = format!("Config path failed: {}", e);
                return;
            }
        };

        if let Err(e) = fs::write(&config_path, content) {
            self.status = format!("Restore write failed: {}", e);
            return;
        }

        self.settings = parsed;
        self.refresh_settings_toml();
        self.disk_toml = self.settings_toml.clone();
        self.settings_dirty = false;
        self.status = format!("Restored backup {}", path.display());
    }

    pub(super) fn edit_in_editor(&mut self) -> Result<(), String> {
        let config_path = Settings::config_path().map_err(|e| e.to_string())?;
        let current_content =
            fs::read_to_string(&config_path).unwrap_or_else(|_| self.settings_toml.clone());

        let mut temp_file = NamedTempFile::new().map_err(|e| e.to_string())?;
        temp_file
            .write_all(current_content.as_bytes())
            .map_err(|e| e.to_string())?;
        let temp_path = temp_file.path().to_path_buf();

        terminal::disable_raw_mode().map_err(|e| e.to_string())?;
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);

        let escaped_path = shell_quote(temp_path.to_string_lossy().as_ref());
        let editor_cmd = format!("${{EDITOR:-vi}} {}", escaped_path);
        let status = Command::new("sh")
            .arg("-lc")
            .arg(editor_cmd)
            .status()
            .map_err(|e| e.to_string())?;

        let _ = execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture);
        terminal::enable_raw_mode().map_err(|e| e.to_string())?;
        let _ = execute!(io::stdout(), terminal::Clear(ClearType::All));

        if !status.success() {
            return Err("Editor exited with non-zero status".to_string());
        }

        let edited = fs::read_to_string(&temp_path).map_err(|e| e.to_string())?;
        match Settings::from_toml(&edited) {
            Ok(parsed) => {
                self.settings = parsed;
                self.refresh_settings_toml();
                self.settings_dirty = self.settings_toml != self.disk_toml;
                self.status = "Edited config loaded. Press Save to persist.".to_string();
                Ok(())
            }
            Err(e) => {
                let stamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|err| err.to_string())?
                    .as_millis();
                let backup_name = format!("config.toml.bak.{}", stamp);
                let backup_path = config_path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join(backup_name);
                if config_path.exists() {
                    fs::copy(&config_path, &backup_path)
                        .map_err(|copy_err| copy_err.to_string())?;
                    self.backup_path = Some(backup_path.clone());
                    self.status = format!(
                        "Invalid TOML rejected. Backup saved at {}",
                        backup_path.display()
                    );
                }
                Err(e.to_string())
            }
        }
    }

    pub(super) async fn restart_gateway(&mut self) {
        let ws_url = ws_url_for_cli(&self.settings.ws_url());
        let (tx, mut rx) = match WsClient::connect(&ws_url).await {
            Ok(pair) => pair,
            Err(e) => {
                self.status = format!("Gateway connect failed: {}", e);
                self.gate_connected = false;
                return;
            }
        };

        if tx.send(WsMessage::RestartGateway).is_err() {
            self.status = "Restart command send failed".to_string();
            return;
        }

        match tokio::time::timeout(std::time::Duration::from_secs(3), rx.next()).await {
            Ok(Some(WsResponse::GatewayRestarting)) => {
                self.status = "Gateway restarting...".to_string();
            }
            Ok(Some(WsResponse::Response { message, .. }))
                if message.kind == GatewayMessageKind::Error =>
            {
                self.status = format!("Restart failed: {}", message.text_fallback);
            }
            _ => {
                self.status = "Restart requested".to_string();
            }
        }
    }

    // ── Modal actions ────────────────────────────────────────────────

    pub(super) fn open_access_level_modal(
        &mut self,
        operator_id: String,
        operator_name: String,
        current: OperatorAccessLevel,
    ) {
        let selected = match current {
            OperatorAccessLevel::PuppetMaster => 0,
            OperatorAccessLevel::Standard => 1,
        };
        self.modal = Some(SelectionModal {
            title: format!("Access Level for {}", operator_name),
            items: vec![
                SelectionItem {
                    label: "Puppet Master".to_string(),
                    value: "puppet_master".to_string(),
                },
                SelectionItem {
                    label: "Standard".to_string(),
                    value: "standard".to_string(),
                },
            ],
            selected_idx: selected,
            on_select: SelectionAction::SetAccessLevel,
            context: Some(operator_id),
        });
    }

    pub(super) fn open_provider_selection_modal(&mut self) {
        self.modal = Some(SelectionModal {
            title: "Select Provider".to_string(),
            items: vec![
                SelectionItem {
                    label: "Anthropic".to_string(),
                    value: "anthropic".to_string(),
                },
                SelectionItem {
                    label: "OpenRouter".to_string(),
                    value: "openrouter".to_string(),
                },
                SelectionItem {
                    label: "Gemini (Google)".to_string(),
                    value: "gemini".to_string(),
                },
                SelectionItem {
                    label: "OpenAI Compatible".to_string(),
                    value: "openai_compatible".to_string(),
                },
                SelectionItem {
                    label: "Kimi Code".to_string(),
                    value: "kimi_code".to_string(),
                },
            ],
            selected_idx: 0,
            on_select: SelectionAction::SelectProvider,
            context: None,
        });
    }

    pub(super) async fn handle_modal_selection(&mut self, modal: SelectionModal) {
        let Some(selected) = modal.items.get(modal.selected_idx) else {
            return;
        };

        match modal.on_select {
            SelectionAction::SetAccessLevel => {
                let Some(operator_id) = modal.context else {
                    return;
                };
                self.set_operator_access_level(&operator_id, &selected.value)
                    .await;
            }
            SelectionAction::SelectProvider => {
                self.show_provider_instructions_and_prompt(&selected.value);
            }
        }
    }

    fn show_provider_instructions_and_prompt(&mut self, provider: &str) {
        self.begin_prompt(
            PromptKind::AddProviderApiKey,
            Some(provider.to_string()),
            None,
        );
    }

    pub(super) fn write_provider_api_key(&mut self, provider: &str, api_key: &str) {
        let env_var = match provider {
            "anthropic" => "ANTHROPIC_API_KEY",
            "openrouter" => "OPENROUTER_API_KEY",
            "gemini" => "GEMINI_API_KEY",
            "openai_compatible" => "OPENAI_API_KEY",
            "kimi_code" => "KIMI_API_KEY",
            _ => {
                self.status = format!("Unknown provider: {}", provider);
                return;
            }
        };

        // Get the config directory
        let config_path = match Settings::config_path() {
            Ok(path) => path,
            Err(e) => {
                self.status = format!("Failed to get config path: {}", e);
                return;
            }
        };

        let Some(config_dir) = config_path.parent() else {
            self.status = "Invalid config path".to_string();
            return;
        };

        let env_path = config_dir.join(".env");

        // Read existing .env content if it exists
        let existing_content = fs::read_to_string(&env_path).unwrap_or_default();

        // Check if this env var already exists
        let mut lines: Vec<String> = existing_content
            .lines()
            .filter(|line| !line.starts_with(&format!("{}=", env_var)))
            .map(|s| s.to_string())
            .collect();

        // Add the new line
        lines.push(format!("{}={}", env_var, api_key));

        // Write back to file
        match fs::write(&env_path, lines.join("\n") + "\n") {
            Ok(_) => {
                self.status = format!("{} written to {}", env_var, env_path.display());
            }
            Err(e) => {
                self.status = format!("Failed to write .env: {}", e);
            }
        }
    }

    // ── Job viewer actions ───────────────────────────────────────────

    pub(super) async fn refresh_jobs(&mut self, ghost_id: Option<&str>) {
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        let result = match ghost_id {
            Some(gid) => JobLogRepository::list_for_ghost(db.pool(), gid, 200).await,
            None => JobLogRepository::list_recent(db.pool(), 200).await,
        };

        match result {
            Ok(summaries) => {
                self.job_view.summaries = summaries;
                self.job_view.detail = None;
                self.content_view = ContentView::List;
                self.content_idx = 0;
                self.status = format!("{} job logs loaded", self.job_view.summaries.len());
            }
            Err(e) => self.status = format!("Job list failed: {}", e),
        }
    }

    pub(super) async fn drill_into_job(&mut self) {
        let Some(job) = self.job_view.summaries.get(self.content_idx) else {
            return;
        };
        let job_id = job.id.clone();
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        match JobLogRepository::get(db.pool(), &job_id).await {
            Ok(Some(log)) => {
                self.job_detail_scroll = last_entry_line_offset(&log);
                self.job_view.detail = Some(log);
                self.content_view = ContentView::JobDetail {
                    job_id: job_id.clone(),
                };
                self.content_idx = 0;
            }
            Ok(None) => self.status = "Job log not found".to_string(),
            Err(e) => self.status = format!("Job fetch failed: {}", e),
        }
    }

    // ── Session viewer actions ───────────────────────────────────────

    pub(super) async fn drill_into_ghost_sessions(&mut self) {
        let Some(ghost_row) = self.ghosts.get(self.content_idx) else {
            self.status = "No ghost selected".to_string();
            return;
        };
        let ghost_id = ghost_row.ghost.id.clone();
        let ghost_name = ghost_row.ghost.name.clone();

        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        match SessionRepository::list_for_ghost(db.pool(), &ghost_id).await {
            Ok(sessions) => {
                self.session_view.sessions = sessions;
                self.content_view = ContentView::GhostSessions {
                    ghost_id,
                    ghost_name,
                };
                self.content_idx = 0;
            }
            Err(e) => self.status = format!("Session list failed: {}", e),
        }
    }

    pub(super) async fn drill_into_session_messages(&mut self) {
        let Some(sess) = self.session_view.sessions.get(self.content_idx) else {
            return;
        };
        let session_id = sess.id.clone();
        let ghost_name = match &self.content_view {
            ContentView::GhostSessions { ghost_name, .. } => ghost_name.clone(),
            _ => "?".to_string(),
        };

        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        match SessionRepository::get_messages(db.pool(), &session_id).await {
            Ok(messages) => {
                self.session_view.scroll = last_message_line_offset(&messages);
                self.session_view.messages = messages;
                self.content_view = ContentView::SessionMessages {
                    ghost_name,
                    session_id,
                };
            }
            Err(e) => self.status = format!("Messages fetch failed: {}", e),
        }
    }

    // ── Knowledge actions ────────────────────────────────────────────

    /// First ghost name if any ghosts are loaded, for knowledge queries.
    fn first_ghost_name(&self) -> Option<String> {
        self.ghosts.first().map(|g| g.ghost.name.clone())
    }

    pub(super) async fn refresh_knowledge_recent(&mut self) {
        match self
            .ws_query(WsMessage::ListRecentNotes {
                ghost_name: self.first_ghost_name(),
                limit: Some(50),
            })
            .await
        {
            Ok(WsResponse::RecentNotes { notes }) => {
                self.knowledge_view.notes = notes;
                self.knowledge_view.detail_title = None;
                self.knowledge_view.detail_body = None;
                self.content_idx = 0;
                self.status = format!("{} notes loaded", self.knowledge_view.notes.len());
            }
            Ok(WsResponse::Response { message, .. })
                if message.kind == GatewayMessageKind::Error =>
            {
                self.status = format!("Knowledge: {}", message.text_fallback);
            }
            Ok(_) => self.status = "Unexpected knowledge response".to_string(),
            Err(e) => self.status = format!("Knowledge: {}", e),
        }
    }

    pub(super) async fn search_knowledge(&mut self, query: &str) {
        if query.is_empty() {
            self.refresh_knowledge_recent().await;
            return;
        }

        match self
            .ws_query(WsMessage::SearchKnowledge {
                ghost_name: self.first_ghost_name(),
                query: query.to_string(),
                max_results: Some(30),
            })
            .await
        {
            Ok(WsResponse::KnowledgeSearchResults { results }) => {
                self.knowledge_view.notes = results;
                self.content_idx = 0;
                self.status = format!("{} search results", self.knowledge_view.notes.len());
            }
            Ok(WsResponse::Response { message, .. })
                if message.kind == GatewayMessageKind::Error =>
            {
                self.status = format!("Search: {}", message.text_fallback);
            }
            Ok(_) => self.status = "Unexpected search response".to_string(),
            Err(e) => self.status = format!("Search: {}", e),
        }
    }

    pub(super) async fn drill_into_knowledge_entry(&mut self) {
        let Some(note) = self.knowledge_view.notes.get(self.content_idx) else {
            return;
        };
        let note_id = note.id.clone();

        match self
            .ws_query(WsMessage::GetKnowledgeEntry {
                id: note_id.clone(),
                max_chars: None,
            })
            .await
        {
            Ok(WsResponse::KnowledgeEntry { title, body, .. }) => {
                self.knowledge_view.detail_title = Some(title);
                self.knowledge_view.detail_body = Some(body);
                self.knowledge_view.scroll = 0;
                self.content_view = ContentView::KnowledgeDetail { note_id };
            }
            Ok(WsResponse::Response { message, .. })
                if message.kind == GatewayMessageKind::Error =>
            {
                self.status = format!("Knowledge: {}", message.text_fallback);
            }
            Ok(_) => self.status = "Unexpected knowledge response".to_string(),
            Err(e) => self.status = format!("Knowledge: {}", e),
        }
    }

    pub(super) async fn refresh_knowledge_stats(&mut self) {
        match self.ws_query(WsMessage::GetKnowledgeStats).await {
            Ok(WsResponse::KnowledgeStats { stats }) => {
                self.knowledge_view.stats = Some(stats);
                self.content_view = ContentView::KnowledgeStats;
                self.status = "Index stats loaded".to_string();
            }
            Ok(WsResponse::Response { message, .. })
                if message.kind == GatewayMessageKind::Error =>
            {
                self.status = format!("Stats: {}", message.text_fallback);
            }
            Ok(_) => self.status = "Unexpected stats response".to_string(),
            Err(e) => self.status = format!("Stats: {}", e),
        }
    }

    // ── WS query helper ──────────────────────────────────────────────

    async fn ws_query(&self, message: WsMessage) -> Result<WsResponse, String> {
        let ws_url = ws_url_for_cli(&self.settings.ws_url());
        let (tx, mut rx) = WsClient::connect(&ws_url)
            .await
            .map_err(|e| e.to_string())?;

        tx.send(message)
            .map_err(|_| "Failed to send WS message".to_string())?;

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return Err("Timed out waiting for gateway response".to_string());
            }
            let remaining = deadline.saturating_duration_since(now);
            match tokio::time::timeout(remaining, rx.next()).await {
                Ok(Some(resp)) => {
                    if is_substantive_response(&resp) {
                        return Ok(resp);
                    }
                }
                Ok(None) => return Err("Gateway closed connection".to_string()),
                Err(_) => return Err("Timed out waiting for gateway response".to_string()),
            }
        }
    }
}

fn last_entry_line_offset(job: &JobLog) -> u16 {
    let mut offset: u16 = 3; // header lines: title, status, blank
    let mut last_start = offset;
    for entry in &job.transcript {
        last_start = offset;
        offset += 1; // role header
        for block in &entry.content {
            offset += content_block_line_count(block);
        }
        offset += 1; // blank separator
    }
    last_start
}

fn last_message_line_offset(messages: &[Message]) -> u16 {
    let mut offset: u16 = 0;
    let mut last_start: u16 = 0;
    for msg in messages {
        last_start = offset;
        offset += 1; // role header
        for block in &msg.content {
            offset += content_block_line_count(block);
        }
        offset += 1; // blank separator
    }
    last_start
}

fn content_block_line_count(block: &ContentBlock) -> u16 {
    match block {
        ContentBlock::Text { text } => text.lines().count().max(1) as u16,
        ContentBlock::ToolUse { .. }
        | ContentBlock::ToolResult { .. }
        | ContentBlock::Image { .. }
        | ContentBlock::File { .. } => 1,
    }
}

fn is_substantive_response(resp: &WsResponse) -> bool {
    !matches!(
        resp,
        WsResponse::Pong | WsResponse::GhostList { .. } | WsResponse::GhostSelected { .. }
    )
}
