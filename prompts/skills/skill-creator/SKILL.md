---
name: skill-creator
description: Guide for creating effective Agent Skills for t-koma. Use when the user wants to create a new skill or update an existing skill.
license: MIT
metadata:
  author: t-koma
  version: "1.0"
---

# Skill Creator Guide

This guide helps you create effective Agent Skills that extend t-koma's capabilities.

## What is a Skill?

A skill is a self-contained directory with instructions, scripts, and resources that help the agent perform specific tasks more accurately and efficiently.

## When to Create a Skill

Create a skill when:

- You have domain-specific knowledge to codify
- You want to provide reusable workflows
- You need to package scripts for common operations
- You want to capture organizational knowledge

## Skill Structure

```
prompts/skills/my-skill/
├── SKILL.md          # Required: Instructions and metadata
├── scripts/          # Optional: Executable code
├── references/       # Optional: Additional docs
└── assets/           # Optional: Static resources
```

## Creating a SKILL.md

The `SKILL.md` file has two parts:

### 1. YAML Frontmatter (Required)

```yaml
---
name: my-skill-name
description: Clear description of what this skill does and when to use it.
license: MIT
compatibility: Requires git, docker, and internet access
metadata:
  author: your-name
  version: "1.0"
---
```

**Naming Rules:**

- 1-64 characters
- Lowercase letters, numbers, hyphens only
- No starting/ending hyphens
- No consecutive hyphens

**Description Tips:**

- Explain WHAT the skill does
- Explain WHEN to use it
- Include keywords for discovery
- 1-1024 characters

### 2. Markdown Body

Write clear, step-by-step instructions. Recommended sections:

```markdown
# Skill Title

## Overview

Brief explanation of the skill's purpose.

## Steps

1. Step one with clear instructions
2. Step two with examples
3. Step three with expected outputs

## Examples

### Example 1: Common Use Case

Input: ...
Output: ...

### Example 2: Edge Case

Input: ...
Output: ...

## Common Pitfalls

- Don't do X because...
- Always check Y before...

## References

- [Reference file](references/REFERENCE.md)
- External documentation
```

## Best Practices

### Progressive Disclosure

Structure for efficient context usage:

1. **Metadata** (~100 tokens): Loaded at startup for all skills
2. **Instructions** (<5000 tokens): Loaded when skill is activated
3. **Resources** (as needed): Loaded only when required

Keep `SKILL.md` under 500 lines. Move detailed content to `references/`.

### Writing Instructions

- Use clear, actionable language
- Provide concrete examples
- Include expected inputs and outputs
- Document error cases
- Use code blocks for commands/scripts

### Scripts Directory

Place executable code in `scripts/`:

- Keep scripts self-contained
- Document dependencies
- Handle errors gracefully
- Use descriptive names (e.g., `extract_data.py`, `deploy.sh`)

### References Directory

Place detailed docs in `references/`:

- `REFERENCE.md` - Technical reference
- `API.md` - API documentation
- Domain-specific files

Keep individual files focused. Smaller files = less context usage.

### Assets Directory

Place static resources in `assets/`:

- Templates
- Configuration files
- Lookup tables

## File References

Reference other files using relative paths:

````markdown
See [the reference guide](references/REFERENCE.md) for details.

Run the extraction script:

```bash
./scripts/extract.py
```
````

```

## Validation

Before using a skill:

1. Verify frontmatter is valid YAML
2. Ensure name follows conventions
3. Check description is descriptive
4. Test any scripts
5. Verify file references work

## Example: Complete Skill

```

prompts/skills/data-extraction/
├── SKILL.md
├── scripts/
│ ├── extract_csv.py
│ └── clean_data.py
└── references/
└── DATA_FORMATS.md

````

**SKILL.md:**

```yaml
---
name: data-extraction
description: Extract and clean data from CSV, JSON, and XML files. Use when processing data files or transforming data formats.
metadata:
  author: data-team
  version: "1.0"
---

# Data Extraction Skill

Extract and clean data from various file formats.

## Supported Formats

- CSV (with various delimiters)
- JSON (flat and nested)
- XML (with XPath support)

## Usage

1. Identify the source file format
2. Choose appropriate extraction script
3. Run with input file path
4. Review cleaned output

## Scripts

- `scripts/extract_csv.py` - Extract from CSV files
- `scripts/clean_data.py` - Clean and normalize data

## Examples

### Extract from CSV

```bash
./scripts/extract_csv.py data/input.csv --output output.json
````

See [references/DATA_FORMATS.md](references/DATA_FORMATS.md) for format details.

```

## Tips for Success

1. **Start Simple**: Create a basic skill first, then expand
2. **Test Iteratively**: Verify the skill works with the agent
3. **Document Assumptions**: Explain what the agent should know
4. **Be Specific**: Give concrete examples, not vague guidance
5. **Handle Errors**: Document what to do when things go wrong
6. **Keep Updated**: Refresh skills as workflows evolve

## Resources

- [Agent Skills Specification](https://agentskills.io/specification)
- [Agent Skills Home](https://agentskills.io/home)
```
