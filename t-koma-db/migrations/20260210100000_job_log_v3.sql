-- Knowledge v3: add todo_list and handoff_note to job_logs
-- Conditional: only add columns if they don't already exist (safe for fresh DBs
-- where the unified schema migration already includes them).
ALTER TABLE job_logs ADD COLUMN todo_list TEXT;
ALTER TABLE job_logs ADD COLUMN handoff_note TEXT;
