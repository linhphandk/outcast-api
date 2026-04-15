CREATE TABLE sessions (
    id            UUID         PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id       UUID         NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    refresh_token VARCHAR(512) UNIQUE NOT NULL,
    user_agent    TEXT,
    ip_address    VARCHAR(45),
    expires_at    TIMESTAMP    NOT NULL,
    revoked_at    TIMESTAMP,
    created_at    TIMESTAMP    NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMP    NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_sessions_user_id ON sessions(user_id);
CREATE INDEX idx_sessions_refresh_token ON sessions(refresh_token);
