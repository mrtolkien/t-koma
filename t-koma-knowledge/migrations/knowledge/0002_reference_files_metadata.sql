ALTER TABLE reference_files ADD COLUMN source_url TEXT;
ALTER TABLE reference_files ADD COLUMN source_type TEXT NOT NULL DEFAULT 'git';
ALTER TABLE reference_files ADD COLUMN fetched_at TEXT;
ALTER TABLE reference_files ADD COLUMN max_age_days INTEGER NOT NULL DEFAULT 0;
