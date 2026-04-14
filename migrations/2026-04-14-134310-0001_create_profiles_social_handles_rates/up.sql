CREATE EXTENSION IF NOT EXISTS citext;

CREATE TABLE profiles (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT        NOT NULL,
    bio        TEXT        NOT NULL,
    niche      TEXT        NOT NULL,
    avatar_url TEXT        NOT NULL,
    username   CITEXT      NOT NULL UNIQUE,
    updated_at TIMESTAMPTZ DEFAULT now(),
    created_at TIMESTAMPTZ DEFAULT now()
);

CREATE TABLE social_handles (
    id             UUID    PRIMARY KEY DEFAULT gen_random_uuid(),
    profile_id     UUID    NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    platform       TEXT    NOT NULL CHECK (platform IN ('instagram', 'tiktok', 'youtube')),
    handle         TEXT    NOT NULL,
    url            TEXT    NOT NULL,
    follower_count INT     NOT NULL CHECK (follower_count >= 0),
    updated_at     TIMESTAMPTZ DEFAULT now(),
    UNIQUE (profile_id, platform)
);

CREATE TABLE rates (
    id         UUID           PRIMARY KEY DEFAULT gen_random_uuid(),
    profile_id UUID           NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    type       TEXT           NOT NULL CHECK (type IN ('post', 'story', 'reel')),
    amount     NUMERIC(10, 2) NOT NULL CHECK (amount >= 0),
    UNIQUE (profile_id, type)
);
