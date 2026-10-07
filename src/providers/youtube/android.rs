//! JNI adapter for the opt-in Android YouTube catalog and stream resolver.
//!
//! This module is compiled only by `android-youtube`. It composes the same
//! provider-neutral catalog/stream registries used by desktop; Android UI and
//! the audio engine only exchange serialized domain tracks and remote sources.

use std::path::PathBuf;
use std::sync::OnceLock;

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;
use serde_json::{json, Value};
use tokio::runtime::Runtime;

use crate::api::ComposedMedia;
use crate::catalog::CatalogError;
use crate::domain::{source::Source, track::Track};
use crate::infrastructure::config::FeatureFlags;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static MEDIA: OnceLock<ComposedMedia> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("tunefold-youtube")
            .build()
            .expect("Android provider runtime initialization")
    })
}

fn read_string(env: &mut JNIEnv<'_>, value: &JString<'_>) -> Result<String, String> {
    env.get_string(value)
        .map(|s| s.to_string_lossy().into_owned())
        .map_err(|error| error.to_string())
}

fn return_json(env: &mut JNIEnv<'_>, value: Value) -> jstring {
    let text = value.to_string();
    env.new_string(text)
        .map(|value| value.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

fn failure(category: &str, message: impl Into<String>) -> Value {
    json!({"ok": false, "category": category, "error": message.into()})
}

fn encode_tracks(tracks: Vec<Track>) -> Value {
    json!({"ok": true, "tracks": tracks})
}

fn catalog_error_category(error: &CatalogError) -> &'static str {
    match error {
        CatalogError::Http(_) => "network_error",
        CatalogError::Api { status: 429, .. } => "provider_unavailable",
        CatalogError::Api { .. } => "metadata_unavailable",
        CatalogError::NotFound { .. } => "metadata_unavailable",
        CatalogError::Invalid { .. } => "metadata_unavailable",
        CatalogError::Config(_) => "provider_unavailable",
        CatalogError::Other(_) => "metadata_unavailable",
    }
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_initializeYoutube(
    mut env: JNIEnv,
    _class: JClass,
    cache_dir: JString,
) -> jboolean {
    let path = match read_string(&mut env, &cache_dir) {
        Ok(path) if !path.trim().is_empty() => PathBuf::from(path),
        _ => return JNI_FALSE,
    };
    let _ = runtime();
    if MEDIA.get().is_none() {
        let flags = FeatureFlags {
            youtube_provider: true,
            ..FeatureFlags::default()
        };
        let _ = MEDIA.set(crate::api::compose_media_in_memory(&flags, Some(path)));
    }
    // Touch the runtime here so an initialization error is returned before UI
    // commands begin. Provider construction itself does no network request.
    JNI_TRUE
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_searchYoutube(
    mut env: JNIEnv,
    _class: JClass,
    query: JString,
    limit: jint,
) -> jstring {
    let query = match read_string(&mut env, &query) {
        Ok(query) if !query.trim().is_empty() => query,
        _ => return return_json(&mut env, failure("search_failed", "Search query is empty")),
    };
    let result = (|| {
        let media = MEDIA.get().ok_or_else(|| {
            (
                "provider_unavailable",
                "provider is not initialized".to_string(),
            )
        })?;
        let provider = media.catalog.get(Source::YouTube).ok_or_else(|| {
            (
                "provider_unavailable",
                "YouTube catalog is disabled".to_string(),
            )
        })?;
        runtime().block_on(async {
            provider
                .search_tracks(&query, limit.clamp(1, 25) as u32)
                .await
                .map_err(|error| (catalog_error_category(&error), error.to_string()))
        })
    })();
    match result {
        Ok(tracks) => return_json(&mut env, encode_tracks(tracks)),
        Err((category, message)) => return_json(&mut env, failure(category, message)),
    }
}

/// Tracks related to a video, for Home and autoplay.
///
/// Uses the provider's own `related` list; the Android layer applies its own
/// deduplication (current track, queue, liked and recently played).
#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_relatedYoutube(
    mut env: JNIEnv,
    _class: JClass,
    video_id: JString,
    _limit: jint,
) -> jstring {
    // The limit is accepted for symmetry with the other JNI entry points, but the
    // Android layer owns selection and dedup, so the provider returns its own
    // list untouched.
    let video_id = match read_string(&mut env, &video_id) {
        Ok(id) if !id.trim().is_empty() => id,
        _ => return return_json(&mut env, failure("invalid_request", "video id is empty")),
    };
    let result = (|| {
        let media = MEDIA.get().ok_or_else(|| {
            (
                "provider_unavailable",
                "provider is not initialized".to_string(),
            )
        })?;
        let provider = media.catalog.get(Source::YouTube).ok_or_else(|| {
            (
                "provider_unavailable",
                "YouTube catalog is disabled".to_string(),
            )
        })?;
        runtime().block_on(async {
            provider
                .related(&video_id)
                .await
                .map_err(|error| (catalog_error_category(&error), error.to_string()))
        })
    })();
    match result {
        Ok(tracks) => return_json(&mut env, encode_tracks(tracks)),
        Err((category, message)) => return_json(&mut env, failure(category, message)),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_resolveYoutubeUrl(
    mut env: JNIEnv,
    _class: JClass,
    url: JString,
) -> jstring {
    let url = match read_string(&mut env, &url) {
        Ok(url) => url,
        Err(error) => return return_json(&mut env, failure("invalid_url", error)),
    };
    let video_id = match super::link::parse_link(&url) {
        super::link::LinkKind::VideoId(id) => id,
        super::link::LinkKind::PlaylistId(_) => {
            return return_json(
                &mut env,
                failure(
                    "unsupported_media",
                    "Playlist links are not playable tracks",
                ),
            )
        }
        super::link::LinkKind::Invalid => {
            return return_json(&mut env, failure("invalid_url", "Unsupported YouTube URL"))
        }
    };
    let result = (|| {
        let media = MEDIA.get().ok_or_else(|| {
            (
                "provider_unavailable",
                "provider is not initialized".to_string(),
            )
        })?;
        let provider = media.catalog.get(Source::YouTube).ok_or_else(|| {
            (
                "provider_unavailable",
                "YouTube catalog is disabled".to_string(),
            )
        })?;
        runtime().block_on(async {
            provider
                .get_track(&video_id)
                .await
                .map_err(|error| (catalog_error_category(&error), error.to_string()))
        })
    })();
    match result {
        Ok(track) => return_json(&mut env, encode_tracks(vec![track])),
        Err((category, message)) => return_json(&mut env, failure(category, message)),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_resolveYoutubeSource(
    mut env: JNIEnv,
    _class: JClass,
    track_json: JString,
) -> jstring {
    let raw = match read_string(&mut env, &track_json) {
        Ok(raw) => raw,
        Err(error) => return return_json(&mut env, failure("source_resolution_failed", error)),
    };
    let track: Track = match serde_json::from_str::<Track>(&raw) {
        Ok(track) if track.source == Source::YouTube && track.external_id.is_some() => track,
        _ => {
            return return_json(
                &mut env,
                failure("invalid_url", "Track has no valid YouTube identity"),
            )
        }
    };
    let result = (|| {
        let media = MEDIA.get().ok_or_else(|| {
            (
                "provider_unavailable",
                "provider is not initialized".to_string(),
            )
        })?;
        let before = media.stream_resolver.cache_stats();
        let source = runtime().block_on(async {
            media
                .stream_resolver
                .resolve(&track)
                .await
                .map_err(|error| {
                    let category = match error.root.category {
                        crate::media::FailureCategory::Unsupported => "unsupported_media",
                        crate::media::FailureCategory::NetworkFailure => "network_error",
                        crate::media::FailureCategory::AuthenticationRequired => {
                            "authentication_required"
                        }
                        crate::media::FailureCategory::ProviderUnavailable => {
                            "provider_unavailable"
                        }
                        crate::media::FailureCategory::Timeout => "network_error",
                        _ => "source_resolution_failed",
                    };
                    (category, error.to_string())
                })
        })?;
        let after = media.stream_resolver.cache_stats();
        Ok((source, after.0 > before.0, after.1 > before.1))
    })();
    match result {
        Ok((source, cache_hit, cache_miss)) => return_json(
            &mut env,
            json!({
                "ok": true,
                "source": source,
                "source_cache": if cache_hit { "hit" } else if cache_miss { "miss" } else { "none" }
            }),
        ),
        Err((category, message)) => return_json(&mut env, failure(category, message)),
    }
}
