use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::content::message::{MessageEntryRaw, MessageTemplate};
use crate::content::prompt::{PromptFrontMatter, PromptTemplate};
use crate::content::template::vars_from_pairs;
use crate::content::{ContentError, ContentScope};

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
        let messages_dir = content_dir("messages");
        let prompts_dir = content_dir("prompts");

        registry.load_messages(&messages_dir)?;
        registry.load_prompts(&prompts_dir)?;

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

    fn load_messages(&mut self, dir: &Path) -> Result<(), ContentError> {
        let mut stack = vec![dir.to_path_buf()];
        while let Some(path) = stack.pop() {
            let entries = fs::read_dir(&path).map_err(|e| ContentError::Io(path.clone(), e))?;
            for entry in entries {
                let entry = entry.map_err(|e| ContentError::Io(path.clone(), e))?;
                let entry_path = entry.path();
                if entry_path.is_dir() {
                    stack.push(entry_path);
                    continue;
                }
                if entry_path.extension().and_then(|s| s.to_str()) != Some("toml") {
                    continue;
                }

                let raw_text =
                    fs::read_to_string(&entry_path).map_err(|e| ContentError::Io(entry_path.clone(), e))?;
                let doc: toml::Value = toml::from_str(&raw_text)
                    .map_err(|e| ContentError::Parse(format!("{}: {}", entry_path.display(), e)))?;
                let table = doc
                    .as_table()
                    .ok_or_else(|| ContentError::Parse(format!("{}: expected table", entry_path.display())))?;

                for (id, value) in table {
                    let entry_table = value
                        .as_table()
                        .ok_or_else(|| ContentError::Parse(format!("{}: {} must be table", entry_path.display(), id)))?;
                    let entry_raw: MessageEntryRaw = toml::Value::Table(entry_table.clone())
                        .try_into()
                        .map_err(|e| ContentError::Parse(format!("{}: {}", entry_path.display(), e)))?;

                    let template = MessageTemplate::from_entry(id.clone(), entry_raw)?;
                    let variants = self.messages.entry(template.id.clone()).or_default();
                    insert_message_variant(variants, template)?;
                }
            }
        }
        Ok(())
    }

    fn load_prompts(&mut self, dir: &Path) -> Result<(), ContentError> {
        let entries = fs::read_dir(dir).map_err(|e| ContentError::Io(dir.to_path_buf(), e))?;
        for entry in entries {
            let entry = entry.map_err(|e| ContentError::Io(dir.to_path_buf(), e))?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| {
                ContentError::Parse(format!("Invalid filename: {}", path.display()))
            })?;
            let (id_from_name, suffix) = parse_filename(stem)?;

            let raw_text = fs::read_to_string(&path).map_err(|e| ContentError::Io(path.clone(), e))?;
            let (front_matter, body) = split_front_matter(&raw_text)
                .map_err(|e| ContentError::Parse(format!("{}: {}", path.display(), e)))?;
            let front: PromptFrontMatter = toml::from_str(&front_matter)
                .map_err(|e| ContentError::Parse(format!("{}: {}", path.display(), e)))?;
            let template = PromptTemplate::from_parts(front, body, path.clone())?;

            validate_template_identity(&template.id, &id_from_name, &template.scope, suffix.as_deref())?;

            let variants = self.prompts.entry(template.id.clone()).or_default();
            insert_prompt_variant(variants, template)?;
        }
        Ok(())
    }
}

fn content_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(name)
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
