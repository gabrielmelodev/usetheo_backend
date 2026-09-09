-- ============================================================
-- 0009_timer.sql
-- Theo Local - SQLite
-- ============================================================

CREATE TABLE IF NOT EXISTS study_timers (
    user_id BLOB PRIMARY KEY
        REFERENCES users(id) ON DELETE CASCADE,

    exam_id BLOB NOT NULL
        REFERENCES exams(id) ON DELETE CASCADE,

    topic_id BLOB NOT NULL
        REFERENCES exam_topics(id) ON DELETE CASCADE,

    session_id BLOB NOT NULL,

    elapsed_seconds INTEGER NOT NULL DEFAULT 0,

    running INTEGER NOT NULL DEFAULT 0,

    started_at TEXT,

    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,

    category TEXT NOT NULL DEFAULT 'teoria',

    running_since TEXT
);

CREATE INDEX IF NOT EXISTS study_timers_exam_id_idx
    ON study_timers(exam_id);

CREATE INDEX IF NOT EXISTS study_timers_topic_id_idx
    ON study_timers(topic_id);

CREATE INDEX IF NOT EXISTS study_timers_session_id_idx
    ON study_timers(session_id);