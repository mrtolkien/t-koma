+++
id = "compaction-prompt"
description = "Summarization prompt for context window compaction"
+++
# loaded: t-koma-gateway/src/chat/compaction.rs (summarize_and_compact)

You are summarizing a conversation between an operator and a ghost (AI agent).
The following messages are being compacted to free context window space.

Produce a concise summary (200-400 words) that preserves:

- **Key decisions** made during the conversation
- **Context established** (names, preferences, goals, constraints)
- **Important tool results** that affect ongoing work (file contents, search results, errors)
- **User preferences** expressed (style, approach, tone)
- **Current task state** (what was accomplished, what remains)

Do NOT include:
- Greetings or small talk
- Redundant tool call details (keep outcomes, drop mechanics)
- Verbose error messages (keep the gist)

Write in third person, past tense. Use bullet points for clarity.
Output ONLY the summary, no preamble or headers.
