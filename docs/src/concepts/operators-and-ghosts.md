# Operators and Ghosts

t-koma uses a two-level identity model: **operators** (humans) and **ghosts** (AI
agents).

## Operators

An operator is an approved end user. Each operator:

- Has a unique ID and display name
- Must be approved before interacting (`OperatorStatus`: pending → approved)
- Can own one or more ghosts
- Connects through **interfaces** (Discord, TUI)

### Interfaces

An interface is a messaging endpoint that ties an operator to a platform. It's
identified by `(platform, external_id)` — for example, a Discord user ID or a WebSocket
client ID.

When a new interface connects, the operator flow prompts whether this is a new or
existing operator, then runs through the approval pipeline. Existing-operator linking is
not fully implemented yet.

## Ghosts

A ghost is a personal AI agent with its own:

- **Data partition**: ghost-scoped sessions/messages/usage/job logs in the unified DB
- **Workspace**: filesystem directory for tools and knowledge
- **Knowledge base**: notes, references, diary entries (with embeddings)
- **System prompt**: personality and behavioral guidance

Each ghost is owned by an operator (`owner_operator_id`). The owner interacts with the
ghost through sessions.

### Per-Ghost Model Override

Ghosts can optionally be assigned a specific model or model chain, overriding the global
`default_model`. This is changeable via Discord `/model` command.

## Sessions

A session is a chat thread between an operator and a ghost. Sessions track:

- Message history (persisted in unified DB, scoped by `ghost_id`)
- Compaction state (summary + cursor for long conversations)
- Background job logs (heartbeat, reflection transcripts)

Sessions support **compaction**: when the conversation grows long, older messages are
summarized into a compaction summary. Original messages are never deleted — only the
window of messages sent to the provider shifts forward.
