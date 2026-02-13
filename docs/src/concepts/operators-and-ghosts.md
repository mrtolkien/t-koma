# OPERATORS and GHOSTS

T-KOMA uses a two-level identity model: **OPERATORS** (humans) and **GHOSTS** (AI
agents).

## OPERATORS

An OPERATOR is an approved end user. Each OPERATOR:

- Has a unique ID and display name
- Must be approved before interacting (`OperatorStatus`: pending → approved)
- Can own one or more GHOSTS
- Connects through **interfaces** (Discord, TUI)

### Interfaces

An interface is a messaging endpoint that ties an OPERATOR to a platform. It's
identified by `(platform, external_id)` — for example, a Discord user ID or a WebSocket
client ID.

When a new interface connects, the OPERATOR flow prompts whether this is a new or
existing OPERATOR, then runs through the approval pipeline. Existing-OPERATOR linking is
not fully implemented yet.

## GHOSTS

A GHOST is a personal AI agent with its own:

- **Data partition**: GHOST-scoped sessions/messages/usage/job logs in the unified DB
- **Workspace**: filesystem directory for tools and knowledge
- **Knowledge base**: notes, references, diary entries (with embeddings)
- **System prompt**: personality and behavioral guidance

Each GHOST is owned by an OPERATOR (`owner_operator_id`). The owner interacts with the
GHOST through sessions.

### Per-GHOST Model Override

GHOSTS can optionally be assigned a specific model or model chain, overriding the global
`default_model`. This is changeable via Discord `/model` command.

## Sessions

A session is a chat thread between an OPERATOR and a GHOST. Sessions track:

- Message history (persisted in unified DB, scoped by `ghost_id`)
- Compaction state (summary + cursor for long conversations)
- Background job logs (heartbeat, reflection transcripts)

Sessions support **compaction**: when the conversation grows long, older messages are
summarized into a compaction summary. Original messages are never deleted — only the
window of messages sent to the provider shifts forward.
