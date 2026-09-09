-- ATENÇÃO: renomeie este arquivo para o próximo número da sua sequência real de
-- migrations (ex.: 0007_sync_tombstones.sql) — o zip enviado não trouxe a pasta
-- migrations/ existente, então não sei qual é o próximo número.

-- Registra exclusões para que outros dispositivos, ao sincronizar, saibam o que
-- foi apagado (um DELETE puro não deixa rastro para propagar via /sync/changes).
CREATE TABLE IF NOT EXISTS sync_tombstones (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    entity_type TEXT NOT NULL, -- 'deck' | 'folder' | 'note' | 'card'
    entity_id UUID NOT NULL,
    deleted_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_sync_tombstones_user_deleted_at
    ON sync_tombstones (user_id, deleted_at);

-- Evita duplicar tombstone se o mesmo id for apagado mais de uma vez (idempotência do push).
CREATE UNIQUE INDEX IF NOT EXISTS idx_sync_tombstones_unique
    ON sync_tombstones (user_id, entity_type, entity_id);
