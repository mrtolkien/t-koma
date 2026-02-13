# T-KOMA Skills

This directory contains [Agent Skills](https://agentskills.io) that extend the
capabilities of the T-KOMA agent.

## What are Skills?

Skills are folders of instructions, scripts, and resources that agents can discover and
use to do things more accurately and efficiently. They provide:

- **Domain expertise**: Specialized knowledge for specific tasks
- **New capabilities**: Extended functionality through instructions and scripts
- **Repeatable workflows**: Consistent patterns for common operations
- **Progressive disclosure**: Metadata loaded at startup, full content on demand

## Skill Structure

Each skill is a directory containing at minimum a `SKILL.md` file:

```
skill-name/
├── SKILL.md          # Required - Instructions and metadata
├── scripts/          # Optional - Executable scripts
├── references/       # Optional - Additional documentation
└── assets/           # Optional - Static resources
```

## SKILL.md Format

```yaml
---
name: skill-name
description: A description of what this skill does and when to use it.
license: MIT
metadata:
  author: your-name
  version: "1.0"
---
# Skill Title

Detailed instructions for the agent...
```

### Required Fields

- `name`: Skill identifier (lowercase, alphanumeric, hyphens only, 1-64 chars)
- `description`: What the skill does and when to use it (1-1024 chars)

### Optional Fields

- `license`: License information
- `compatibility`: Environment requirements
- `metadata`: Additional key-value pairs

## Available Skills

- **skill-creator**: Guide for creating new skills
- **reference-researcher**: Advanced research/import strategies for reference topics
- **cron-job-author**: Create and improve file-based CRON jobs under `workspace/cron/`

## Using Skills

The agent automatically discovers skills in this directory at startup. To use a skill,
the agent:

1. Identifies relevant skills based on the task
2. Loads the full skill content when needed
3. Follows the instructions in SKILL.md
4. Uses scripts from the `scripts/` directory if available

## Creating New Skills

1. Create a new directory with your skill name
2. Add a `SKILL.md` file with proper frontmatter
3. Add optional `scripts/`, `references/`, and `assets/` as needed
4. Test the skill with the agent

See the `skill-creator` skill for detailed guidance.

## Resources

- [Agent Skills Specification](https://agentskills.io/specification)
- [Agent Skills Home](https://agentskills.io/home)
