//! Parser puro de enlaces YouTube → recurso interno.
//!
//! Sin red, sin regex, sin allocs innecesarias: solo clasifica la URL para que
//! el backend decida (`get_track` para video; playlist remota = preview +
//! importación como copia local, sin espejo sincronizado por decisión Q2).
//!
//! Formatos aceptados:
//! - `https://www.youtube.com/watch?v=ID` (+ `&list=PLID` opcional, se ignora
//!   en favor del video según decisión de copia local)
//! - `https://music.youtube.com/watch?v=ID`, `m.youtube.com`, sin `www`
//! - `https://youtu.be/ID`
//! - `https://www.youtube.com/shorts/ID`
//! - `https://www.youtube.com/playlist?list=PLID` (y `music.`/`m.`)

/// Recurso identificado en una URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkKind {
    /// Video individual (`video_id` listo para `get_track`).
    VideoId(String),
    /// Playlist remota (`list=...`). Tunefold la importa como **copia local**,
    /// nunca como espejo sincronizado.
    PlaylistId(String),
    /// No es un enlace YouTube reconocible.
    Invalid,
}

/// Clasifica `raw` (recorta espacios). Nunca toca red.
pub fn parse_link(raw: &str) -> LinkKind {
    let s = raw.trim();
    if s.is_empty() {
        return LinkKind::Invalid;
    }
    // Sin esquema ni host reconocible: no es un link (la búsqueda por texto
    // sigue su camino normal hacia `music_search_tracks`).
    let lower = s.to_ascii_lowercase();
    let after_scheme = if lower.strip_prefix("https://").is_some() {
        &s["https://".len()..]
    } else if let Some(rest) = lower.strip_prefix("http://") {
        let _ = rest;
        &s["http://".len()..]
    } else if lower.starts_with("www.")
        || lower.starts_with("youtu.be")
        || lower.starts_with("music.")
        || lower.starts_with("m.youtube")
        || lower.starts_with("youtube.com")
    {
        s
    } else {
        return LinkKind::Invalid;
    };

    let host_end = after_scheme.find('/').map_or(after_scheme.len(), |i| i);
    let host = after_scheme[..host_end].to_ascii_lowercase();
    let path_query = &after_scheme[host_end..];

    let is_youtube_host = host == "youtube.com"
        || host.ends_with(".youtube.com")
        || host == "youtu.be"
        || host == "www.youtube.com"
        || host == "music.youtube.com"
        || host == "m.youtube.com";
    if !is_youtube_host {
        return LinkKind::Invalid;
    }

    // youtu.be/ID
    if host == "youtu.be" {
        let id = take_segment(path_query.trim_start_matches('/'));
        return valid_video(&id)
            .map(LinkKind::VideoId)
            .unwrap_or(LinkKind::Invalid);
    }

    // /shorts/ID
    if let Some(rest) = path_query.strip_prefix("/shorts/") {
        let id = take_segment(rest);
        return valid_video(&id)
            .map(LinkKind::VideoId)
            .unwrap_or(LinkKind::Invalid);
    }

    // /playlist?list=PLID
    if path_query.starts_with("/playlist") {
        if let Some(list) = query_param(path_query, "list") {
            return valid_list(&list)
                .map(LinkKind::PlaylistId)
                .unwrap_or(LinkKind::Invalid);
        }
        return LinkKind::Invalid;
    }

    // /watch?v=ID (&list=PLID opcional: manda el video por decisión Q2 copia)
    if path_query.starts_with("/watch") {
        if let Some(v) = query_param(path_query, "v") {
            if let Some(id) = valid_video(&v) {
                return LinkKind::VideoId(id);
            }
        }
        // /watch sin v pero con list → trátalo como playlist (copia local).
        if let Some(list) = query_param(path_query, "list") {
            return valid_list(&list)
                .map(LinkKind::PlaylistId)
                .unwrap_or(LinkKind::Invalid);
        }
        return LinkKind::Invalid;
    }

    LinkKind::Invalid
}

/// ¿Parece URL (para que Search la desvíe a `ResolveLink`)? Barato y sin
/// falsos positivos sobre títulos normales.
pub fn looks_like_url(raw: &str) -> bool {
    let s = raw.trim().to_ascii_lowercase();
    s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("www.youtube.com")
        || s.starts_with("youtube.com")
        || s.starts_with("music.youtube.com")
        || s.starts_with("m.youtube.com")
        || s.starts_with("youtu.be")
}

/// Segmento hasta `? # & /` (y recorta espacios).
fn take_segment(s: &str) -> String {
    s.split(['?', '#', '&', '/'])
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Parámetro `key` del query (`?a=1&key=VAL` o `&key=VAL`). Sin decoding
/// percent: los IDs de YouTube son base64url y nunca lo necesitan.
fn query_param(path_query: &str, key: &str) -> Option<String> {
    let q = path_query.split('?').nth(1)?;
    for pair in q.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next()?.trim() == key {
            let v = it.next().unwrap_or("").trim();
            if !v.is_empty() {
                return Some(take_segment(v));
            }
        }
    }
    None
}

fn valid_video(id: &str) -> Option<String> {
    let id = id.trim();
    // IDs reales: 11 chars base64url; se acepta 6..64 por tolerancia futura.
    if (6..=64).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Some(id.to_string())
    } else {
        None
    }
}

fn valid_list(id: &str) -> Option<String> {
    let id = id.trim();
    if (2..=128).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Some(id.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_variants_resolve_video() {
        assert_eq!(
            parse_link("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            LinkKind::VideoId("dQw4w9WgXcQ".into())
        );
        assert_eq!(
            parse_link("https://music.youtube.com/watch?v=dQw4w9WgXcQ&list=PL123"),
            // Manda el video (copia local Q2), la lista se ignora aquí.
            LinkKind::VideoId("dQw4w9WgXcQ".into())
        );
        assert_eq!(
            parse_link("http://m.youtube.com/watch?v=abc123XYZ-_"),
            LinkKind::VideoId("abc123XYZ-_".into())
        );
    }

    #[test]
    fn short_and_shorts_resolve_video() {
        assert_eq!(
            parse_link("https://youtu.be/dQw4w9WgXcQ"),
            LinkKind::VideoId("dQw4w9WgXcQ".into())
        );
        assert_eq!(
            parse_link("https://www.youtube.com/shorts/dQw4w9WgXcQ?t=10"),
            LinkKind::VideoId("dQw4w9WgXcQ".into())
        );
    }

    #[test]
    fn playlist_resolves_copy_target() {
        assert_eq!(
            parse_link("https://www.youtube.com/playlist?list=PL123abc"),
            LinkKind::PlaylistId("PL123abc".into())
        );
        assert_eq!(
            parse_link("https://www.youtube.com/watch?list=PL123abc"),
            LinkKind::PlaylistId("PL123abc".into())
        );
    }

    #[test]
    fn invalid_inputs_stay_search() {
        for raw in [
            "",
            "   ",
            "queen bohemian rhapsody",
            "https://example.com/watch?v=dQw4w9WgXcQ",
            "https://www.youtube.com/watch",
            "https://youtu.be/",
            "https://www.youtube.com/watch?v=!!!",
            "ftp://youtube.com/watch?v=dQw4w9WgXcQ",
        ] {
            assert_eq!(parse_link(raw), LinkKind::Invalid, "{raw}");
        }
    }

    #[test]
    fn detector_avoids_false_positives() {
        assert!(looks_like_url("https://youtu.be/dQw4w9WgXcQ"));
        assert!(looks_like_url("  www.youtube.com/watch?v=x  "));
        assert!(!looks_like_url("queen bohemian rhapsody"));
        assert!(!looks_like_url("watch?v=algo"));
    }
}
