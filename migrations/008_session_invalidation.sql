-- this file is for documentation / reference only
-- tokens issued before this moment are refused, so a password reset or a ban
-- takes effect immediately instead of waiting out the 7-day jwt expiry
-- NULL means nothing has ever invalidated this user's sessions

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS sessions_valid_from TIMESTAMP;
