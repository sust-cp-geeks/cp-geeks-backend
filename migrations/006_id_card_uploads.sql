-- this file is for documentation / reference only
-- id card photos for students registering before they get a student email
-- the images live in cloudflare r2; these columns hold the object keys only

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS id_card_front_path VARCHAR(255),
    ADD COLUMN IF NOT EXISTS id_card_back_path  VARCHAR(255);

-- lets admins find applications still waiting on a decision
CREATE INDEX IF NOT EXISTS idx_users_status ON users(status);

-- note: the older id_card_path column from 001 was never read or written by
-- any handler and is superseded by the two columns above
