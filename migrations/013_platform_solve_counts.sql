-- per-difficulty solve counts, so an atcoder profile shows the same three
-- charts a codeforces one does
--
-- stored as json because the bucket labels differ per platform: atcoder uses
-- its own 400-wide colour bands, codeforces uses 500-wide rating ranges

ALTER TABLE platform_profiles
    ADD COLUMN IF NOT EXISTS solve_counts JSONB;
