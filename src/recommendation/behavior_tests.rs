//! Tests de comportamiento deterministas (FASE 12) del sistema de
//! recomendaciones y del modelo de señales. NO dependen de Internet: todo son
//! fixtures locales.

use std::collections::HashMap;
use std::time::Duration;

use chrono::NaiveDate;

use crate::domain::{album::Album, artist::Artist, genre::Genre, source::Source, track::Track};
use crate::recommendation::{
    acoustic_similarity, metadata_similarity, negative_penalty, popularity_factor, rank,
    recency_bonus, user_affinity, PlayContext, PlaySignal, SignalKind, TrackAcousticProfile,
    TrackSignals, UserProfile,
};

// ───────────────────────────────────────────────────────────── helpers

fn track(id: i64, title: &str, artist: &str, genre: &str, year: i32) -> Track {
    let mut t = Track::new(
        title.to_string(),
        vec![Artist::new(artist.to_string(), None, None, None)],
        Source::YouTube,
    );
    t.id = id;
    t.duration = Some(Duration::from_secs(200));
    t.genres = vec![Genre::new(genre.to_string())];
    t.album = Some(Album::new(
        "Album".to_string(),
        NaiveDate::from_ymd_opt(year, 6, 1),
        None,
        None,
    ));
    t
}

fn signal(
    track_id: i64,
    kind: SignalKind,
    ctx: PlayContext,
    dur_ms: i64,
    td_ms: i64,
) -> PlaySignal {
    PlaySignal {
        id: track_id,
        track_id,
        signal: kind,
        context: ctx,
        at: "2024-06-15 12:00:00".to_string(),
        duration_ms: Some(dur_ms),
        recomm_id: None,
        track_duration_ms: Some(td_ms),
    }
}

fn acoustic(track_id: i64, bpm: f32, bass: f32, high: f32) -> TrackAcousticProfile {
    TrackAcousticProfile {
        track_id,
        rms_mean: 0.3,
        bass_mean: bass,
        low_mid_mean: 0.2,
        mid_mean: 0.2,
        high_mid_mean: 0.2,
        high_mean: high,
        spectral_centroid_mean: 0.5,
        bpm_mean: bpm,
        bpm_variance: 0.0,
        onset_mean: 0.1,
        band_profile: [bass, 0.2, 0.2, 0.2, high],
        frame_count: 100,
        ..TrackAcousticProfile::default()
    }
}

fn prof(bass: f32, mid: f32, high: f32, bpm: f32, centroid: f32) -> TrackAcousticProfile {
    TrackAcousticProfile {
        track_id: 0,
        rms_mean: 0.3,
        bass_mean: bass,
        low_mid_mean: mid,
        mid_mean: mid,
        high_mid_mean: high,
        high_mean: high,
        spectral_centroid_mean: centroid,
        bpm_mean: bpm,
        bpm_variance: 0.0,
        onset_mean: 0.1,
        band_profile: [bass, mid, mid, high, high],
        frame_count: 100,
        ..TrackAcousticProfile::default()
    }
}

// ─────────────────────────────────────────────────────── FASE 9: metadata

#[test]
fn metadata_similarity_favors_shared_artist_and_genre() {
    let rock = track(1, "A", "The Beatles", "rock", 1965);
    let same_artist = track(2, "B", "The Beatles", "pop", 1970);
    let diff_artist = track(3, "C", "Other", "jazz", 2010);

    let mut tracks = HashMap::new();
    for t in [&rock, &same_artist, &diff_artist] {
        tracks.insert(t.id, t.clone());
    }
    // El usuario escucha mucho "The Beatles" y "rock".
    let signals = vec![
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
    ];
    let profile = UserProfile::from_signals(&signals, &tracks, &HashMap::new());
    assert!(profile
        .favorite_artists
        .contains(&"The Beatles".to_string()));
    assert!(profile.favorite_genres.contains(&"rock".to_string()));

    let same = metadata_similarity(&same_artist, &profile);
    let diff = metadata_similarity(&diff_artist, &profile);
    assert!(same > diff, "comparte artista ⇒ mayor similitud metadata");
    assert!(same > 0.0);
}

#[test]
fn metadata_similarity_uses_decade_when_album_date_known() {
    let user_track = track(1, "A", "X", "dance", 1998);
    let mut tracks = HashMap::new();
    tracks.insert(1, user_track.clone());
    let profile = UserProfile::from_signals(
        &[signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        )],
        &tracks,
        &HashMap::new(),
    );
    assert!(profile.favorite_decades.contains(&1990));

    let same_decade = track(2, "B", "Y", "pop", 1996);
    let other_decade = track(3, "C", "Z", "pop", 2010);
    assert!(
        metadata_similarity(&same_decade, &profile) > metadata_similarity(&other_decade, &profile),
        "la década favorita pesa"
    );
}

// ─────────────────────────────────────────────────────── FASE 9: acoustic

#[test]
fn acoustic_similarity_ranks_sonically_similar_tracks() {
    let bassy = prof(0.8, 0.2, 0.1, 90.0, 0.2);
    let another_bassy = prof(0.7, 0.25, 0.15, 95.0, 0.25);
    let trebly = prof(0.1, 0.3, 0.9, 140.0, 0.9);

    let s1 = acoustic_similarity(&bassy, &another_bassy);
    let s2 = acoustic_similarity(&bassy, &trebly);
    assert!(s1 > s2, "dos tracks graves son más parecidos entre sí");
    assert!(s1 > 0.5);
}

#[test]
fn acoustic_similarity_is_symmetric() {
    let a = prof(0.5, 0.4, 0.3, 110.0, 0.5);
    let b = prof(0.6, 0.5, 0.4, 120.0, 0.6);
    let ab = acoustic_similarity(&a, &b);
    let ba = acoustic_similarity(&b, &a);
    assert!((ab - ba).abs() < 1e-6);
}

// ─────────────────────────────────────────────────────── FASE 9: affinity

#[test]
fn user_affinity_prefers_closer_acoustic_match() {
    // Dos tracks con metadata y engagement IDÉNTICOS (misma artista, misma
    // frecuencia de escucha): la afinidad se desempata por perfil acústico, que
    // es determinista (no depende del reloj de pared). El perfil del usuario se
    // construye desde un track grave/lento; el track #1 comparte ese sonido y
    // el #2 es agudo/rápido.
    let mut tracks = HashMap::new();
    let t1 = track(1, "A", "Beat", "rock", 2000);
    let t2 = track(2, "B", "Beat", "rock", 2001);
    tracks.insert(1, t1.clone());
    tracks.insert(2, t2.clone());

    // Usuario: escucha t1 hasta el final (grave, lento).
    let sigs = vec![
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
    ];
    let aps = HashMap::from([
        (1, acoustic(1, 90.0, 0.8, 0.1)),
        (2, acoustic(2, 160.0, 0.1, 0.9)),
    ]);
    let profile = UserProfile::from_signals(&sigs, &tracks, &aps);
    assert!(profile.acoustic_profile.bass > 0.5, "perfil grave");

    // engagement: ambos tracks NO están en el historial de legado (no se usó),
    // así que engagement ≈ 0 para ambos; decide la afinidad acústica.
    let user_acoustic = profile.acoustic_profile.clone();
    let empty_hist: Vec<crate::infrastructure::storage::TrackListeningStats> = vec![];

    let a1 = user_affinity(&t1, &empty_hist, &user_acoustic, &aps);
    let a2 = user_affinity(&t2, &empty_hist, &user_acoustic, &aps);
    assert!(a1 > a2, "el sonido igual al gusto tiene mayor afinidad");
}

// ─────────────────────────────────────────────────────── FASE 9: negative

#[test]
fn negative_penalty_flags_skipped_tracks() {
    // 50% skips manuales ⇒ penalización fuerte.
    let p = negative_penalty(5, 10);
    assert!(p < 1.0);
    let low = negative_penalty(1, 10);
    assert!(p < low, "más skips ⇒ más penalización");
}

#[test]
fn negative_ratio_is_smooth_and_bounded() {
    for i in 0..=10 {
        let p = negative_penalty(i, 10);
        assert!(p > 0.0 && p <= 1.0);
    }
    // Sin señal negativa, incluso con muchos plays, no penaliza.
    assert!((negative_penalty(0, 50) - 1.0).abs() < 1e-9);
}

// ─────────────────────────────────────────────────────── FASE 9: recency

#[test]
fn recency_bonus_decays_and_clamps() {
    assert!((recency_bonus(0.0) - 1.0).abs() < 1e-9, "ahora = 1.0");
    let recent = recency_bonus(1.0);
    let old = recency_bonus(60.0);
    assert!(recent > old, "lo reciente pesa más");
    assert!(old > 0.0);
}

// ─────────────────────────────────────────────────────── FASE 9: popularity

#[test]
fn popularity_is_log_normalized_not_linear() {
    assert!((popularity_factor(0, 100) - 0.0).abs() < 1e-9);
    let p1 = popularity_factor(1, 100);
    let p100 = popularity_factor(100, 100);
    assert_eq!(p100, 1.0);
    // Un factor 100x en plays NO multiplica 100x el score (log suaviza).
    assert!(p100 < p1 * 20.0);
}

// ─────────────────────────────────────────────────────── FASE 10: profile

#[test]
fn profile_does_not_treat_play_as_like() {
    let mut tracks = HashMap::new();
    tracks.insert(1, track(1, "A", "Beat", "rock", 2000));

    // Mil "plays" por AUTOPLAY (sin elección consciente) no deben dominar el
    // perfil ni contar como likes.
    let autoplay: Vec<PlaySignal> = (0..1000)
        .map(|i| {
            signal(
                i as i64,
                SignalKind::Play,
                PlayContext::Autoplay,
                30_000,
                200_000,
            )
        })
        .collect();
    let profile = UserProfile::from_signals(&autoplay, &tracks, &HashMap::new());
    // El peso por contexto autoplay (0.4) es considerablemente menor que un
    // like manual.
    assert!(profile.total_weight < 1000.0 * 1.5, "autoplay pesa poco");
    assert_eq!(profile.total_likes, 0, "play ≠ like");
}

#[test]
fn manual_like_weights_more_than_autoplay_play() {
    let mut tracks = HashMap::new();
    tracks.insert(1, track(1, "A", "Beat", "rock", 2000));

    let like_sig: Vec<PlaySignal> = vec![signal(
        1,
        SignalKind::Like,
        PlayContext::Manual,
        200_000,
        200_000,
    )];
    let like_profile = UserProfile::from_signals(&like_sig, &tracks, &HashMap::new());

    let autoplay_sig: Vec<PlaySignal> = vec![signal(
        1,
        SignalKind::Play,
        PlayContext::Autoplay,
        200_000,
        200_000,
    )];
    let autoplay_profile = UserProfile::from_signals(&autoplay_sig, &tracks, &HashMap::new());

    assert!(
        like_profile.total_weight > autoplay_profile.total_weight,
        "un like manual pesa más que un play de autoplay"
    );
}

#[test]
fn skip_in_autoplay_does_not_count_as_dislike() {
    let mut tracks = HashMap::new();
    tracks.insert(1, track(1, "A", "Beat", "rock", 2000));

    // Saltos de autoplay: no son señal negativa significativa.
    let autoplay_skips: Vec<PlaySignal> = vec![signal(
        1,
        SignalKind::Skip,
        PlayContext::Autoplay,
        5_000,
        200_000,
    )];
    let profile = UserProfile::from_signals(&autoplay_skips, &tracks, &HashMap::new());
    assert_eq!(
        profile.total_skips, 0,
        "skip de autoplay no cuenta como disgusto"
    );
}

#[test]
fn manual_skip_counts_as_negative_signal() {
    let mut tracks = HashMap::new();
    tracks.insert(1, track(1, "A", "Beat", "rock", 2000));

    let manual_skip: Vec<PlaySignal> = vec![signal(
        1,
        SignalKind::Skip,
        PlayContext::Manual,
        5_000,
        200_000,
    )];
    let profile = UserProfile::from_signals(&manual_skip, &tracks, &HashMap::new());
    assert_eq!(profile.total_skips, 1, "skip manual sí es señal negativa");
}

#[test]
fn completed_plays_build_strong_profile() {
    let mut tracks = HashMap::new();
    tracks.insert(1, track(1, "A", "ReplayArtist", "jazz", 2000));

    let sigs = vec![
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
        signal(1, SignalKind::Replay, PlayContext::Manual, 200_000, 200_000),
    ];
    let profile = UserProfile::from_signals(&sigs, &tracks, &HashMap::new());
    assert!(profile
        .favorite_artists
        .contains(&"ReplayArtist".to_string()));
    assert!(profile.favorite_genres.contains(&"jazz".to_string()));
    assert!(profile.total_replays >= 1);
    assert_eq!(profile.total_completions, 2);
}

#[test]
fn weighted_favorites_dominate_scarce_ones() {
    let mut tracks = HashMap::new();
    tracks.insert(1, track(1, "A", "Metal", "metal", 2000));
    tracks.insert(2, track(2, "B", "Classical", "classical", 2000));

    // Escucha MUCHO metal hasta el final y toca clásica una vez (skip autoplay).
    let sigs = vec![
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
        signal(2, SignalKind::Play, PlayContext::Autoplay, 30_000, 200_000),
    ];
    let profile = UserProfile::from_signals(&sigs, &tracks, &HashMap::new());
    assert!(
        profile.favorite_genres.first().map(|s| s.as_str()) == Some("metal"),
        "el género dominante encabeza el perfil"
    );
    // La clásica (1 play de autoplay) queda detrás del metal (5 completas).
    assert!(
        profile
            .favorite_genres
            .iter()
            .position(|g| g == "classical")
            .unwrap_or(usize::MAX)
            > profile
                .favorite_genres
                .iter()
                .position(|g| g == "metal")
                .unwrap_or(usize::MAX),
        "metal domina sobre classical"
    );
}

// ─────────────────────────────────────────────────────── FASE 9: ranking

#[tokio::test]
async fn rank_places_matching_track_above_mismatch() {
    let mut tracks = HashMap::new();
    tracks.insert(1, track(1, "A", "Beat", "rock", 2000));
    tracks.insert(2, track(2, "B", "MetalArtist", "metal", 2005));
    tracks.insert(3, track(3, "C", "Classical", "classical", 1990));

    // Usuario consume rock de "Beat".
    let sigs = vec![
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
    ];
    let profile = UserProfile::from_signals(&sigs, &tracks, &HashMap::new());
    let aps = HashMap::new();
    let hist = vec![crate::infrastructure::storage::TrackListeningStats {
        track_id: 1,
        key: tracks[&1].identifier(),
        artist_name: Some("Beat".to_string()),
        play_count: 3,
        last_played: "2024-06-15 12:00:00".to_string(),
        recently_played: true,
    }];
    let signals_map = HashMap::<i64, TrackSignals>::new();

    let candidates: Vec<Track> = vec![tracks[&2].clone(), tracks[&3].clone()];
    let ranked = rank(&candidates, &profile, &hist, &aps, &signals_map, None).await;
    assert_eq!(ranked.len(), 2);
    // El rockero "B" debería quedar por encima de "C" (clásica).
    assert_eq!(ranked[0].track_id, 2, "rock similar a la preferencia");
    assert!(ranked[0].final_score >= ranked[1].final_score);
}

#[tokio::test]
async fn rank_penalizes_frequently_skipped_track() {
    let mut tracks = HashMap::new();
    tracks.insert(1, track(1, "A", "Beat", "rock", 2000));
    tracks.insert(2, track(2, "B", "Beat2", "rock", 2001));

    // Usuario escucha a "Beat" mucho (afinidad alta por artista), pero el track
    // #1 tiene MUCHOS skips manuales ⇒ queda penalizado frente al #2.
    let sigs = vec![
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
    ];
    let profile = UserProfile::from_signals(&sigs, &tracks, &HashMap::new());
    let aps = HashMap::new();
    let hist = vec![crate::infrastructure::storage::TrackListeningStats {
        track_id: 1,
        key: tracks[&1].identifier(),
        artist_name: Some("Beat".to_string()),
        play_count: 2,
        last_played: "2024-06-15 12:00:00".to_string(),
        recently_played: true,
    }];
    let mut signals_map = HashMap::new();
    // Track 1: 5 plays con 4 negativas. Track 2: sin señales.
    signals_map.insert(
        1,
        TrackSignals {
            plays: 5,
            negative: 4,
        },
    );
    signals_map.insert(
        2,
        TrackSignals {
            plays: 0,
            negative: 0,
        },
    );

    let candidates: Vec<Track> = vec![tracks[&1].clone(), tracks[&2].clone()];
    let ranked = rank(&candidates, &profile, &hist, &aps, &signals_map, None).await;
    assert_eq!(ranked[0].track_id, 2, "el track con skips queda de último");
}

#[tokio::test]
async fn rank_distinguishes_popular_from_recommended() {
    // Un track muy popular pero ajeno al gusto NO supera a uno que encaja.
    let mut tracks = HashMap::new();
    tracks.insert(1, track(1, "GustaI", "Rockero", "rock", 2000));
    tracks.insert(2, track(2, "Popular", "PopStar", "pop", 2020));

    let sigs = vec![
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
    ];
    let profile = UserProfile::from_signals(&sigs, &tracks, &HashMap::new());
    let aps = HashMap::new();
    // El track popular tiene MUCHÍSIMOS plays (popularidad), el recomendado pocos.
    let hist = vec![
        crate::infrastructure::storage::TrackListeningStats {
            track_id: 1,
            key: tracks[&1].identifier(),
            artist_name: Some("Rockero".to_string()),
            play_count: 3,
            last_played: "2024-06-15 12:00:00".to_string(),
            recently_played: true,
        },
        crate::infrastructure::storage::TrackListeningStats {
            track_id: 2,
            key: tracks[&2].identifier(),
            artist_name: Some("PopStar".to_string()),
            play_count: 10_000,
            last_played: "2024-01-01 12:00:00".to_string(),
            recently_played: false,
        },
    ];
    let signals_map = HashMap::new();
    let candidates: Vec<Track> = vec![tracks[&1].clone(), tracks[&2].clone()];
    let ranked = rank(&candidates, &profile, &hist, &aps, &signals_map, None).await;
    assert_eq!(
        ranked[0].track_id, 1,
        "lo que encaja al gusto vence a lo meramente popular"
    );
}

// ─────────────────────────────────────────────────────── FASE 11: señales

#[test]
fn signals_distinguish_like_from_play() {
    let mut tracks = HashMap::new();
    tracks.insert(1, track(1, "A", "Beat", "rock", 2000));

    let like_sig = signal(1, SignalKind::Like, PlayContext::Manual, 200_000, 200_000);
    let play_sig = signal(1, SignalKind::Play, PlayContext::Manual, 120_000, 200_000);
    assert_ne!(like_sig.signal, play_sig.signal, "señales distintas");

    let p_like = UserProfile::from_signals(&[like_sig], &tracks, &HashMap::new());
    let p_play = UserProfile::from_signals(&[play_sig], &tracks, &HashMap::new());
    assert_eq!(p_like.total_likes, 1);
    assert_eq!(p_play.total_likes, 0);
}

#[test]
fn signals_count_replay_as_distinct_signal() {
    let mut tracks = HashMap::new();
    tracks.insert(1, track(1, "A", "Beat", "rock", 2000));
    let replay = signal(1, SignalKind::Replay, PlayContext::Manual, 200_000, 200_000);
    let profile = UserProfile::from_signals(&[replay], &tracks, &HashMap::new());
    assert_eq!(profile.total_replays, 1);
    assert!(profile.tracks_replayed.contains(&1));
}

#[test]
fn playlist_add_is_positive_signal() {
    let mut tracks = HashMap::new();
    tracks.insert(1, track(1, "A", "Beat", "rock", 2000));
    let add = signal(1, SignalKind::PlaylistAdd, PlayContext::Manual, 0, 200_000);
    let profile = UserProfile::from_signals(&[add], &tracks, &HashMap::new());
    // La adición a playlist tiene peso positivo, aunque no hubo reproducción.
    assert!(profile.total_weight > 0.0);
    assert_eq!(profile.total_plays, 0);
}

#[test]
fn rec_click_counts_as_play_intent_but_not_like() {
    let mut tracks = HashMap::new();
    tracks.insert(1, track(1, "A", "Beat", "rock", 2000));
    let click = signal(
        1,
        SignalKind::RecClick,
        PlayContext::Recommendation,
        0,
        200_000,
    );
    let profile = UserProfile::from_signals(&[click], &tracks, &HashMap::new());
    assert_eq!(
        profile.total_plays, 1,
        "un click en recomendación es un intento"
    );
    assert_eq!(profile.total_likes, 0, "pero no es un like");
}

// ─────────────────────────────────────────────────────── FASE 8: acústica en ranking

#[tokio::test]
async fn acoustic_features_steer_ranking_when_metadata_is_tied() {
    // Dos tracks del MISMO artista y género (metadata empatada): se separan por
    // perfil acústico. El usuario prefiere sonido grave y lento (bajo BPM).
    let mut tracks = HashMap::new();
    let t1 = track(1, "A", "Beat", "rock", 2000);
    let t2 = track(2, "B", "Beat", "rock", 2001);
    tracks.insert(1, t1.clone());
    tracks.insert(2, t2.clone());

    // Usuario: escucha t1 (grave, lento) hasta el final muchas veces.
    let sigs = vec![
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
        signal(
            1,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
    ];
    let aps = HashMap::from([
        (1, acoustic(1, 90.0, 0.8, 0.1)),
        (2, acoustic(2, 160.0, 0.1, 0.9)),
    ]);
    let profile = UserProfile::from_signals(&sigs, &tracks, &aps);
    // El perfil acústico del usuario debe reflejar graves/lento.
    assert!(profile.acoustic_profile.bpm_mean < 120.0, "perfil lento");
    assert!(profile.acoustic_profile.bass > 0.5, "perfil grave");

    let hist = vec![crate::infrastructure::storage::TrackListeningStats {
        track_id: 1,
        key: t1.identifier(),
        artist_name: Some("Beat".to_string()),
        play_count: 3,
        last_played: "2024-06-15 12:00:00".to_string(),
        recently_played: true,
    }];
    let signals_map = HashMap::new();
    let candidates: Vec<Track> = vec![t1.clone(), t2.clone()];
    let ranked = rank(&candidates, &profile, &hist, &aps, &signals_map, None).await;
    // Aunque ambos comparten artista/género, el acústico similar vence.
    assert_eq!(
        ranked[0].track_id, 1,
        "el sonido parecido gana sobre metadata empatada"
    );
}

// ───────────────── Rediseño v0.20: semilla armónica/estructural

fn chroma_on(pitches: &[usize]) -> [f32; 12] {
    let mut c = [0.0f32; 12];
    for &p in pitches {
        c[p] = 1.0;
    }
    let t: f32 = c.iter().sum();
    for v in c.iter_mut() {
        *v /= t;
    }
    c
}

fn structure_flat(rms: f32) -> [f32; 32] {
    let mut s = [0.0f32; 32];
    for i in 0..8 {
        s[i * 4 + 1] = rms;
    }
    s
}

#[allow(clippy::too_many_arguments)]
fn harmonic_profile(
    track_id: i64,
    bpm: f32,
    bass: f32,
    high: f32,
    chroma: [f32; 12],
    key: i16,
    rate: f32,
    struct_rms: f32,
) -> TrackAcousticProfile {
    let mut p = acoustic(track_id, bpm, bass, high);
    p.chroma_profile = chroma;
    p.key = key;
    p.key_confidence = 0.9;
    p.chord_change_rate = rate;
    p.structure_profile = structure_flat(struct_rms);
    p
}

#[tokio::test]
async fn seed_similarity_breaks_tie_between_fresh_candidates() {
    // Sin señales de usuario ni historial, meta/affinity/popularity empatan a 0:
    // la similitud con la SEMILLA (la pista que suena) es lo único que decide.
    let mut tracks = HashMap::new();
    tracks.insert(1, track(1, "A", "Seedling", "lofi", 2010));
    tracks.insert(2, track(2, "B", "Distant", "edm", 2022));

    let profile = UserProfile::default();
    let hist = vec![];
    let signals_map = HashMap::new();

    let seed_profile = harmonic_profile(99, 90.0, 0.5, 0.2, chroma_on(&[0, 4, 7]), 0, 1.0, 0.5);
    let match_a = harmonic_profile(1, 90.0, 0.5, 0.2, chroma_on(&[0, 4, 7]), 0, 1.0, 0.5);
    // Disonante: otro tempo, otra tonalidad (tritón), otro ritmo armónico.
    let mismatch_b = harmonic_profile(2, 160.0, 0.1, 0.9, chroma_on(&[1, 5, 8]), 6, 4.0, 0.05);

    let aps = HashMap::from([(1, match_a), (2, mismatch_b)]);
    // Orden inicial: el candidato malo PRIMERO (simula el orden del provider).
    let candidates: Vec<Track> = vec![tracks[&2].clone(), tracks[&1].clone()];
    let ranked = rank(
        &candidates,
        &profile,
        &hist,
        &aps,
        &signals_map,
        Some(&seed_profile),
    )
    .await;

    assert_eq!(
        ranked[0].track_id, 1,
        "el candidato rítmicamente afín a la semilla vence al provider-order"
    );
    assert!(
        ranked[0].components.harmonic > 0.8,
        "componente armónico alto para el afinado: {}",
        ranked[0].components.harmonic
    );
    assert!(ranked[0].final_score > ranked[1].final_score);
}

#[tokio::test]
async fn popular_mismatched_track_does_not_beat_rhythmic_match() {
    // El usuario escucha rock grave/lento. A: pocos plays pero afinado con su
    // gusto Y con la semilla. B: hiper-escuchado (popularidad local máxima)
    // pero rítmicamente disonante. El ritmo debe ganar a la popularidad.
    let mut tracks = HashMap::new();
    tracks.insert(1, track(1, "A", "Beat", "rock", 2000));
    tracks.insert(2, track(2, "B", "PopStar", "pop", 2022));
    tracks.insert(9, track(9, "Seed", "Beat", "rock", 1999));

    let sigs = vec![
        signal(
            9,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
        signal(
            9,
            SignalKind::Completed,
            PlayContext::Manual,
            200_000,
            200_000,
        ),
    ];
    let profile = UserProfile::from_signals(&sigs, &tracks, &HashMap::new());

    let seed_profile = harmonic_profile(9, 95.0, 0.75, 0.15, chroma_on(&[0, 4, 7]), 0, 1.2, 0.5);
    let match_a = harmonic_profile(1, 92.0, 0.7, 0.2, chroma_on(&[0, 4, 7]), 0, 1.0, 0.5);
    let mismatch_b = harmonic_profile(2, 150.0, 0.1, 0.85, chroma_on(&[1, 5, 8]), 6, 3.5, 0.05);

    let aps = HashMap::from([(1, match_a), (2, mismatch_b), (9, seed_profile.clone())]);
    let hist = vec![
        crate::infrastructure::storage::TrackListeningStats {
            track_id: 1,
            key: tracks[&1].identifier(),
            artist_name: Some("Beat".to_string()),
            play_count: 1,
            last_played: "2024-06-15 12:00:00".to_string(),
            recently_played: false,
        },
        crate::infrastructure::storage::TrackListeningStats {
            track_id: 2,
            key: tracks[&2].identifier(),
            artist_name: Some("PopStar".to_string()),
            play_count: 5_000,
            last_played: "2024-01-01 12:00:00".to_string(),
            recently_played: false,
        },
    ];
    let signals_map = HashMap::new();
    let candidates: Vec<Track> = vec![tracks[&2].clone(), tracks[&1].clone()];
    let ranked = rank(
        &candidates,
        &profile,
        &hist,
        &aps,
        &signals_map,
        Some(&seed_profile),
    )
    .await;

    assert_eq!(
        ranked[0].track_id, 1,
        "el candidato rítmicamente afín vence al hiper-popular disonante"
    );
    let pop_component = ranked[1].components.popularity;
    assert!(
        pop_component > ranked[0].components.popularity,
        "el popular SÍ tiene el componente de popularidad más alto (pero pierde)"
    );
}

#[tokio::test]
async fn without_seed_and_profiles_order_is_provider_stable() {
    // Lista fría: sin perfiles acústicos, sin señales, sin historial ⇒ todos
    // empatan a 0 y el orden del provider se conserva (sort estable).
    let mut tracks = HashMap::new();
    tracks.insert(1, track(1, "A", "X", "rock", 2000));
    tracks.insert(2, track(2, "B", "Y", "pop", 2001));
    tracks.insert(3, track(3, "C", "Z", "jazz", 2002));

    let candidates: Vec<Track> = vec![tracks[&1].clone(), tracks[&2].clone(), tracks[&3].clone()];
    let ranked = rank(
        &candidates,
        &UserProfile::default(),
        &[],
        &HashMap::new(),
        &HashMap::new(),
        None,
    )
    .await;
    let ids: Vec<i64> = ranked.iter().map(|s| s.track_id).collect();
    assert_eq!(
        ids,
        vec![1, 2, 3],
        "empate total ⇒ orden del provider intacto"
    );
}
