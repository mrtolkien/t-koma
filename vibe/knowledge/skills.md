# Agent Skills System

This document provides detailed information about the Agent Skills system in t-koma.

## Overview

Agent Skills are self-contained directories of instructions, scripts, and resources that extend the agent's capabilities. They follow the [Agent Skills specification](https://agentskills.io/specification) originally developed by Anthropic.

## Why Skills?

Skills solve a critical problem: agents are capable but often lack the context needed to do real work reliably. Skills provide:

- **Domain expertise**: Package specialized knowledge into reusable instructions
- **New capabilities**: Extend agent functionality with scripts and tools
- **Repeatable workflows**: Turn multi-step tasks into consistent processes
- **Progressive disclosure**: Load metadata at startup, full content on demand

## Architecture

### Core Components

**`Skill` struct** (`t-koma-core/src/skills.rs`):
- Represents a loaded skill with metadata
- Parses YAML frontmatter from SKILL.md
- Loads full content on demand
- Lists scripts, references, and assets

**`SkillRegistry` struct** (`t-koma-core/src/skill_registry.rs`):
- Discovers skills from multiple directories (project + config)
- Manages skill access and search
- Provides skill listings for system prompts
- Config skills take precedence over project skills

**`DefaultSkillsManager` struct** (`t-koma-core/src/default_skills.rs`):
- Manages default skills embedded in the binary
- Writes default skills to config directory on first run
- Preserves user modifications (doesn't overwrite existing)

### Skill Storage Locations

Skills are discovered from multiple locations:

1. **Project directory**: `./default-prompts/skills/` - Version-controlled project skills
2. **User config directory**: `~/.config/t-koma/skills/` (XDG)

Config skills take precedence over project skills with the same name.

```
Project: ./default-prompts/skills/
├── skill-creator/         # Guide for creating skills
│   └── SKILL.md
└── README.md              # Documentation

User Config: ~/.config/t-koma/skills/  (Linux)
└── my-private-skill/      # User-specific skills
    └── SKILL.md
```

### Default Skills

Default skills are embedded at compile time using `include_str!`:

```rust
// t-koma-core/src/default_skills.rs
pub const DEFAULT_SKILLS: &[DefaultSkill] = &[DefaultSkill {
    name: "skill-creator",
    content: include_str!("../../default-prompts/skills/skill-creator/SKILL.md"),
}];
```

On initialization, default skills are written to the config directory if they don't
already exist. This allows users to:
- Have working skills out of the box
- Modify skills without losing defaults
- Reset skills by deleting from config directory

## SKILL.md Format

### Frontmatter (Required)

```yaml
---
name: skill-name
description: A description of what this skill does and when to use it.
license: MIT
compatibility: Requires git, docker, and internet access
metadata:
  author: your-name
  version: "1.0"
---
```

**Field Requirements:**

| Field | Required | Constraints |
|-------|----------|-------------|
| `name` | Yes | 1-64 chars, lowercase alphanumeric + hyphens only |
| `description` | Yes | 1-1024 chars, describes what and when |
| `license` | No | License name or file reference |
| `compatibility` | No | Environment requirements |
| `metadata` | No | Key-value pairs for extra info |

### Name Validation

- Must be 1-64 characters
- May only contain `a-z`, `0-9`, and `-`
- Must not start or end with `-`
- Must not contain consecutive hyphens (`--`)
- Must match parent directory name

### Body Content

The Markdown body contains skill instructions:

```markdown
# Skill Title

## Overview

Brief explanation of the skill's purpose.

## Steps

1. Step one
2. Step two

## Examples

Examples of inputs and outputs...

## Common Pitfalls

Things to watch out for...
```

## Directory Structure

```
skill-name/
├── SKILL.md          # Required: Instructions and metadata
├── scripts/          # Optional: Executable code
│   ├── script.py
│   └── helper.sh
├── references/       # Optional: Additional documentation
│   ├── REFERENCE.md
│   └── api-docs.md
└── assets/           # Optional: Static resources
    └── template.txt
```

### scripts/

Contains executable code that agents can run:

- Should be self-contained or document dependencies
- Include helpful error messages
- Handle edge cases gracefully

### references/

Contains additional documentation loaded on demand:

- Keep files focused and small
- Use descriptive names
- Reference from SKILL.md

### assets/

Contains static resources:

- Templates
- Configuration files
- Data files

## Progressive Disclosure

The skills system uses progressive disclosure for efficient context usage:

1. **Metadata** (~100 tokens): Loaded at startup for all skills
   - `name` and `description` only
   - Used for skill discovery and selection

2. **Instructions** (<5000 tokens): Loaded when skill is activated
   - Full SKILL.md content
   - Includes all guidance and examples

3. **Resources** (as needed): Loaded only when required
   - Reference files
   - Script contents
   - Assets

**Best Practice**: Keep SKILL.md under 500 lines. Move detailed content to `references/`.

## Using Skills in Code

### Discovering Skills

```rust
use t_koma_core::SkillRegistry;

// Create registry and discover skills (default-prompts + config)
let registry = SkillRegistry::new()?;

println!("Discovered {} skills", registry.len());
```

### Accessing Skills

```rust
// Get a skill by name
if let Some(skill) = registry.get("skill-creator") {
    println!("Found: {}", skill.name);
}

// List all skills
for (name, description) in registry.list_skills() {
    println!("{}: {}", name, description);
}

// Search skills
let results = registry.search("pdf");
```

### Loading Skill Content

```rust
// Load full content (mutates the skill)
let mut registry = registry;
let skill = registry.load_skill("skill-creator")?;

if let Some(content) = &skill.content {
    println!("Full content: {}", content);
}
```

### Reading Resources

```rust
let skill = registry.get("my-skill").unwrap();

// List available resources
let scripts = skill.list_scripts();
let references = skill.list_references();

// Read specific files
let script_content = skill.read_script("extract.py")?;
let ref_content = skill.read_reference("API.md")?;
```

## Integration with System Prompt

To make skills available to the agent, include them in the system prompt:

```rust
let registry = SkillRegistry::new(skills_path)?;

let skills_list = registry
    .list_skills()
    .iter()
    .map(|(name, desc)| format!("- {}: {}", name, desc))
    .collect::<Vec<_>>()
    .join("\n");

let system_prompt = format!(r#"
You have access to the following skills:
{}

When a task matches a skill, load it and follow its instructions.
"#, skills_list);
```

## Error Handling

The skills system uses `SkillError` for error handling:

```rust
use t_koma_core::skills::SkillError;

match Skill::from_file(path) {
    Ok(skill) => { /* use skill */ }
    Err(SkillError::Io(e)) => { /* handle IO error */ }
    Err(SkillError::YamlParse(e)) => { /* handle parse error */ }
    Err(SkillError::InvalidFormat(msg)) => { /* handle invalid format */ }
    Err(SkillError::MissingField(field)) => { /* handle missing field */ }
    Err(SkillError::NotFound(name)) => { /* handle not found */ }
}
```

## Creating Skills

### Step 1: Create Directory

```bash
mkdir default-prompts/skills/my-new-skill
```

### Step 2: Create SKILL.md

```bash
cat > default-prompts/skills/my-new-skill/SKILL.md << 'EOF'
---
name: my-new-skill
description: Description of what this skill does and when to use it.
---

# My New Skill

Instructions for the agent...
EOF
```

### Step 3: Add Optional Resources

```bash
mkdir default-prompts/skills/my-new-skill/scripts
mkdir default-prompts/skills/my-new-skill/references
# Add files as needed
```

### Step 4: Test

```rust
let skill = Skill::from_file(Path::new("default-prompts/skills/my-new-skill/SKILL.md"))?;
assert_eq!(skill.name, "my-new-skill");
```

## Testing Skills

### Unit Tests

```rust
#[test]
fn test_skill_parsing() {
    let skill = Skill::from_file(Path::new("default-prompts/skills/test/SKILL.md")).unwrap();
    assert_eq!(skill.name, "test");
    assert!(!skill.description.is_empty());
}
```

### Registry Tests

```rust
#[test]
fn test_registry_discovery() {
    let registry = SkillRegistry::new().unwrap();
    assert!(registry.has_skill("skill-creator"));
}
```

## Best Practices

### For Skill Authors

1. **Clear Naming**: Use descriptive, hyphenated names
2. **Good Descriptions**: Explain what AND when to use
3. **Progressive Disclosure**: Keep SKILL.md concise
4. **Concrete Examples**: Show inputs and outputs
5. **Error Handling**: Document common failures
6. **File References**: Use relative paths

### For System Integration

1. **Lazy Loading**: Don't load all skill content at startup
2. **Graceful Degradation**: Handle missing skills gracefully
3. **Search**: Use `registry.search()` for skill discovery
4. **Caching**: Consider caching loaded skill content

## Resources

- [Agent Skills Home](https://agentskills.io/home)
- [Agent Skills Specification](https://agentskills.io/specification)
- Example skills in `/default-prompts/skills/` directory
