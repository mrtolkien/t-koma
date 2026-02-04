use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ToolContext {
    ghost_name: String,
    workspace_root: PathBuf,
    cwd: PathBuf,
    allow_outside_workspace: bool,
    dirty: bool,
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
            dirty: false,
        }
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

    pub fn set_cwd(&mut self, cwd: PathBuf) {
        if self.cwd != cwd {
            self.cwd = cwd;
            self.dirty = true;
        }
    }

    pub fn set_allow_outside_workspace(&mut self, allow: bool) {
        self.allow_outside_workspace = allow;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    pub fn new_for_tests(root: &Path) -> Self {
        Self::new(
            "test-ghost".to_string(),
            root.to_path_buf(),
            root.to_path_buf(),
            false,
        )
    }
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
    let normalized = normalize_absolute_path(path);
    let workspace = normalize_absolute_path(context.workspace_root());
    normalized.starts_with(&workspace)
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
}
