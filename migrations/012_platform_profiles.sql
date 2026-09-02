-- competitive programming profiles from platforms other than codeforces
--
-- these are synced in the background rather than fetched per request: atcoder
-- has no bulk endpoint and asks for a second between calls, so ~350 members
-- takes minutes. the api serves these rows; nothing hits atcoder on the
-- request path.
--
-- keyed by platform so codechef can be added without another migration.

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS atcoder_handle VARCHAR(100);

CREATE TABLE IF NOT EXISTS platform_profiles (
    user_id      INTEGER NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    platform     VARCHAR(20) NOT NULL,
    handle       VARCHAR(100) NOT NULL,
    rating       INTEGER,
    max_rating   INTEGER,
    rank_title   VARCHAR(40),
    solved_count INTEGER,
    synced_at    TIMESTAMP,
    -- why the last sync failed for this member; NULL when it succeeded, so a
    -- bad handle is visible instead of silently missing from the leaderboard
    sync_error   TEXT,
    PRIMARY KEY (user_id, platform)
);

-- one row per contest the member actually entered
CREATE TABLE IF NOT EXISTS platform_rating_history (
    user_id      INTEGER NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    platform     VARCHAR(20) NOT NULL,
    contest_id   VARCHAR(120) NOT NULL,
    contest_name VARCHAR(255) NOT NULL,
    old_rating   INTEGER,
    new_rating   INTEGER,
    place        INTEGER,
    performance  INTEGER,
    is_rated     BOOLEAN NOT NULL DEFAULT true,
    ended_at     TIMESTAMP,
    PRIMARY KEY (user_id, platform, contest_id)
);

-- the leaderboard reads one platform ordered by rating
CREATE INDEX IF NOT EXISTS idx_platform_profiles_board
    ON platform_profiles(platform, rating DESC);

-- a profile reads one member's history newest first
CREATE INDEX IF NOT EXISTS idx_platform_history_user
    ON platform_rating_history(user_id, platform, ended_at DESC);
