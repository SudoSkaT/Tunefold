-- 0011: features armónicas y estructurales para el scoring de recomendaciones.
--
-- Amplía `track_acoustic_profiles` (aditivo, nunca destructivo) con:
--   * chroma_profile: 12 clases de pica de la cromática media (JSON).
--   * key_id: tonalidad estimada (-1 desconocida; 0..11 mayor; 12..23 menor).
--   * key_confidence: correlación de la estimación (0..1).
--   * chord_change_rate: tasa de cambios armónicos (cambios/s).
--   * structure_profile: 8 buckets temporales × [flux, rms, centroid, onset]
--     (JSON con 32 floats) — firma estructural de la pista.
--
-- Los perfiles antiguos quedan con valores por defecto (cromática cero,
-- tonalidad -1) y se re-calculan en la próxima reproducción de cada track.

PRAGMA foreign_keys = ON;

ALTER TABLE track_acoustic_profiles
    ADD COLUMN chroma_profile TEXT NOT NULL DEFAULT '[0,0,0,0,0,0,0,0,0,0,0,0]';

ALTER TABLE track_acoustic_profiles
    ADD COLUMN key_id INTEGER NOT NULL DEFAULT -1;

ALTER TABLE track_acoustic_profiles
    ADD COLUMN key_confidence REAL NOT NULL DEFAULT 0.0;

ALTER TABLE track_acoustic_profiles
    ADD COLUMN chord_change_rate REAL NOT NULL DEFAULT 0.0;

ALTER TABLE track_acoustic_profiles
    ADD COLUMN structure_profile TEXT NOT NULL DEFAULT
        '[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]';
