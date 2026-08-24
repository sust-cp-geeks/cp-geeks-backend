-- this file is for documentation / reference only
-- backfilled: these tables came in with the events backend but never got a file

-- events table
CREATE TABLE IF NOT EXISTS events (
    event_id           SERIAL PRIMARY KEY,
    description        TEXT NOT NULL,
    event_date         TIMESTAMP NOT NULL,
    created_at         TIMESTAMP DEFAULT NOW(),
    vjudge_contest_ids BIGINT[]
);

-- teams table — deleting an event removes its teams
CREATE TABLE IF NOT EXISTS teams (
    team_id    SERIAL PRIMARY KEY,
    event_id   INTEGER REFERENCES events(event_id) ON DELETE CASCADE,
    coach_name VARCHAR(100)
);

-- team members table — stored by reg_number, joined to users when we need a name
CREATE TABLE IF NOT EXISTS team_members (
    member_id  SERIAL PRIMARY KEY,
    team_id    INTEGER REFERENCES teams(team_id) ON DELETE CASCADE,
    reg_number VARCHAR(50) NOT NULL
);

-- indexes for the batch team lookups
CREATE INDEX IF NOT EXISTS idx_teams_event_id ON teams(event_id);
CREATE INDEX IF NOT EXISTS idx_team_members_team_id ON team_members(team_id);
CREATE INDEX IF NOT EXISTS idx_team_members_reg_number ON team_members(reg_number);
