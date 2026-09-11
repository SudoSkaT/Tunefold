//! Cliente de LRCLIB: letras sincronizadas (LRC) del karaoke.
//!
//! Servicio legítimo (sin API key) e independiente de cualquier proveedor de
//! streaming: vive fuera de `providers::youtube` para que el karaoke siga
//! funcionando en los binarios SIN feature YouTube. La letra plana NUNCA se
//! mezcla como alternativa. La búsqueda puntúa por normalización de
//! título/artista y cercanía de duración, descartando instrumentales y falsos
//! positivos.
//!
//! LRCLIB pide identificar la app en cada petición: [`lrclib_client`] fija el
//! User-Agent `Tunefold/<versión>` (contacto: repositorio GitHub).

use std::time::Duration;

use crate::domain::track::Track;

/// API pública de LRCLIB: letras sincronizadas (LRC) sin API key. La consulta
/// se hace por búsqueda de firma del track (título + artista); la duración
/// solo desempata entre registros.
const LRCLIB_API: &str = "https://lrclib.net/api/search";

/// Cliente HTTP dedicado a LRCLIB, con el User-Agent identificativo que pide
/// el servicio y un timeout prudente por petición.
pub(crate) fn lrclib_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(format!("Tunefold/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(8))
        .build()
        .expect("cliente reqwest válido")
}

/// Consulta LRCLIB por búsqueda (`GET /api/search`) y devuelve la letra
/// **sincronizada** (LRC) del mejor registro, si existe.
///
/// Es la única fuente del karaoke: si LRCLIB solo ofrece `plainLyrics`, se
/// devuelve `None` (sin LRC no hay karaoke sincronizado y la letra plana no se
/// mezcla como fuente alternativa). La búsqueda no exige duración exacta (más
/// aciertos); el registro se puntúa por artista + título + cercanía de duración
/// y se descartan instrumentales y falsos positivos por título parecido.
pub(crate) async fn fetch_lrclib_lyrics(http: &reqwest::Client, track: &Track) -> Option<String> {
    let title = track.title.trim();
    if title.is_empty() {
        return None;
    }
    let artist = track
        .primary_artist_name()
        .unwrap_or_default()
        .trim()
        .to_string();
    let seconds = track.duration.map(|d| d.as_secs()).unwrap_or(0);

    let mut url = reqwest::Url::parse(LRCLIB_API).ok()?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("track_name", title);
        if !artist.is_empty() {
            q.append_pair("artist_name", &artist);
        }
        q.append_pair("page_size", "20");
    }
    let resp = http.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        // 404 = sin registros; 429 = rate-limit. Ambos se tratan como "sin
        // letras" aquí.
        return None;
    }
    let results: Vec<serde_json::Value> = resp.json().await.ok()?;

    let best = results
        .into_iter()
        .filter_map(|r| {
            let score = lrclib_score(&r, title, &artist, seconds)?;
            Some((score, r))
        })
        .min_by_key(|(score, _)| *score)?
        .1;

    best.get("syncedLyrics")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
}

/// Clasifica un registro de búsqueda de LRCLIB para el track pedido.
///
/// Devuelve `None` si no sirve (instrumental, título irrelevante, o "parecido"
/// sin respaldo de artista y duración). Si sirve, una puntuación ordenable en
/// la que **menor es mejor**: `(nivel de título, ¿no coincide artista?, Δ
/// duración)`.
///
/// Niveles de título:
/// - 0: igual tras normalizar (mayúsculas, tildes, espacios, puntuación);
/// - 1: igual tras quitar sufijos genéricos entre paréntesis/corchetes
///   (p. ej. «(Official Video)», «(Live)», «(Remaster)») — exige Δ duración
///   razonable;
/// - 2: contención (uno contiene al otro, p. ej. «(Remix)» o «(Feat. X)») —
///   exige artista que coincida y Δ duración pequeña.
///
/// Así un título solo *parecido* (otra canción) no se acepta: necesita artista
/// y duración en el nivel de contención.
fn lrclib_score(
    record: &serde_json::Value,
    title: &str,
    artist: &str,
    seconds: u64,
) -> Option<(u8, u8, u64)> {
    if record
        .get("instrumental")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return None;
    }
    let hit = record.get("trackName").and_then(|v| v.as_str())?;
    let hit_artist = record
        .get("artistName")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let nt = normalize(title);
    let nh = normalize(hit);
    if nt.is_empty() || nh.is_empty() {
        return None;
    }

    let level = if nh == nt {
        0
    } else {
        let kt = title_key(title);
        let kh = title_key(hit);
        if kt.is_empty() || kh.is_empty() {
            return None;
        }
        if kh == kt {
            1
        } else if nh.contains(&nt) || nt.contains(&nh) {
            2
        } else {
            return None;
        }
    };

    let a = artist_core(artist);
    let ha = artist_core(hit_artist);
    let artist_ok = a.is_empty() || a == ha || a.contains(&ha) || ha.contains(&a);

    let dur_diff = record
        .get("duration")
        .and_then(|v| v.as_u64())
        .map(|d| d.abs_diff(seconds))
        .unwrap_or(u64::MAX);

    // Reglas anti falso positivo: el título idéntico basta; una variante exige
    // duración razonable; la contención exige además artista que coincida.
    match level {
        0 => {}
        1 => {
            if dur_diff > 20 {
                return None;
            }
        }
        _ => {
            if !artist_ok || dur_diff > 10 {
                return None;
            }
        }
    }

    Some((level, u8::from(!artist_ok), dur_diff))
}

/// Normaliza texto para comparar: minúsculas, sin diacríticos, puntuación
/// colapsada a espacios y espacios compactados. «Canción (Remix)» →
/// «cancion remix». Cubre mayúsculas/minúsculas, tildes y espacios extra.
fn normalize(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Sufijos genéricos de portales/versiones que no cambian la letra. No incluye
/// «remix» (una versión distinta no debe confundirse con la canción original).
const GENERIC_QUALIFIERS: &[&str] = &[
    "official video",
    "official music video",
    "music video",
    "lyric video",
    "lyrics",
    "audio",
    "hd",
    "4k",
    "hq",
    "official",
    "visualizer",
    "video",
    "album version",
    "acoustic",
    "radio edit",
    "remaster",
    "remastered",
    "version",
    "live",
    "bonus track",
    "single",
    "deluxe",
];

/// Título normalizado sin los sufijos genéricos entre paréntesis/corchetes:
/// «Song (Official Video)» → «song»; «Song (Remix)» → «song remix».
fn title_key(s: &str) -> String {
    let mut out = String::new();
    let mut token = String::new();
    let mut in_group = false;
    for c in s.chars() {
        match c {
            '(' | '[' | '{' => in_group = true,
            ')' | ']' | '}' => {
                in_group = false;
                let t = normalize(&token);
                token.clear();
                if !t.is_empty() && !GENERIC_QUALIFIERS.contains(&t.as_str()) {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(&t);
                }
            }
            _ if in_group => token.push(c),
            _ => out.push(c),
        }
    }
    if in_group && !token.is_empty() {
        let t = normalize(&token);
        if !t.is_empty() && !GENERIC_QUALIFIERS.contains(&t.as_str()) {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&t);
        }
    }
    normalize(&out)
}

/// Núcleo del artista: se ignora el sufijo de colaboración («feat.», «ft.»,
/// «featuring», «con») para que el artista principal gane la comparación.
fn artist_core(s: &str) -> String {
    let norm = normalize(s);
    for sep in ["featuring", "feat", "ft"] {
        if let Some(i) = norm.find(sep) {
            let head = norm[..i].trim();
            return if head.is_empty() {
                norm
            } else {
                head.to_string()
            };
        }
    }
    norm
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrclib_score_picks_exact_title_and_artist_first() {
        let exact = serde_json::from_str(
            r#"{"trackName":"Never Gonna Give You Up","artistName":"Rick Astley","duration":213,"instrumental":false}"#,
        ).unwrap();
        let live = serde_json::from_str(
            r#"{"trackName":"Never Gonna Give You Up (Live)","artistName":"Rick Astley","duration":231,"instrumental":false}"#,
        ).unwrap();
        let sa = lrclib_score(&exact, "Never Gonna Give You Up", "Rick Astley", 213).unwrap();
        let sb = lrclib_score(&live, "Never Gonna Give You Up", "Rick Astley", 213).unwrap();
        assert!(
            sa < sb,
            "la coincidencia exacta puntúa mejor que la versión Live"
        );
    }
    #[test]
    fn lrclib_score_uses_duration_as_tiebreak() {
        let close = serde_json::from_str(
            r#"{"trackName":"Bohemian Rhapsody","artistName":"Queen","duration":355,"instrumental":false}"#,
        ).unwrap();
        let far = serde_json::from_str(
            r#"{"trackName":"Bohemian Rhapsody","artistName":"Queen","duration":400,"instrumental":false}"#,
        ).unwrap();
        let (_, _, d1) = lrclib_score(&close, "Bohemian Rhapsody", "Queen", 354).unwrap();
        let (_, _, d2) = lrclib_score(&far, "Bohemian Rhapsody", "Queen", 354).unwrap();
        assert!(d1 < d2, "la duración cercana desempata");
    }
    #[test]
    fn lrclib_score_rejects_instrumental_and_foreign_title() {
        let instrumental = serde_json::from_str(
            r#"{"trackName":"Bohemian Rhapsody","artistName":"Queen","duration":354,"instrumental":true}"#,
        ).unwrap();
        assert!(lrclib_score(&instrumental, "Bohemian Rhapsody", "Queen", 354).is_none());

        let foreign = serde_json::from_str(
            r#"{"trackName":"Otra Canción","artistName":"Queen","duration":354,"instrumental":false}"#,
        ).unwrap();
        assert!(lrclib_score(&foreign, "Bohemian Rhapsody", "Queen", 354).is_none());
    }
    #[test]
    fn lrclib_score_normalizes_case_accents_punctuation_and_spaces() {
        // Título con acentos, mayúsculas y espacios extra: coincide exacto.
        let cafe = serde_json::from_str(
            r#"{"trackName":"Café  —  Con Leche","artistName":"La Oreja","duration":200,"instrumental":false}"#,
        ).unwrap();
        let (level, artist_bad, _) =
            lrclib_score(&cafe, "  CAFE CON LECHE ", "la oreja", 200).unwrap();
        assert_eq!(level, 0, "la normalización iguala títulos");
        assert_eq!(artist_bad, 0, "el artista también se normaliza");
    }
    #[test]
    fn lrclib_score_strips_generic_qualifiers_between_parens() {
        // «(Official Video)» es un sufijo de portal: misma canción, nivel 1.
        let video = serde_json::from_str(
            r#"{"trackName":"Song (Official Video)","artistName":"Artist","duration":200,"instrumental":false}"#,
        ).unwrap();
        let (level, _, _) = lrclib_score(&video, "Song", "Artist", 200).unwrap();
        assert_eq!(level, 1, "sufijo genérico no cambia la canción");

        // «(Remix)» es otra versión: contención, exige duración cercana.
        let remix_close = serde_json::from_str(
            r#"{"trackName":"Song (Remix)","artistName":"Artist","duration":205,"instrumental":false}"#,
        ).unwrap();
        let (level, _, _) = lrclib_score(&remix_close, "Song", "Artist", 200).unwrap();
        assert_eq!(level, 2, "remix = contención con duración");

        // Si el remix dura demasiado (otra versión larga), se descarta.
        let remix_far = serde_json::from_str(
            r#"{"trackName":"Song (Extended Remix)","artistName":"Artist","duration":500,"instrumental":false}"#,
        ).unwrap();
        assert!(
            lrclib_score(&remix_far, "Song", "Artist", 200).is_none(),
            "remix con duración muy distinta no es la canción"
        );
    }
    #[test]
    fn lrclib_score_handles_featuring_and_versions() {
        // El artista principal manda: «feat.» no rompe la comparación.
        let feat = serde_json::from_str(
            r#"{"trackName":"We Found Love (feat. Calvin Harris)","artistName":"Rihanna","duration":216,"instrumental":false}"#,
        ).unwrap();
        let (level, artist_bad, _) =
            lrclib_score(&feat, "We Found Love (feat. Calvin Harris)", "Rihanna", 216).unwrap();
        assert_eq!(level, 0);
        assert_eq!(artist_bad, 0, "el feat. se ignora en el artista");

        // «(2011 Remaster)»: se resuelve por contención con artista y duración.
        let remaster = serde_json::from_str(
            r#"{"trackName":"Song (2011 Remaster)","artistName":"Artist","duration":200,"instrumental":false}"#,
        ).unwrap();
        let (level, _, _) = lrclib_score(&remaster, "Song", "Artist", 200).unwrap();
        assert_eq!(level, 2, "versión remasterizada: contención validada");

        // «(Remaster)» puro sí se trata como sufijo genérico (nivel 1).
        let remaster_plain = serde_json::from_str(
            r#"{"trackName":"Song (Remaster)","artistName":"Artist","duration":200,"instrumental":false}"#,
        ).unwrap();
        let (level, _, _) = lrclib_score(&remaster_plain, "Song", "Artist", 200).unwrap();
        assert_eq!(level, 1);
    }
    #[test]
    fn lrclib_score_rejects_similar_title_with_wrong_artist() {
        // Título *parecido* (contención) pero de otro artista: es otra canción,
        // no debe aceptarse aunque la duración sea cercana.
        let other_song = serde_json::from_str(
            r#"{"trackName":"Song of Ice and Fire","artistName":"Some Band","duration":200,"instrumental":false}"#,
        ).unwrap();
        assert!(
            lrclib_score(&other_song, "Song", "Queen", 200).is_none(),
            "no se acepta otra canción solo por el título parecido"
        );

        // Misma canción, artista sin acentos ni mayúsculas: sí.
        let same = serde_json::from_str(
            r#"{"trackName":"Song","artistName":"Queen","duration":200,"instrumental":false}"#,
        )
        .unwrap();
        assert!(lrclib_score(&same, "song", "queen", 200).is_some());
    }
    #[test]
    fn lrclib_score_rejects_wrong_duration_on_containment() {
        // Contención con artista correcto pero duración muy lejana: descartada.
        let far = serde_json::from_str(
            r#"{"trackName":"Song (Live 2024)","artistName":"Artist","duration":900,"instrumental":false}"#,
        ).unwrap();
        assert!(
            lrclib_score(&far, "Song", "Artist", 200).is_none(),
            "duración descarta versiones que no son la canción"
        );
    }
}
