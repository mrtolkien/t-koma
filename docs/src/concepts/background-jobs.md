# Background Jobs

T-KOMA runs background jobs to maintain session health and curate knowledge. All
scheduling is centralized in `scheduler.rs`.

## Heartbeat (Session Health Check)

The heartbeat checks on idle sessions and lets the GHOST process pending context.

- **Trigger**: session idle for `idle_minutes` (default 4)
- **Skip guard**: skipped if a successful heartbeat already ran since last activity
- **Prompt**: uses `HEARTBEAT.md` in the GHOST workspace (auto-created on first use)
- **Output**: full transcript stored in `job_logs`, not in session messages
- **Continue mode**: if the GHOST responds with `HEARTBEAT_CONTINUE`, the heartbeat
  reschedules after `continue_minutes` (default 30) without posting to the session

Only meaningful heartbeat runs (status `"ran"`) post a summary into the session.

## Reflection (Knowledge Curation)

Reflection is an autonomous knowledge curation run that processes recent conversation
into persistent knowledge.

- **Trigger**: checked after each heartbeat tick; runs when new session messages exist
  since the last successful reflection and the session is idle
- **No cooldown**: one run per idle window, then waits for new messages
- **Input**: filtered transcript (text blocks preserved, tool-use summarized,
  tool-result stripped)
- **Continuity**: previous `handoff_note` from `job_logs` is injected; the final model
  response becomes the next `handoff_note`
- **Output**: full transcript, TODO list, and handoff note written to `job_logs`

### Reflection and Web Cache

During chat, web results are cached as plain files in `.web-cache/`. The reflection
system sees this file list and curates content into proper reference topics using
`reference_manage`. The cache is auto-cleared after successful reflection.

## CRON (File-Based Jobs)

CRON jobs are defined as markdown files in each GHOST workspace under `cron/`.

- **Schedule**: standard 5-field CRON expression (UTC)
- **Source of truth**: plaintext frontmatter + markdown prompt body
- **Watcher**: gateway watches `cron/` folders and refreshes the in-memory queue on file
  changes
- **Downtime behavior**: missed runs are skipped
- **Output continuity**: each job can carry previous output via files under
  `cron/.state/`

## Job Lifecycle

Background jobs use `SessionChat::chat_job()` instead of `chat()`, keeping their
transcripts out of the session messages table:

1. `JobLog::start()` — create in-progress log
2. `JobLogRepository::insert_started()` — persist at job start (TUI shows "in progress")
3. `JobLogRepository::update_todos()` — update TODO list mid-run
4. `JobLogRepository::finish()` — set status, transcript, and handoff note

## Key Files

- `t-koma-gateway/src/heartbeat.rs`
- `t-koma-gateway/src/reflection.rs`
- `t-koma-gateway/src/scheduler.rs`
- `t-koma-db/src/job_logs.rs`
