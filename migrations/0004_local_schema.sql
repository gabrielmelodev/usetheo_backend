-- Theo: banco LOCAL.
-- PostgreSQL remoto fica reservado a auth/cadastro/sessões e sincronização.
-- UUIDs são armazenados pelo SQLx/SQLite como BLOB; datas como TEXT.

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS decks (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    user_id BLOB NOT NULL,
    folder_id BLOB,
    parent_deck_id BLOB,
    name TEXT NOT NULL,
    description TEXT,
    support_text TEXT,
    about TEXT,
    target_cards INTEGER NOT NULL DEFAULT 0,
    card_type TEXT NOT NULL DEFAULT 'basic',
    difficulty TEXT NOT NULL DEFAULT 'medio',
    color TEXT NOT NULL DEFAULT '#556B2F',
    icon TEXT,
    show_review_queue INTEGER NOT NULL DEFAULT 1,
    allow_subdecks INTEGER NOT NULL DEFAULT 1,
    is_public INTEGER NOT NULL DEFAULT 0,
    is_official INTEGER NOT NULL DEFAULT 0,
    anki_name TEXT,
    imported_from_anki INTEGER NOT NULL DEFAULT 0,
    review_cards_per_day INTEGER,
    is_template INTEGER NOT NULL DEFAULT 0,
    position INTEGER NOT NULL DEFAULT 0,
    copied_from_deck_id BLOB,
    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_local_decks_user ON decks(user_id);
CREATE INDEX IF NOT EXISTS idx_local_decks_folder ON decks(folder_id);

CREATE TABLE IF NOT EXISTS folders (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    user_id BLOB NOT NULL,
    parent_folder_id BLOB,
    name TEXT NOT NULL,
    position INTEGER NOT NULL DEFAULT 0,
    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_local_folders_user ON folders(user_id);

CREATE TABLE IF NOT EXISTS note_types (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    user_id BLOB NOT NULL,
    name TEXT NOT NULL,
    fields TEXT NOT NULL DEFAULT '[]',
    templates TEXT NOT NULL DEFAULT '[]',
    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_local_note_types_user ON note_types(user_id);

CREATE TABLE IF NOT EXISTS notes (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    user_id BLOB NOT NULL,
    deck_id BLOB NOT NULL,
    note_type_id BLOB NOT NULL,
    fields TEXT NOT NULL DEFAULT '{}',
    tags TEXT NOT NULL DEFAULT '[]',
    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_local_notes_user ON notes(user_id);
CREATE INDEX IF NOT EXISTS idx_local_notes_deck ON notes(deck_id);

CREATE TABLE IF NOT EXISTS cards (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    user_id BLOB NOT NULL,
    note_id BLOB NOT NULL,
    deck_id BLOB NOT NULL,
    template_index INTEGER NOT NULL DEFAULT 0,
    queue TEXT NOT NULL DEFAULT 'new',
    due TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    interval_days REAL NOT NULL DEFAULT 0,
    ease_factor REAL NOT NULL DEFAULT 2.5,
    repetitions INTEGER NOT NULL DEFAULT 0,
    lapses INTEGER NOT NULL DEFAULT 0,
    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_local_cards_user ON cards(user_id);
CREATE INDEX IF NOT EXISTS idx_local_cards_deck ON cards(deck_id);
CREATE INDEX IF NOT EXISTS idx_local_cards_due ON cards(user_id, due);

CREATE TABLE IF NOT EXISTS exams (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    user_id BLOB NOT NULL,
    name TEXT NOT NULL,
    plan_mode TEXT NOT NULL DEFAULT 'free',
    exam_date TEXT,
    sprint_days INTEGER,
    weekly_hours REAL,
    continuous_mode TEXT,
    max_session_minutes INTEGER,
    review_intervals_days TEXT,
    excluded_weekdays TEXT NOT NULL DEFAULT '[]',
    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_local_exams_user ON exams(user_id);

CREATE TABLE IF NOT EXISTS exam_subjects (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    exam_id BLOB NOT NULL,
    name TEXT NOT NULL,
    weight INTEGER NOT NULL DEFAULT 3,
    knowledge_factor REAL NOT NULL DEFAULT 1.0,
    position INTEGER NOT NULL DEFAULT 0,
    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_local_exam_subjects_exam ON exam_subjects(exam_id);

CREATE TABLE IF NOT EXISTS exam_topics (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    exam_id BLOB NOT NULL,
    subject_id BLOB NOT NULL,
    parent_topic_id BLOB,
    number TEXT NOT NULL,
    title TEXT NOT NULL,
    position INTEGER NOT NULL DEFAULT 0,
    relevance INTEGER NOT NULL DEFAULT 1,
    in_sprint INTEGER NOT NULL DEFAULT 0,
    scheduled_date TEXT,
    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_local_exam_topics_exam ON exam_topics(exam_id);
CREATE INDEX IF NOT EXISTS idx_local_exam_topics_subject ON exam_topics(subject_id);

CREATE TABLE IF NOT EXISTS study_logs (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    user_id BLOB NOT NULL,
    exam_id BLOB NOT NULL,
    topic_id BLOB NOT NULL,
    category TEXT NOT NULL,
    studied_on TEXT NOT NULL,
    minutes INTEGER NOT NULL,
    pages INTEGER,
    video_minutes INTEGER,
    questions_total INTEGER,
    questions_correct INTEGER,
    notes TEXT,
    logged_same_day INTEGER NOT NULL DEFAULT 0,
    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_local_study_logs_user ON study_logs(user_id);
CREATE INDEX IF NOT EXISTS idx_local_study_logs_topic ON study_logs(topic_id);
CREATE INDEX IF NOT EXISTS idx_local_study_logs_date ON study_logs(studied_on);

CREATE TABLE IF NOT EXISTS topic_reviews (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    user_id BLOB NOT NULL,
    topic_id BLOB NOT NULL,
    study_log_id BLOB,
    step INTEGER NOT NULL DEFAULT 1,
    scheduled_date TEXT NOT NULL,
    completed_at TEXT,
    completed_study_log_id BLOB,
    skipped INTEGER NOT NULL DEFAULT 0,
    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_local_topic_reviews_user ON topic_reviews(user_id);
CREATE INDEX IF NOT EXISTS idx_local_topic_reviews_due ON topic_reviews(user_id, scheduled_date);

CREATE TABLE IF NOT EXISTS manual_schedule_entries (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    user_id BLOB NOT NULL,
    exam_id BLOB NOT NULL,
    weekday INTEGER NOT NULL,
    subject_id BLOB,
    topic_id BLOB,
    planned_minutes INTEGER,
    question_goal INTEGER,
    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_local_manual_schedule_exam ON manual_schedule_entries(exam_id);

CREATE TABLE IF NOT EXISTS goals (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    user_id BLOB NOT NULL,
    period TEXT NOT NULL,
    metric TEXT NOT NULL,
    target REAL NOT NULL,
    exam_id BLOB,
    active INTEGER NOT NULL DEFAULT 1,
    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_local_goals_user ON goals(user_id);

CREATE TABLE IF NOT EXISTS active_timers (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    user_id BLOB NOT NULL,
    exam_id BLOB NOT NULL,
    topic_id BLOB NOT NULL,
    category TEXT NOT NULL,
    started_at TEXT NOT NULL,
    accumulated_seconds INTEGER NOT NULL DEFAULT 0,
    running_since TEXT,
    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_local_active_timer_user ON active_timers(user_id);

CREATE TABLE IF NOT EXISTS questions (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    user_id BLOB NOT NULL,
    exam_id BLOB,
    subject_id BLOB,
    topic_id BLOB,
    banca TEXT,
    source TEXT,
    year INTEGER,
    question_type TEXT NOT NULL,
    statement TEXT NOT NULL,
    alternatives TEXT NOT NULL DEFAULT '[]',
    correct_alternative_id TEXT,
    explanation TEXT,
    difficulty TEXT NOT NULL DEFAULT 'medio',
    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_local_questions_user ON questions(user_id);
CREATE INDEX IF NOT EXISTS idx_local_questions_exam ON questions(exam_id);

CREATE TABLE IF NOT EXISTS question_attempts (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    user_id BLOB NOT NULL,
    question_id BLOB NOT NULL,
    selected_alternative_id TEXT,
    is_correct INTEGER NOT NULL DEFAULT 0,
    answered_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_local_attempts_user ON question_attempts(user_id);
CREATE INDEX IF NOT EXISTS idx_local_attempts_question ON question_attempts(question_id);

CREATE TABLE IF NOT EXISTS review_logs (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    card_id BLOB NOT NULL,
    user_id BLOB NOT NULL,
    rating TEXT NOT NULL,
    interval_before REAL NOT NULL,
    interval_after REAL NOT NULL,
    ease_before REAL NOT NULL,
    ease_after REAL NOT NULL,
    reviewed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_local_review_logs_user ON review_logs(user_id);
CREATE INDEX IF NOT EXISTS idx_local_review_logs_reviewed_at ON review_logs(reviewed_at);

CREATE TABLE IF NOT EXISTS media_attachments (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    user_id BLOB NOT NULL,
    deck_id BLOB NOT NULL,
    note_id BLOB,
    kind TEXT NOT NULL,
    filename TEXT NOT NULL,
    content_type TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    storage_path TEXT NOT NULL,
    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_local_media_user ON media_attachments(user_id);
CREATE INDEX IF NOT EXISTS idx_local_media_deck ON media_attachments(deck_id);

CREATE TABLE IF NOT EXISTS audit_logs (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),
    user_id BLOB,
    action TEXT NOT NULL,
    ip_address TEXT,
    user_agent TEXT,
    metadata TEXT NOT NULL DEFAULT '{}',
    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
