ALTER TABLE social_handles
    ADD COLUMN engagement_rate NUMERIC(5,4) NOT NULL DEFAULT 0,
    ADD COLUMN last_synced_at  TIMESTAMPTZ;
