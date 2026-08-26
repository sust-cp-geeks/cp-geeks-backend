-- this file is for documentation / reference only
-- holds the address a user is switching to, until they prove they own it
-- by entering the code we mail there

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS pending_email VARCHAR(150);

-- step 2 of the change looks the account up by the address being claimed
CREATE INDEX IF NOT EXISTS idx_users_pending_email ON users(pending_email);
