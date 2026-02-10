use std::collections::HashMap;
use std::path::PathBuf;

use include_dir::{Dir, DirEntry, include_dir};

use crate::content::message::{MessageEntryRaw, MessageTemplate};
use crate::content::prompt::{PromptFrontMatter, PromptTemplate};
use crate::content::template::vars_from_pairs;
use crate::content::{ContentError, ContentScope};
use t_koma_core::GatewayMessage;

static MESSAGES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/messages");
static PROMPTS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../prompts/system");

/// Read an embedded prompt file by path (for `{{ include }}` resolution).
///
/// Simple filenames (e.g. `"system-prompt.md"`) are looked up directly in the
/// root prompts/system directory. Relative include paths are normalized from the
/// gateway prompt root (`prompts/system`).
pub fn read_embedded_prompt(path: &str) -> Result<String, ContentError> {
    // Fast path: simple filename lookup in prompts/system
    if let Some(file) = PROMPTS_DIR.get_file(path) {
        return file
            .contents_utf8()
            .map(|s| s.to_string())
            .ok_or_else(|| ContentError::Parse(format!("non-UTF-8 embedded file: {path}")));
    }

    // Normalize relative path from prompts/system to workspace-relative
    let normalized = normalize_include_path(path);
    let prompt_prefix = "prompts/system/";
    if let Some(relative) = normalized.strip_prefix(prompt_prefix)
        && let Some(file) = PROMPTS_DIR.get_file(relative)
    {
        return file
            .contents_utf8()
            .map(|s| s.to_string())
            .ok_or_else(|| ContentError::Parse(format!("non-UTF-8 embedded file: {path}")));
    }

    Err(ContentError::Io(
        PathBuf::from(path),
        std::io::Error::new(std::io::ErrorKind::NotFound, "embedded prompt not found"),
    ))
}

/// Normalize an include path that is relative to `prompts/system`.
///
/// Resolves `..` components to a workspace-relative prompt path.
fn normalize_include_path(path: &str) -> String {
    // Start conceptually at prompts/system/
    let mut stack: Vec<&str> = vec!["prompts", "system"];
    for component in path.split('/') {
        match component {
            ".." => {
                stack.pop();
            }
            "." | "" => {}
            c => stack.push(c),
        }
    }
    stack.join("/")
}

#[derive(Debug, Default)]
pub struct ContentRegistry {
    messages: HashMap<String, MessageVariants>,
    prompts: HashMap<String, PromptVariants>,
}

#[derive(Debug, Default)]
struct MessageVariants {
    shared: Option<MessageTemplate>,
    interface: HashMap<String, MessageTemplate>,
    provider: HashMap<String, MessageTemplate>,
}

#[derive(Debug, Default)]
struct PromptVariants {
    shared: Option<PromptTemplate>,
    interface: HashMap<String, PromptTemplate>,
    provider: HashMap<String, PromptTemplate>,
}

impl ContentRegistry {
    pub fn load() -> Result<Self, ContentError> {
        let mut registry = Self::default();
        registry.load_messages()?;
        registry.load_prompts()?;
        Ok(registry)
    }

    pub fn message_text(
        &self,
        id: &str,
        interface: Option<&str>,
        vars: &[(&str, &str)],
    ) -> Result<String, ContentError> {
        let template = self.message_template(id, interface)?;
        let vars = vars_from_pairs(vars);
        template.render_plain(&vars)
    }

    pub fn gateway_message(
        &self,
        id: &str,
        interface: Option<&str>,
        vars: &[(&str, &str)],
    ) -> Result<GatewayMessage, ContentError> {
        let template = self.message_template(id, interface)?;
        let vars = vars_from_pairs(vars);
        template.render_gateway(&vars)
    }

    pub fn prompt_text(
        &self,
        id: &str,
        provider: Option<&str>,
        vars: &[(&str, &str)],
    ) -> Result<String, ContentError> {
        let template = self.prompt_template(id, provider)?;
        let vars = vars_from_pairs(vars);
        template.render(&vars)
    }

    pub fn prompt_template(
        &self,
        id: &str,
        provider: Option<&str>,
    ) -> Result<&PromptTemplate, ContentError> {
        let variants = self
            .prompts
            .get(id)
            .ok_or_else(|| ContentError::MissingPrompt(id.to_string()))?;

        if let Some(provider) = provider
            && let Some(template) = variants.provider.get(provider)
        {
            return Ok(template);
        }

        variants
            .shared
            .as_ref()
            .ok_or_else(|| ContentError::MissingPrompt(id.to_string()))
    }

    pub fn message_template(
        &self,
        id: &str,
        interface: Option<&str>,
    ) -> Result<&MessageTemplate, ContentError> {
        let variants = self
            .messages
            .get(id)
            .ok_or_else(|| ContentError::MissingMessage(id.to_string()))?;

        if let Some(interface) = interface
            && let Some(template) = variants.interface.get(interface)
        {
            return Ok(template);
        }

        variants
            .shared
            .as_ref()
            .ok_or_else(|| ContentError::MissingMessage(id.to_string()))
    }

    fn load_messages(&mut self) -> Result<(), ContentError> {
        for file in collect_embedded_files(&MESSAGES_DIR) {
            let path = file.path();
            if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }

            let raw_text = file.contents_utf8().ok_or_else(|| {
                ContentError::Parse(format!("non-UTF-8 message file: {}", path.display()))
            })?;
            let doc: toml::Value = toml::from_str(raw_text)
                .map_err(|e| ContentError::Parse(format!("{}: {}", path.display(), e)))?;
            let table = doc.as_table().ok_or_else(|| {
                ContentError::Parse(format!("{}: expected table", path.display()))
            })?;

            for (id, value) in table {
                let entry_table = value.as_table().ok_or_else(|| {
                    ContentError::Parse(format!("{}: {} must be table", path.display(), id))
                })?;
                let entry_raw: MessageEntryRaw = toml::Value::Table(entry_table.clone())
                    .try_into()
                    .map_err(|e| ContentError::Parse(format!("{}: {}", path.display(), e)))?;

                let template = MessageTemplate::from_entry(id.clone(), entry_raw)?;
                let variants = self.messages.entry(template.id.clone()).or_default();
                insert_message_variant(variants, template)?;
            }
        }
        Ok(())
    }

    fn load_prompts(&mut self) -> Result<(), ContentError> {
        for file in PROMPTS_DIR.files() {
            let path = file.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| {
                ContentError::Parse(format!("Invalid filename: {}", path.display()))
            })?;
            let (id_from_name, suffix) = parse_filename(stem)?;

            let raw_text = file.contents_utf8().ok_or_else(|| {
                ContentError::Parse(format!("non-UTF-8 prompt file: {}", path.display()))
            })?;
            let (front_matter, body) = split_front_matter(raw_text)
                .map_err(|e| ContentError::Parse(format!("{}: {}", path.display(), e)))?;
            let front: PromptFrontMatter = toml::from_str(&front_matter)
                .map_err(|e| ContentError::Parse(format!("{}: {}", path.display(), e)))?;
            let template = PromptTemplate::from_parts(front, body)?;

            validate_template_identity(
                &template.id,
                &id_from_name,
                &template.scope,
                suffix.as_deref(),
            )?;

            let variants = self.prompts.entry(template.id.clone()).or_default();
            insert_prompt_variant(variants, template)?;
        }
        Ok(())
    }
}

fn collect_embedded_files<'a>(dir: &'a Dir<'a>) -> Vec<&'a include_dir::File<'a>> {
    let mut files = Vec::new();
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(d) => files.extend(collect_embedded_files(d)),
            DirEntry::File(f) => files.push(f),
        }
    }
    files
}

fn parse_filename(stem: &str) -> Result<(String, Option<String>), ContentError> {
    let mut parts = stem.split('@');
    let id = parts
        .next()
        .ok_or_else(|| ContentError::Parse(format!("Invalid filename: {stem}")))?
        .to_string();
    let suffix = parts.next().map(|s| s.to_string());
    if parts.next().is_some() {
        return Err(ContentError::Parse(format!(
            "Multiple @ in filename: {stem}"
        )));
    }
    Ok((id, suffix))
}

fn validate_template_identity(
    template_id: &str,
    filename_id: &str,
    scope: &ContentScope,
    suffix: Option<&str>,
) -> Result<(), ContentError> {
    if template_id != filename_id {
        return Err(ContentError::Parse(format!(
            "Template id '{}' does not match filename '{}'",
            template_id, filename_id
        )));
    }

    match (scope, suffix) {
        (ContentScope::Shared, None) => Ok(()),
        (ContentScope::Interface(name), Some(suffix)) if name == suffix => Ok(()),
        (ContentScope::Provider(name), Some(suffix)) if name == suffix => Ok(()),
        (ContentScope::Shared, Some(_)) => Err(ContentError::Parse(format!(
            "Shared scope requires no suffix for {}",
            filename_id
        ))),
        _ => Err(ContentError::Parse(format!(
            "Scope does not match filename suffix for {}",
            filename_id
        ))),
    }
}

fn insert_message_variant(
    variants: &mut MessageVariants,
    template: MessageTemplate,
) -> Result<(), ContentError> {
    let template_id = template.id.clone();
    let scope = template.scope.clone();
    match scope {
        ContentScope::Shared => {
            if variants.shared.is_some() {
                return Err(ContentError::Duplicate(template_id));
            }
            variants.shared = Some(template);
        }
        ContentScope::Interface(name) => {
            if variants.interface.insert(name.clone(), template).is_some() {
                return Err(ContentError::Duplicate(format!("{}@{}", template_id, name)));
            }
        }
        ContentScope::Provider(name) => {
            if variants.provider.insert(name.clone(), template).is_some() {
                return Err(ContentError::Duplicate(format!("{}@{}", template_id, name)));
            }
        }
    }
    Ok(())
}

fn insert_prompt_variant(
    variants: &mut PromptVariants,
    template: PromptTemplate,
) -> Result<(), ContentError> {
    let template_id = template.id.clone();
    let scope = template.scope.clone();
    match scope {
        ContentScope::Shared => {
            if variants.shared.is_some() {
                return Err(ContentError::Duplicate(template_id));
            }
            variants.shared = Some(template);
        }
        ContentScope::Interface(name) => {
            if variants.interface.insert(name.clone(), template).is_some() {
                return Err(ContentError::Duplicate(format!("{}@{}", template_id, name)));
            }
        }
        ContentScope::Provider(name) => {
            if variants.provider.insert(name.clone(), template).is_some() {
                return Err(ContentError::Duplicate(format!("{}@{}", template_id, name)));
            }
        }
    }
    Ok(())
}

fn split_front_matter(text: &str) -> Result<(String, String), String> {
    let mut lines = text.lines();
    let Some(first) = lines.next() else {
        return Err("Missing front matter".to_string());
    };
    if first.trim() != "+++" {
        return Err("Front matter must start with +++".to_string());
    }

    let mut front = Vec::new();
    for line in &mut lines {
        if line.trim() == "+++" {
            let body = lines.collect::<Vec<_>>().join("\n");
            return Ok((front.join("\n"), body));
        }
        front.push(line);
    }

    Err("Front matter must end with +++".to_string())
}
