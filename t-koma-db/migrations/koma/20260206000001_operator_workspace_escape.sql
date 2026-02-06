ALTER TABLE operators
    ADD COLUMN allow_workspace_escape INTEGER DEFAULT 0
    CHECK (allow_workspace_escape IN (0, 1));
