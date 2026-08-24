-- this file is for documentation / reference only
-- backfilled: is_manager is used by the jwt claims but was never added to 001

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS is_manager BOOLEAN DEFAULT false;
