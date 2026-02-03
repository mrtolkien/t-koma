# Agent Skills System Specification

## Overview

Implement an Agent Skills system for t-koma that allows the agent to discover, load, and use specialized skills stored in the configuration folder (`/skills`). Skills are folders containing instructions, scripts, and resources that enhance the agent's capabilities with procedural knowledge and domain-specific context.

The implementation follows the open Agent Skills specification originally developed by Anthropic.

## Goals

1. **Skill Discovery and Loading**:
   - Implement skill discovery from the configuration directory
   - Parse `SKILL.md` files with YAML frontmatter
   - Support the standard skill directory structure

2. **Skill Registry**:
   - Create a `SkillRegistry` to manage available skills
   - Support loading skill metadata (name, description) at startup
   - Support loading full skill content on demand

3. **Default Skills Directory**:
   - Create a `/skills` directory in the project root
   - Add example skills for common tasks
   - Document the skill format

4. **Integration with System Prompt**:
   - Skills should be discoverable by the agent
   - Provide skill information to the model when relevant

## Architecture

### Directory Structure

```
t-koma/
├── default-prompts/skills/          # Skills configuration folder
│   ├── skill-creator/               # Example: Skill creation guide
│   │   └── SKILL.md
│   └── README.md                    # Skills folder documentation
├── t-koma-core/
│   └── src/
│       ├── lib.rs
│       ├── skills.rs                # Skill struct and parser
│       └── skill_registry.rs        # Skill discovery and registry
├── vibe/
│   ├── knowledge/
│   │   └── skills.md                # Knowledge base for skills
```

### Skill Struct

```rust
/// Represents a loaded skill
#[derive(Debug, Clone)]
pub struct Skill {
    /// Skill name (from frontmatter)
    pub name: String,
    /// Skill description (from frontmatter)
    pub description: String,
    /// Optional license information
    pub license: Option<String>,
    /// Optional compatibility notes
    pub compatibility: Option<String>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
    /// Path to the skill directory
    pub path: PathBuf,
    /// Full content of SKILL.md (loaded on demand)
    pub content: Option<String>,
}

impl Skill {
    /// Parse a skill from a SKILL.md file path
    pub fn from_file(path: &Path) -> Result<Self, SkillError>;
    
    /// Load the full content of the skill
    pub async fn load_content(&mut self) -> Result<(), SkillError>;
    
    /// Get a list of available scripts in the skill's scripts/ directory
    pub fn list_scripts(&self) -> Vec<PathBuf>;
    
    /// Get a list of available references in the skill's references/ directory
    pub fn list_references(&self) -> Vec<PathBuf>;
}
```

### SkillRegistry

```rust
/// Registry for managing available skills
#[derive(Debug, Clone)]
pub struct SkillRegistry {
    /// Map of skill name to skill metadata
    skills: HashMap<String, Skill>,
    /// Base path for skill discovery
    skills_path: PathBuf,
}

impl SkillRegistry {
    /// Create a new registry and discover skills at the given path
    pub async fn new(skills_path: PathBuf) -> Result<Self, SkillError>;
    
    /// Discover all skills in the skills directory
    pub async fn discover_skills(&mut self) -> Result<(), SkillError>;
    
    /// Get a skill by name
    pub fn get(&self, name: &str) -> Option<&Skill>;
    
    /// Get all skill names and descriptions (for system prompt)
    pub fn list_skills(&self) -> Vec<(String, String)>;
    
    /// Load full content of a specific skill
    pub async fn load_skill(&mut self, name: &str) -> Result<&Skill, SkillError>;
}
```

### SKILL.md Format

```yaml
---
name: skill-name
description: A description of what this skill does and when to use it.
license: Apache-2.0
compatibility: Designed for Claude Code (or similar products)
metadata:
  author: example-org
  version: "1.0"
---

# Skill Instructions

Step-by-step instructions for the agent...

## Examples

Examples of inputs and outputs...
```

## Implementation Steps

1. **Create `t-koma-core/src/skills.rs`**:
   - Define `Skill` struct with metadata fields
   - Implement YAML frontmatter parser
   - Add methods for loading content and listing resources

2. **Create `t-koma-core/src/skill_registry.rs`**:
   - Define `SkillRegistry` struct
   - Implement skill discovery from directory
   - Add methods for accessing skills

3. **Add Error Type**:
   - Create `SkillError` enum for skill-related errors
   - Include variants for parsing errors, IO errors, invalid format

4. **Create Default Skills Directory**:
   - Create `/skills` folder in project root
   - Add `README.md` explaining the skills system
   - Create example `skill-creator` skill

5. **Update Configuration**:
   - Add `skills_path` to `Config` struct
   - Default to `{data_dir}/skills` or project-relative path

6. **Create Knowledge Base**:
   - Write `vibe/knowledge/skills.md` with detailed information
   - Update `AGENTS.md` with skills usage information

7. **Add Tests**:
   - Unit tests for skill parsing
   - Unit tests for registry discovery
   - Test with example skills

## Testing

### Unit Tests

- Test YAML frontmatter parsing
- Test skill discovery from directory
- Test skill content loading
- Test error handling for invalid skills

### Integration

- Verify skills are discoverable at startup
- Test skill content loading on demand
- Ensure skills don't break existing functionality

## Notes

- Skills follow the open Agent Skills specification
- Each skill is self-contained in its own directory
- The `SKILL.md` file is required; other directories are optional
- Skills are loaded progressively: metadata first, full content on demand
- The skills system is additive and doesn't replace existing tools
