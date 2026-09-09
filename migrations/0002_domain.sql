CREATE TABLE folders (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    parent_folder_id TEXT REFERENCES folders(id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX folders_user_id_idx
    ON folders(user_id);


CREATE TABLE decks (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    folder_id TEXT REFERENCES folders(id) ON DELETE SET NULL,
    parent_deck_id TEXT REFERENCES decks(id) ON DELETE SET NULL,

    name TEXT NOT NULL,
    description TEXT,
    support_text TEXT,
    about TEXT,

    target_cards INTEGER NOT NULL DEFAULT 20,
    card_type TEXT NOT NULL DEFAULT 'basic',
    difficulty TEXT NOT NULL DEFAULT 'medio',
    color TEXT NOT NULL DEFAULT '#8b5cf6',
    icon TEXT,

    show_review_queue INTEGER NOT NULL DEFAULT 1,
    allow_subdecks INTEGER NOT NULL DEFAULT 1,
    is_public INTEGER NOT NULL DEFAULT 0,

    anki_name TEXT,
    imported_from_anki INTEGER NOT NULL DEFAULT 0,
    review_cards_per_day INTEGER,

    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX decks_user_id_idx
    ON decks(user_id);

CREATE INDEX decks_folder_id_idx
    ON decks(folder_id);


CREATE TABLE note_types (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,

    fields TEXT NOT NULL DEFAULT '[]',
    templates TEXT NOT NULL DEFAULT '[]',

    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX note_types_user_id_idx
    ON note_types(user_id);


CREATE TABLE notes (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    deck_id TEXT NOT NULL REFERENCES decks(id) ON DELETE CASCADE,
    note_type_id TEXT NOT NULL REFERENCES note_types(id) ON DELETE RESTRICT,

    fields TEXT NOT NULL DEFAULT '{}',
    tags TEXT NOT NULL DEFAULT '[]',

    inserted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX notes_deck_id_idx
    ON notes(deck_id);

CREATE INDEX notes_user_id_idx
    ON notes(user_id);


CREATE TABLE cards (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    deck_id TEXT NOT NULL REFERENCES decks(id) ON DELETE CASCADE,

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

CREATE INDEX cards_deck_id_idx
    ON cards(deck_id);

CREATE INDEX cards_due_idx
    ON cards(user_id, queue, due);


CREATE TABLE review_logs (
    id TEXT PRIMARY KEY,
    card_id TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    rating TEXT NOT NULL,

    interval_before REAL NOT NULL,
    interval_after REAL NOT NULL,

    ease_before REAL NOT NULL,
    ease_after REAL NOT NULL,

    reviewed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX review_logs_user_id_idx
    ON review_logs(user_id, reviewed_at);