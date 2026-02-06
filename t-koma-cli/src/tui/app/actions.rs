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

use t_koma_core::{ModelConfig, ProviderType, Settings, WsMessage, WsResponse};
use t_koma_db::{
    GhostDbPool, GhostRepository, OperatorAccessLevel, OperatorRepository, OperatorStatus,
    Platform, SessionRepository,
};

use crate::client::WsClient;

use super::{
    state::{Metrics, OperatorView},
    util::{load_disk_config, shell_quote, ws_url_for_cli},
    TuiApp,
};

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

        let ghosts = GhostRepository::list_all(db.pool()).await.unwrap_or_default();
        let ghost_count = ghosts.len();

        let mut recent_message_count = 0;
        let since = Utc::now().timestamp() - 300;
        for ghost in &ghosts {
            if let Ok(ghost_db) = GhostDbPool::new(&ghost.name).await
                && let Ok(count) = SessionRepository::count_messages_since(ghost_db.pool(), since).await
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
                self.ghosts = list;
                if self.content_idx >= self.ghosts.len() {
                    self.content_idx = self.ghosts.len().saturating_sub(1);
                }
            }
            Err(e) => self.status = format!("Ghost refresh failed: {}", e),
        }
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
        let Some(db) = &self.db else {
            self.status = "DB unavailable".to_string();
            return;
        };

        if let Err(e) = OperatorRepository::approve(db.pool(), &operator.id).await {
            self.status = format!("Approve failed: {}", e);
            return;
        }
        self.status = format!("Approved {}", operator.id);
        self.refresh_operators().await;
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

    pub(super) async fn set_operator_access_level(
        &mut self,
        operator_id: &str,
        input: &str,
    ) {
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

    pub(super) async fn set_operator_rate_limits(
        &mut self,
        operator_id: &str,
        input: &str,
    ) {
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

    pub(super) async fn set_operator_workspace_escape(
        &mut self,
        operator_id: &str,
        allow: bool,
    ) {
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
                if let Ok(path) = GhostDbPool::workspace_path_for(ghost_name)
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

        self.settings.default_model = alias.to_string();
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
                    fs::copy(&config_path, &backup_path).map_err(|copy_err| copy_err.to_string())?;
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
            Ok(Some(WsResponse::Error { message })) => {
                self.status = format!("Restart failed: {}", message);
            }
            _ => {
                self.status = "Restart requested".to_string();
            }
        }
    }
}
