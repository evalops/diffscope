-- DiffScope PostgreSQL schema
-- Reviews, comments, events, and conventions

CREATE TABLE IF NOT EXISTS reviews (
    id              TEXT PRIMARY KEY,
    status          TEXT NOT NULL CHECK (status IN ('Pending','Running','Complete','Failed')),
    diff_source     TEXT NOT NULL,
    started_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at    TIMESTAMPTZ,
    files_reviewed  INTEGER NOT NULL DEFAULT 0,
    error           TEXT,
    pr_summary_text TEXT,
    summary_json    JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_reviews_status ON reviews(status);
CREATE INDEX IF NOT EXISTS idx_reviews_started_at ON reviews(started_at DESC);
CREATE INDEX IF NOT EXISTS idx_reviews_created_at ON reviews(created_at DESC);

CREATE TABLE IF NOT EXISTS comments (
    id              TEXT PRIMARY KEY,
    review_id       TEXT NOT NULL REFERENCES reviews(id) ON DELETE CASCADE,
    file_path       TEXT NOT NULL,
    line_number     INTEGER NOT NULL,
    content         TEXT NOT NULL,
    rule_id         TEXT,
    severity        TEXT NOT NULL,
    category        TEXT NOT NULL,
    suggestion      TEXT,
    confidence      REAL NOT NULL,
    code_suggestion JSONB,
    tags            TEXT[] NOT NULL DEFAULT '{}',
    fix_effort      TEXT NOT NULL DEFAULT 'Low',
    feedback        TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_comments_review_id ON comments(review_id);
CREATE INDEX IF NOT EXISTS idx_comments_severity ON comments(severity);
CREATE INDEX IF NOT EXISTS idx_comments_category ON comments(category);

CREATE TABLE IF NOT EXISTS review_events (
    review_id           TEXT PRIMARY KEY REFERENCES reviews(id) ON DELETE CASCADE,
    event_type          TEXT NOT NULL,
    diff_source         TEXT NOT NULL,
    title               TEXT,
    model               TEXT NOT NULL,
    provider            TEXT,
    base_url            TEXT,
    duration_ms         BIGINT NOT NULL DEFAULT 0,
    diff_fetch_ms       BIGINT,
    llm_total_ms        BIGINT,
    diff_bytes          INTEGER NOT NULL DEFAULT 0,
    diff_files_total    INTEGER NOT NULL DEFAULT 0,
    diff_files_reviewed INTEGER NOT NULL DEFAULT 0,
    diff_files_skipped  INTEGER NOT NULL DEFAULT 0,
    comments_total      INTEGER NOT NULL DEFAULT 0,
    comments_by_severity JSONB NOT NULL DEFAULT '{}',
    comments_by_category JSONB NOT NULL DEFAULT '{}',
    overall_score       REAL,
    hotspots_detected   INTEGER NOT NULL DEFAULT 0,
    high_risk_files     INTEGER NOT NULL DEFAULT 0,
    tokens_prompt       INTEGER,
    tokens_completion   INTEGER,
    tokens_total        INTEGER,
    file_metrics        JSONB,
    hotspot_details     JSONB,
    convention_suppressed INTEGER,
    comments_by_pass    JSONB NOT NULL DEFAULT '{}',
    github_posted       BOOLEAN NOT NULL DEFAULT FALSE,
    github_repo         TEXT,
    github_pr           INTEGER,
    error               TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_events_model ON review_events(model);
CREATE INDEX IF NOT EXISTS idx_events_event_type ON review_events(event_type);
CREATE INDEX IF NOT EXISTS idx_events_created_at ON review_events(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_events_diff_source ON review_events(diff_source);
CREATE INDEX IF NOT EXISTS idx_events_github_repo ON review_events(github_repo) WHERE github_repo IS NOT NULL;

CREATE TABLE IF NOT EXISTS convention_patterns (
    id              SERIAL PRIMARY KEY,
    pattern_text    TEXT NOT NULL,
    category        TEXT NOT NULL,
    accepted_count  INTEGER NOT NULL DEFAULT 0,
    rejected_count  INTEGER NOT NULL DEFAULT 0,
    file_patterns   TEXT[] NOT NULL DEFAULT '{}',
    first_seen      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(pattern_text, category)
);
