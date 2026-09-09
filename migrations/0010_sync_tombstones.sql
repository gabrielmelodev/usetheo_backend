-- ============================================================
-- Theo Local
-- 0010_sync_tombstones.sql
-- SQLite
--
-- Registra exclusões para sincronização entre dispositivos.
-- UUIDs são armazenados como BLOB no SQLite.
-- Datas são armazenadas como TEXT.
-- ============================================================

CREATE TABLE IF NOT EXISTS sync_tombstones (
    id BLOB PRIMARY KEY DEFAULT (randomblob(16)),

    user_id BLOB NOT NULL
        REFERENCES users(id) ON DELETE CASCADE,

    entity_type TEXT NOT NULL,

    entity_id BLOB NOT NULL,

    deleted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_sync_tombstones_user_deleted_at
    ON sync_tombstones (user_id, deleted_at);

CREATE UNIQUE INDEX IF NOT EXISTS idx_sync_tombstones_unique
    ON sync_tombstones (user_id, entity_type, entity_id);