use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Reason why a tool requires operator approval before proceeding.
///
/// Each variant carries the data needed to render an appropriate approval
/// message in the transport layer (WebSocket, Discord, etc.).
#[derive(Debug, Clone)]
pub enum ApprovalReason {
    /// Tool wants to access a path outside the ghost workspace.
    WorkspaceEscape(String),
    /// Tool wants to import external sources into a reference topic (potentially large fetch).
    ReferenceImport { title: String, summary: String },
}

impl ApprovalReason {
    /// Parse an approval reason from a tool error string.
    ///
    /// Returns `None` if the error does not start with `APPROVAL_REQUIRED:`.
    /// Backward compatible: bare path strings become `WorkspaceEscape`.
    pub fn parse(error: &str) -> Option<Self> {
        let payload = error.strip_prefix(APPROVAL_REQUIRED_PREFIX)?;
        let payload = payload.trim();
        if payload.is_empty() {
            return None;
        }

        // JSON payload → structured reason
        if payload.starts_with('{')
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(payload)
            && let Some(reason_type) = value.get("reason").and_then(|v| v.as_str())
        {
            return match reason_type {
                "reference_import" => Some(ApprovalReason::ReferenceImport {
                    title: value
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    summary: value
                        .get("summary")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                }),
                _ => None,
            };
        }

        // Bare string → workspace escape (backward compatible)
        Some(ApprovalReason::WorkspaceEscape(payload.to_string()))
    }

    /// Format this reason as a tool error string.
    pub fn to_error(&self) -> String {
        match self {
            ApprovalReason::WorkspaceEscape(path) => {
                format!("{}{}", APPROVAL_REQUIRED_PREFIX, path)
            }
            ApprovalReason::ReferenceImport { title, summary } => {
                let json = serde_json::json!({
                    "reason": "reference_import",
                    "title": title,
                    "summary": summary,
                });
                format!("{}{}", APPROVAL_REQUIRED_PREFIX, json)
            }
        }
    }

    /// Denial message shown to the ghost when the operator denies approval.
    pub fn denial_message(&self) -> &'static str {
        match self {
            ApprovalReason::WorkspaceEscape(_) => {
                "Error: Operator denied approval to leave the workspace."
            }
            ApprovalReason::ReferenceImport { .. } => {
                "Error: Operator denied approval to import this reference topic."
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolContext {
    ghost_name: String,
    workspace_root: PathBuf,
    cwd: PathBuf,
    allow_outside_workspace: bool,
    operator_access_level: t_koma_db::OperatorAccessLevel,
    allow_workspace_escape: bool,
    approved_actions: Vec<String>,
    dirty: bool,
    knowledge_engine: Option<Arc<t_koma_knowledge::KnowledgeEngine>>,
}

pub const APPROVAL_REQUIRED_PREFIX: &str = "APPROVAL_REQUIRED:";

impl ToolContext {
    pub fn new(
        ghost_name: String,
        workspace_root: PathBuf,
        cwd: PathBuf,
        allow_outside_workspace: bool,
    ) -> Self {
        Self {
            ghost_name,
            workspace_root,
            cwd,
            allow_outside_workspace,
            operator_access_level: t_koma_db::OperatorAccessLevel::Standard,
            allow_workspace_escape: false,
            approved_actions: Vec::new(),
            dirty: false,
            knowledge_engine: None,
        }
    }

    pub fn with_knowledge_engine(mut self, engine: Arc<t_koma_knowledge::KnowledgeEngine>) -> Self {
        self.knowledge_engine = Some(engine);
        self
    }

    pub fn knowledge_engine(&self) -> Option<&Arc<t_koma_knowledge::KnowledgeEngine>> {
        self.knowledge_engine.as_ref()
    }

    pub fn ghost_name(&self) -> &str {
        &self.ghost_name
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn allow_outside_workspace(&self) -> bool {
        self.allow_outside_workspace
    }

    pub fn operator_access_level(&self) -> t_koma_db::OperatorAccessLevel {
        self.operator_access_level
    }

    pub fn set_operator_access_level(&mut self, level: t_koma_db::OperatorAccessLevel) {
        self.operator_access_level = level;
    }

    pub fn allow_workspace_escape(&self) -> bool {
        self.allow_workspace_escape
    }

    pub fn set_allow_workspace_escape(&mut self, allow: bool) {
        self.allow_workspace_escape = allow;
    }

    pub fn set_cwd(&mut self, cwd: PathBuf) {
        if self.cwd != cwd {
            self.cwd = cwd;
            self.dirty = true;
        }
    }

    pub fn set_allow_outside_workspace(&mut self, allow: bool) {
        self.allow_outside_workspace = allow;
    }

    /// Grant a one-shot named approval (consumed on first `has_approval` check).
    pub fn grant_approval(&mut self, action: &str) {
        self.approved_actions.push(action.to_string());
    }

    /// Check (and consume) a named approval. Returns `true` if it was granted.
    pub fn has_approval(&mut self, action: &str) -> bool {
        if let Some(pos) = self.approved_actions.iter().position(|a| a == action) {
            self.approved_actions.swap_remove(pos);
            true
        } else {
            false
        }
    }

    /// Apply the appropriate context changes when an approval reason is granted.
    pub fn apply_approval(&mut self, reason: &ApprovalReason) {
        match reason {
            ApprovalReason::WorkspaceEscape(_) => {
                self.set_allow_outside_workspace(true);
            }
            ApprovalReason::ReferenceImport { .. } => {
                self.grant_approval("reference_import");
            }
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    pub fn new_for_tests(root: &Path) -> Self {
        Self {
            ghost_name: "test-ghost".to_string(),
            workspace_root: root.to_path_buf(),
            cwd: root.to_path_buf(),
            allow_outside_workspace: false,
            operator_access_level: t_koma_db::OperatorAccessLevel::Standard,
            allow_workspace_escape: false,
            approved_actions: Vec::new(),
            dirty: false,
            knowledge_engine: None,
        }
    }
}

pub fn resolve_local_path_unchecked(context: &ToolContext, raw_path: &str) -> PathBuf {
    let input_path = Path::new(raw_path);
    let absolute = if input_path.is_absolute() {
        input_path.to_path_buf()
    } else {
        context.cwd().join(input_path)
    };

    normalize_absolute_path(&absolute)
}

pub fn resolve_local_path(context: &mut ToolContext, raw_path: &str) -> Result<PathBuf, String> {
    let input_path = Path::new(raw_path);
    let absolute = if input_path.is_absolute() {
        input_path.to_path_buf()
    } else {
        context.cwd().join(input_path)
    };

    let normalized = normalize_absolute_path(&absolute);

    if !is_within_workspace(context, &normalized) {
        if context.allow_outside_workspace() {
            context.set_allow_outside_workspace(false);
            return Ok(normalized);
        }

        let normalized_cwd = normalize_absolute_path(context.cwd());
        if !is_within_workspace(context, &normalized_cwd) && normalized.starts_with(&normalized_cwd)
        {
            return Ok(normalized);
        }

        return Err(format!(
            "{}{}",
            APPROVAL_REQUIRED_PREFIX,
            normalized.display()
        ));
    }

    Ok(normalized)
}

pub fn approval_required_path(error: &str) -> Option<&str> {
    error
        .strip_prefix(APPROVAL_REQUIRED_PREFIX)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
}

pub fn is_within_workspace(context: &ToolContext, path: &Path) -> bool {
    let normalized = canonicalize_for_boundary_check(path);
    let workspace = canonicalize_for_boundary_check(context.workspace_root());
    normalized.starts_with(&workspace)
}

fn canonicalize_for_boundary_check(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }

    // For non-existent paths, resolve the nearest existing ancestor and
    // re-attach the unresolved suffix to keep checks symlink-aware.
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    let mut probe = path;
    loop {
        if let Ok(canonical_ancestor) = std::fs::canonicalize(probe) {
            let mut rebuilt = canonical_ancestor;
            for part in suffix.iter().rev() {
                rebuilt.push(part);
            }
            return rebuilt;
        }

        let Some(file_name) = probe.file_name() else {
            break;
        };
        suffix.push(file_name.to_os_string());

        let Some(parent) = probe.parent() else {
            break;
        };
        probe = parent;
    }

    normalize_absolute_path(path)
}

fn normalize_absolute_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    let mut has_root = false;
    let mut stack: Vec<std::ffi::OsString> = Vec::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
                stack.clear();
                stack.push(prefix.as_os_str().to_os_string());
            }
            Component::RootDir => {
                has_root = true;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !stack.is_empty() {
                    stack.pop();
                }
            }
            Component::Normal(part) => stack.push(part.to_os_string()),
        }
    }

    if has_root {
        normalized.push(Path::new("/"));
    }

    for part in stack {
        normalized.push(part);
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn resolve_local_path_blocks_outside_workspace() {
        let workspace = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let mut context = ToolContext::new(
            "test-ghost".to_string(),
            workspace.path().to_path_buf(),
            workspace.path().to_path_buf(),
            false,
        );

        let result = resolve_local_path(&mut context, outside.path().to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn resolve_local_path_allows_inside_workspace() {
        let workspace = TempDir::new().unwrap();
        let inside_path = workspace.path().join("file.txt");
        let mut context = ToolContext::new(
            "test-ghost".to_string(),
            workspace.path().to_path_buf(),
            workspace.path().to_path_buf(),
            false,
        );

        let result = resolve_local_path(&mut context, inside_path.to_str().unwrap());
        assert!(result.is_ok());
    }

    #[test]
    fn resolve_local_path_blocks_symlink_escape() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let workspace = TempDir::new().unwrap();
            let outside = TempDir::new().unwrap();
            let link = workspace.path().join("escape");
            symlink(outside.path(), &link).unwrap();

            let mut context = ToolContext::new(
                "test-ghost".to_string(),
                workspace.path().to_path_buf(),
                workspace.path().to_path_buf(),
                false,
            );

            let escaped = link.join("secrets.txt");
            let result = resolve_local_path(&mut context, escaped.to_str().unwrap());
            assert!(result.is_err());
        }
    }
}
