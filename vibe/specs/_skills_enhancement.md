# Skills System Enhancement Specification

## Overview

Enhance the skills system with three key improvements:
1. Discover skills from both project and config directories (XDG)
2. Create default skills embedded in the binary
3. Add integration test for skill usage

## Goals

### 1. Multi-Source Skill Discovery

Skills should be discovered from multiple locations:
- Project skills directory: `./default-prompts/skills/`
- User config directory: `~/.config/t-koma/skills/` (XDG)

The registry should merge skills from both locations, with user config skills taking precedence.

### 2. Default Skills (Embedded)

Default skills should be embedded in the binary using `include_str!`:
- Embedded at compile time from `skils/` directory
- On startup, check if they exist in config directory
- If not present, write them to config directory
- This allows users to modify skills while preserving defaults

The `skill-creator` skill is a good candidate for embedding.

### 3. Integration Test

Create an integration test that:
- Sets up a test skill
- Asks the model to use the skill
- Verifies the model requests the skill content
- Uses snapshot testing for validation

## Implementation

### Multi-Source Registry

```rust
pub struct SkillRegistry {
    skills: HashMap<String, Skill>,
    project_path: Option<PathBuf>,
    config_path: Option<PathBuf>,
}

impl SkillRegistry {
    /// Create registry from both project and config paths
    pub fn new_with_paths(
        project_path: Option<PathBuf>,
        config_path: Option<PathBuf>
    ) -> Result<Self, SkillError>;
    
    /// Load default skills from embedded resources
    pub fn write_default_skills(&self) -> Result<(), SkillError>;
}
```

### Default Skills Module

```rust
// t-koma-core/src/default_skills.rs
pub struct DefaultSkill {
    pub name: &'static str,
    pub content: &'static str,
}

pub const DEFAULT_SKILLS: &[DefaultSkill] = &[
    DefaultSkill {
        name: "skill-creator",
        content: include_str!("../../../default-prompts/skills/skill-creator/SKILL.md"),
    },
];
```

### Config Integration

Update `Config` to provide the skills config path:

```rust
impl Config {
    /// Get the skills directory path (XDG config)
    pub fn skills_config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("t-koma").join("skills"))
    }
}
```

## Steps

1. **Update SkillRegistry** for multi-source discovery
2. **Create default_skills module** with embedded skills
3. **Add Config helper** for skills path
4. **Write integration test** for skill usage
5. **Update documentation**

## Testing

- Unit tests for multi-source registry
- Test default skills embedding
- Integration test with snapshot
