-- this file is for documentation / reference only
-- tracks edits, tightens created_at, and stops an author's posts from
-- blocking their account deletion

-- null until the post is edited, so "never edited" stays distinguishable
ALTER TABLE announcements
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMP;

-- created_at drives the ordering, and postgres sorts nulls first on DESC,
-- so a null would silently pin a post to the top of the feed
UPDATE announcements SET created_at = NOW() WHERE created_at IS NULL;
ALTER TABLE announcements
    ALTER COLUMN created_at SET DEFAULT NOW(),
    ALTER COLUMN created_at SET NOT NULL;

-- deleting a member used to fail while any of their posts survived;
-- the post stays, the byline just becomes unknown
ALTER TABLE announcements
    DROP CONSTRAINT IF EXISTS announcements_author_id_fkey;
ALTER TABLE announcements
    ADD CONSTRAINT announcements_author_id_fkey
    FOREIGN KEY (author_id) REFERENCES users(user_id) ON DELETE SET NULL;

-- the feed is always ordered newest first
CREATE INDEX IF NOT EXISTS idx_announcements_created_at ON announcements(created_at DESC);
