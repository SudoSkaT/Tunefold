-- 0009: arquitectura de playlists persistentes (FASE playlists/L1K3D).
--
-- Extiende el modelo local de playlists hacia la base de todas las
-- funcionalidades planeadas: playlists persistentes con metadata, L1K3D como
-- playlist de SISTEMA no eliminable, membership con orden y fecha, y caché
-- persistente de la paleta de colores del artwork (para no recalcular nunca
-- durante el render).
--
-- No toca tablas ni columnas previas: solo añade columnas, tablas e índices
-- nuevos, de modo que las bases existentes migran sin pérdida de datos.

PRAGMA foreign_keys = ON;

-- ---------------------------------------------------------------- playlists
-- `kind`: 0 = User (creada/renombrada/eliminada libremente), 1 = System
--         (L1K3D: autogenerada, no se renombra ni se elimina).
-- `updated_at`: marcado de DEFECTO a `created_at` por filas históricas.
ALTER TABLE playlists ADD COLUMN kind INTEGER NOT NULL DEFAULT 0;
ALTER TABLE playlists ADD COLUMN updated_at TEXT;
UPDATE playlists SET updated_at = created_at WHERE updated_at IS NULL;

-- La playlist especial L1K3D existe SIEMPRE (semántica de "like" del
-- producto): se autogenera si no hay ninguna fila con ese nombre reservado y,
-- si una playlist de usuario anterior usaba ese nombre, se promociona a
-- sistema (el nombre queda reservado).
INSERT INTO playlists (name, kind, created_at, updated_at)
SELECT 'L1K3D', 1, datetime('now'), datetime('now')
WHERE NOT EXISTS (SELECT 1 FROM playlists WHERE name = 'L1K3D');

UPDATE playlists SET kind = 1 WHERE name = 'L1K3D' AND kind = 0;

-- -------------------------------------------------------- membership (items)
-- `added_at`: cuándo se añadió cada canción (orden temporal de la membership).
ALTER TABLE playlist_tracks ADD COLUMN added_at TEXT NOT NULL DEFAULT (datetime('now'));

-- --------------------------------------------------------- artwork palette
-- Caché persistente de los tres colores dominantes del artwork (extraídos al
-- decodificar la miniatura). La paleta pertenece al TRACK (su artwork
-- efectivo, incluido el override), nunca se recalcula durante el render y se
-- sirve desde aquí cuando la miniatura aún no está decodificada en memoria.
CREATE TABLE IF NOT EXISTS artwork_palettes (
    track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    primary_hex   TEXT NOT NULL,
    secondary_hex TEXT NOT NULL,
    accent_hex    TEXT NOT NULL,
    updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ------------------------------------------------------- defensa en BD
-- L1K3D no se puede eliminar ni renombrar NI A NIVEL DE BASE DE DATOS. La capa
-- de aplicación también lo guarda; estos triggers son la red de seguridad para
-- cualquier futuro consumidor de SQL.
CREATE TRIGGER IF NOT EXISTS trg_playlists_system_no_delete
BEFORE DELETE ON playlists
WHEN OLD.kind = 1
BEGIN
    SELECT RAISE(ABORT, 'no se puede eliminar una playlist de sistema (L1K3D)');
END;

CREATE TRIGGER IF NOT EXISTS trg_playlists_system_no_rename
BEFORE UPDATE OF name ON playlists
WHEN OLD.kind = 1 AND NEW.name <> OLD.name
BEGIN
    SELECT RAISE(ABORT, 'no se puede renombrar una playlist de sistema (L1K3D)');
END;

-- Cualquier cambio en la membership marca la playlist como modificada.
CREATE TRIGGER IF NOT EXISTS trg_playlist_tracks_touch_add
AFTER INSERT ON playlist_tracks
BEGIN
    UPDATE playlists SET updated_at = datetime('now') WHERE id = NEW.playlist_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_playlist_tracks_touch_remove
AFTER DELETE ON playlist_tracks
BEGIN
    UPDATE playlists SET updated_at = datetime('now') WHERE id = OLD.playlist_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_playlist_tracks_touch_move
AFTER UPDATE OF position ON playlist_tracks
BEGIN
    UPDATE playlists SET updated_at = datetime('now') WHERE id = NEW.playlist_id;
END;

-- Orden de lectura por defecto: posición ascendente dentro de la playlist.
CREATE INDEX IF NOT EXISTS idx_playlist_tracks_order
    ON playlist_tracks(playlist_id, position);