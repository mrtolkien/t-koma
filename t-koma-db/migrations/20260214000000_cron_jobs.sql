-- Add per-ghost background model overrides and extend job_logs to support cron kind.
ALTER TABLE
  ghosts
ADD
  COLUMN heartbeat_model_aliases TEXT;
ALTER TABLE
  ghosts
ADD
  COLUMN reflection_model_aliases TEXT;
-- Rebuild job_logs to include 'cron' in job_kind constraint.
ALTER TABLE
  job_logs RENAME TO job_logs_old;
CREATE TABLE IF NOT EXISTS job_logs (
  id TEXT PRIMARY KEY,
  ghost_id TEXT NOT NULL,
  job_kind TEXT NOT NULL CHECK (job_kind IN ('heartbeat', 'reflection', 'cron')),
  session_id TEXT NOT NULL,
  started_at INTEGER NOT NULL,
  finished_at INTEGER,
  status TEXT,
  transcript TEXT NOT NULL DEFAULT '[]',
  todo_list TEXT,
  handoff_note TEXT,
  FOREIGN KEY (ghost_id) REFERENCES ghosts(id) ON DELETE CASCADE,
  FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);
INSERT INTO
  job_logs (
    id,
    ghost_id,
    job_kind,
    session_id,
    started_at,
    finished_at,
    status,
    transcript,
    todo_list,
    handoff_note
  )
SELECT
  id,
  ghost_id,
  job_kind,
  session_id,
  started_at,
  finished_at,
  status,
  transcript,
  todo_list,
  handoff_note
FROM
  job_logs_old;
DROP TABLE job_logs_old;
CREATE INDEX IF NOT EXISTS idx_job_logs_ghost_id ON job_logs(ghost_id);
CREATE INDEX IF NOT EXISTS idx_job_logs_session_id ON job_logs(session_id);
CREATE INDEX IF NOT EXISTS idx_job_logs_session_kind ON job_logs(session_id, job_kind, started_at DESC);
