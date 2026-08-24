-- this file is for documentation / reference only
-- backfilled: these tables came in with the problemset section but never got a file

-- problem sections table
CREATE TABLE IF NOT EXISTS problem_sections (
    id          SERIAL PRIMARY KEY,
    name        VARCHAR(255) NOT NULL,
    description TEXT,
    created_at  TIMESTAMP DEFAULT NOW()
);

-- subsections table — deleting a section removes its subsections
CREATE TABLE IF NOT EXISTS problem_subsections (
    id          SERIAL PRIMARY KEY,
    section_id  INTEGER REFERENCES problem_sections(id) ON DELETE CASCADE,
    name        VARCHAR(255) NOT NULL,
    description TEXT,
    created_at  TIMESTAMP DEFAULT NOW()
);

-- problem items table — deleting a subsection removes its items
CREATE TABLE IF NOT EXISTS problem_items (
    id             SERIAL PRIMARY KEY,
    subsection_id  INTEGER REFERENCES problem_subsections(id) ON DELETE CASCADE,
    item_type      VARCHAR(50) NOT NULL,
    title          VARCHAR(255) NOT NULL,
    url            VARCHAR(500) NOT NULL,
    platform       VARCHAR(100),
    created_at     TIMESTAMP DEFAULT NOW()
);

-- indexes for the grouping queries in get_problems
CREATE INDEX IF NOT EXISTS idx_problem_subsections_section_id ON problem_subsections(section_id);
CREATE INDEX IF NOT EXISTS idx_problem_items_subsection_id ON problem_items(subsection_id);
