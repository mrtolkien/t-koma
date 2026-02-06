ALTER TABLE operators
    ADD COLUMN access_level TEXT NOT NULL DEFAULT 'standard'
    CHECK (access_level IN ('puppet_master', 'standard'));

ALTER TABLE operators
    ADD COLUMN rate_limit_5m_max INTEGER DEFAULT 10;

ALTER TABLE operators
    ADD COLUMN rate_limit_1h_max INTEGER DEFAULT 100;
