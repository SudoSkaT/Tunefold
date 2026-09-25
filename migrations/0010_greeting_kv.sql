-- 0010: kv genérico para saludo diario y nombre del usuario.
--
-- No toca tablas previas: solo añade `kv(key PRIMARY KEY, value, updated_at)`.
-- Claves usadas:
-- - `last_greeting_date` = `YYYY-MM-DD` (fecha local del último saludo mostrado).
-- - `display_name` = nombre elegido por el usuario (vacío = sin personalizar).

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS kv (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
