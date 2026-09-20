//! Letras sincronizadas en formato LRC, para el modo karaoke.
//!
//! Un LRC es texto plano con marcas de tiempo por línea: `[mm:ss.xx] texto`.
//! Este módulo solo conoce el formato; sacar los datos (LRCLIB) y renderizarlos
//! (UI) es responsabilidad de otras capas.

use std::time::Duration;

/// Una línea de letra sincronizada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricLine {
    /// Instante (desde el inicio de la canción) en que se canta la línea.
    pub time: Duration,
    /// Texto de la línea (puede estar vacío: instrumental/silencio).
    pub text: String,
}

/// Letra sincronizada: lista de líneas ordenadas por tiempo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncLyrics {
    /// Líneas ordenadas cronológicamente.
    pub lines: Vec<LyricLine>,
}

impl SyncLyrics {
    /// Parsea un bloque LRC tal y como lo entrega LRCLIB (`syncedLyrics`).
    ///
    /// Soporta `[mm:ss]`, `[mm:ss.xx]`, `[hh:mm:ss.xx]`, con o sin fracción, y
    /// varias marcas por línea (`[00:12.34][00:15.67] texto`). Las etiquetas de
    /// metadatos (`[ti:...]`, `[ar:...]`) y las líneas sin marca se ignoran.
    ///
    /// Etiqueta `[offset:±ms]` (estándar LRC, en milisegundos con signo): se
    /// aplica a TODAS las líneas (saturando en cero). Sin ella las letras con
    /// desplazamiento global sonarían sistemáticamente adelantadas o
    /// atrasadas; antes se ignoraba en silencio.
    pub fn parse(input: &str) -> SyncLyrics {
        let mut lines = Vec::new();
        let mut offset_ms: i64 = 0;
        for raw in input.lines() {
            let mut rest = raw.trim();
            if let Some(off) = strip_offset(rest) {
                offset_ms = off;
                continue;
            }
            let mut times = Vec::new();
            while let Some((time, tail)) = strip_timestamp(rest) {
                times.push(time);
                rest = tail.trim_start();
            }
            if times.is_empty() {
                continue;
            }
            let text = rest.trim().to_string();
            for time in times {
                let shifted = (time.as_millis() as i64 + offset_ms).max(0) as u64;
                lines.push(LyricLine {
                    time: Duration::from_millis(shifted),
                    text: text.clone(),
                });
            }
        }
        lines.sort_by_key(|l| l.time);
        SyncLyrics { lines }
    }

    /// ¿Hay algún verso sincronizado?
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Índice de la línea que debe cantarse en `position`: la última cuya
    /// marca de tiempo es `<= position`. `None` si aún no empezó ninguna.
    pub fn active_index(&self, position: Duration) -> Option<usize> {
        let n = self.lines.partition_point(|l| l.time <= position);
        n.checked_sub(1)
    }
}

/// Intenta extraer una marca de tiempo `[mm:ss.xx]` del inicio de `s`.
fn strip_timestamp(s: &str) -> Option<(Duration, &str)> {
    let s = s.strip_prefix('[')?;
    let (body, tail) = s.split_once(']')?;
    let time = parse_time(body)?;
    Some((time, tail))
}

/// Extrae la etiqueta global `[offset:±ms]` (milisegundos con signo, estándar
/// LRC). Solo vale al inicio de la línea y no genera verso.
fn strip_offset(s: &str) -> Option<i64> {
    let body = s.strip_prefix("[offset:")?.strip_suffix(']')?;
    body.trim().parse().ok()
}

/// Parsea un reloj `mm:ss.xx` (o `hh:mm:ss.xx`) a una duración.
fn parse_time(body: &str) -> Option<Duration> {
    let (clock, frac) = body.split_once('.').unwrap_or((body, ""));
    let parts: Vec<u64> = clock
        .split(':')
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().ok())
        .collect::<Option<Vec<_>>>()?;
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let secs = match parts.len() {
        1 => parts[0],
        2 => parts[0] * 60 + parts[1],
        _ => parts[0] * 3600 + parts[1] * 60 + parts[2],
    };
    let mut millis = secs * 1000;
    if !frac.is_empty() {
        let trimmed = frac.chars().take(3).collect::<String>();
        let digits = trimmed.len() as u32;
        let value: u64 = trimmed.parse().ok()?;
        // Centésimas de segundo (estándar LRC `.xx`) o milisegundos (`.xxx`).
        millis += value * 1000 / 10u64.pow(digits);
    }
    Some(Duration::from_millis(millis))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_lrc() {
        let lrc = "[00:12.34] primera línea\n[00:15.67] segunda\n[00:20] tercera\n";
        let sync = SyncLyrics::parse(lrc);
        assert_eq!(sync.lines.len(), 3);
        assert_eq!(sync.lines[0].time, Duration::from_millis(12_340));
        assert_eq!(sync.lines[0].text, "primera línea");
        assert_eq!(sync.lines[1].time, Duration::from_millis(15_670));
        assert_eq!(sync.lines[1].text, "segunda");
        assert_eq!(sync.lines[2].time, Duration::from_millis(20_000));
    }

    #[test]
    fn parse_multiple_timestamps_per_line() {
        let lrc = "[00:01.00][00:03.00][00:05.00] eco\n";
        let sync = SyncLyrics::parse(lrc);
        assert_eq!(sync.lines.len(), 3);
        assert_eq!(sync.lines[0].time, Duration::from_millis(1_000));
        assert_eq!(sync.lines[2].time, Duration::from_millis(5_000));
        assert_eq!(sync.lines[2].text, "eco");
    }

    #[test]
    fn parse_ignores_metadata_and_sorts() {
        let lrc = "[ti:Canción]\n[ar:Artista]\n[01:00.00] fin\n[00:10.00] inicio\n[00:05][00:07] juntas\n";
        let sync = SyncLyrics::parse(lrc);
        let times: Vec<u64> = sync
            .lines
            .iter()
            .map(|l| l.time.as_millis() as u64)
            .collect();
        assert_eq!(times, vec![5_000, 7_000, 10_000, 60_000]);
    }

    #[test]
    fn parse_hour_format_and_millis() {
        let lrc = "[00:01:02.500] alarma\n";
        let sync = SyncLyrics::parse(lrc);
        assert_eq!(sync.lines[0].time, Duration::from_millis(62_500));

        let lrc = "[00:02.123] milisegundos\n";
        let sync = SyncLyrics::parse(lrc);
        assert_eq!(sync.lines[0].time, Duration::from_millis(2_123));
    }

    #[test]
    fn active_index_tracks_position() {
        let lrc = "[00:05] a\n[00:10] b\n[00:15] c\n";
        let sync = SyncLyrics::parse(lrc);
        assert_eq!(sync.active_index(Duration::ZERO), None);
        assert_eq!(sync.active_index(Duration::from_secs(4)), None);
        assert_eq!(sync.active_index(Duration::from_secs(5)), Some(0));
        assert_eq!(sync.active_index(Duration::from_secs(14)), Some(1));
        assert_eq!(sync.active_index(Duration::from_secs(99)), Some(2));
    }

    #[test]
    fn empty_input_yields_no_lines() {
        assert!(SyncLyrics::parse("").is_empty());
        assert!(SyncLyrics::parse("[ti:x]\nsin tiempo\n").is_empty());
    }

    #[test]
    fn offset_tag_shifts_all_lines() {
        // Desplazamiento global negativo (caso típico: intro instrumental):
        // todas las marcas se adelantan y la etiqueta no genera verso.
        let lrc = "[offset:-2000]\n[00:10.00] diez\n[00:20.00] veinte\n";
        let sync = SyncLyrics::parse(lrc);
        assert_eq!(sync.lines.len(), 2);
        assert_eq!(sync.lines[0].time, Duration::from_millis(8_000));
        assert_eq!(sync.lines[1].time, Duration::from_millis(18_000));
        assert_eq!(sync.active_index(Duration::from_millis(8_500)), Some(0));
    }

    #[test]
    fn positive_offset_delays_and_clamps_at_zero() {
        let lrc = "[offset:+1500]\n[00:10.00] diez\n";
        let sync = SyncLyrics::parse(lrc);
        assert_eq!(sync.lines[0].time, Duration::from_millis(11_500));
        // Un offset que dejaría tiempos negativos satura en cero, sin pánico.
        let lrc = "[offset:-99999]\n[00:10.00] diez\n";
        let sync = SyncLyrics::parse(lrc);
        assert_eq!(sync.lines[0].time, Duration::ZERO);
    }

    #[test]
    fn malformed_offset_is_ignored_like_metadata() {
        // `[offset:x]` no numérico no desplaza nada ni genera versos.
        let lrc = "[offset:pronto]\n[00:10.00] diez\n";
        let sync = SyncLyrics::parse(lrc);
        assert_eq!(sync.lines.len(), 1);
        assert_eq!(sync.lines[0].time, Duration::from_millis(10_000));
    }
}
