-- this file is for documentation / reference only
-- pinning, an arbitrary outbound link, and optional ties to an event or contest

ALTER TABLE announcements
    ADD COLUMN IF NOT EXISTS is_pinned  BOOLEAN NOT NULL DEFAULT false,
    -- an arbitrary link: registration form, vjudge contest, facebook post
    ADD COLUMN IF NOT EXISTS link_url   VARCHAR(500),
    -- what the button should say; falls back to the url if absent
    ADD COLUMN IF NOT EXISTS link_label VARCHAR(100),
    -- optional ties to something already in the system
    ADD COLUMN IF NOT EXISTS event_id   INTEGER,
    ADD COLUMN IF NOT EXISTS contest_no INTEGER;

-- deleting an event or contest empties the tie, it does not delete the post
ALTER TABLE announcements DROP CONSTRAINT IF EXISTS announcements_event_id_fkey;
ALTER TABLE announcements
    ADD CONSTRAINT announcements_event_id_fkey
    FOREIGN KEY (event_id) REFERENCES events(event_id) ON DELETE SET NULL;

ALTER TABLE announcements DROP CONSTRAINT IF EXISTS announcements_contest_no_fkey;
ALTER TABLE announcements
    ADD CONSTRAINT announcements_contest_no_fkey
    FOREIGN KEY (contest_no) REFERENCES contests(contest_no) ON DELETE SET NULL;

-- the default feed: pinned first, then newest
CREATE INDEX IF NOT EXISTS idx_announcements_feed
    ON announcements(is_pinned DESC, created_at DESC);

-- ?upcoming=true scans forward on event_date
CREATE INDEX IF NOT EXISTS idx_announcements_event_date
    ON announcements(event_date);
