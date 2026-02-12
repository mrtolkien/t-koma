-- Per-ghost model alias override (JSON array of alias strings, e.g. '["kimi25","gemma3"]').
-- NULL means "use the system default_model chain".
ALTER TABLE
  ghosts
ADD
  COLUMN model_aliases TEXT;
