-- Add compaction columns to sessions for persistent context summarization.
-- compaction_summary: LLM-generated summary of compacted messages.
-- compaction_cursor_id: messages.id of the last message included in the summary.
ALTER TABLE sessions ADD COLUMN compaction_summary TEXT;
ALTER TABLE sessions ADD COLUMN compaction_cursor_id TEXT;
