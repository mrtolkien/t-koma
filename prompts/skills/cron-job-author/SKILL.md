---
name: cron-job-author
description:
  Create and improve file-based CRON jobs for a GHOST in T-KOMA. Use when you need to
  add or update `workspace/cron/*.md` jobs with schedule, pre-model tools, and a clear
  objective.
license: MIT
metadata:
  author: t-koma
  version: "1.0"
---

# CRON Job Author

Use this skill when the OPERATOR asks for scheduled automation.

## Goal

Design CRON jobs that are useful immediately and improve over time.

Every CRON prompt should:

- clearly restate the OPERATOR's intent and success criteria,
- use focused pre-model tool calls for deterministic input data,
- explicitly allow self-improvement when there is a clear, unambiguous improvement path.

## Where CRON jobs live

CRON jobs are markdown files in:

- `workspace/cron/*.md`

State from the previous run may be carried by the runtime automatically.

## File format

Use TOML frontmatter delimited by `+++`:

```markdown
+++
name = "Daily recap"
schedule = "0 8 * * *"
enabled = true
carry_last_output = true
pre_tools = [
  { name = "web_fetch", input = { url = "https://example.com/feed.xml", mode = "text", max_chars = 12000 } }
]
+++

Explain the OPERATOR intent clearly. State what "good output" means. If you detect a
clear and low-risk improvement, update this CRON file's prompt and/or pre_tools for next
runs.
```

`carry_last_output` controls whether the last successful output of this same CRON job is
injected into the next run as context (`true` = keep continuity, `false` = stateless).

## Scheduling rules

- Use standard 5-field CRON syntax.
- Schedule is interpreted in UTC.
- Missed runs are skipped when the system is down.

## Prompt-writing guidance

Include these elements in the CRON prompt body:

1. The OPERATOR intent in concrete terms.
2. What inputs are expected from pre-model tools.
3. Required output format and brevity level.
4. A constrained self-improvement instruction: "If you find a clear, unambiguous
   improvement, update this CRON job's prompt and/or pre_tools to improve future runs.
   Do not make speculative or broad changes."

## Safety and quality guardrails

- Prefer small edits over rewrites when self-improving.
- Keep scope tight to the CRON job objective.
- Avoid adding extra pre-tools unless they clearly improve signal quality.
- Keep the prompt deterministic and testable.

## Validate before finishing

Use the CLI validator before finalizing CRON file edits:

- `tkoma cron-validate`
- `tkoma cron-validate path/to/cron-or-file.md`
