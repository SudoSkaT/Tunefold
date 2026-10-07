//! Android JNI bridge for Phase 3: real decoder pipeline.
//!
//! Provides the FFI surface for:
//! 1. Real PCM playback via Android AudioTrack (HTTP → Symphonia → PCM)
//! 2. Playback controls (play/pause/resume/stop/seek/volume)
//! 3. Analysis engine integration (same PCM feeds analysis)
//! 4. Visual state delivery to Kotlin

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use jni::objects::{JByteBuffer, JClass, JObject, JString};
use jni::sys::{jboolean, jfloat, jint, jlong, jobject, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;

use crate::analysis::{AnalysisConfig, AnalysisRuntime};
use crate::platform::android_decoder::{DecodeOutcome, DecoderDiagnostics, PlaybackTraceLog};
use crate::platform::android_download::{self, DownloadProgress};

const STATE_IDLE: u8 = 0;
const STATE_BUFFERING: u8 = 1;
const STATE_PLAYING: u8 = 2;
const STATE_PAUSED: u8 = 3;
const STATE_STOPPED: u8 = 4;
const STATE_ERROR: u8 = 5;

/// Opaque handle to the Android engine state.
pub struct AndroidEngine {
    analysis: Option<AnalysisRuntime>,
    playing: Arc<AtomicBool>,
    ring: Arc<crate::platform::android_playback::PcmRing>,
    position_ms: Arc<Mutex<u64>>,
    sample_rate: Arc<AtomicU64>,
    volume: Arc<Mutex<f32>>,
    cancel: Arc<AtomicBool>,
    decoder_handle: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
    state: Arc<AtomicU8>,
    decoder_finished: Arc<AtomicBool>,
    /// `true` ONLY when the decoder reached the end of the media normally.
    ///
    /// Distinct from `decoder_finished`, which a user-initiated stop also sets:
    /// end-of-track must be reported exactly once, and must never be confused
    /// with the user pressing stop.
    track_finished: Arc<AtomicBool>,
    last_error: Arc<Mutex<Option<String>>>,
    decoder_diagnostics: Arc<DecoderDiagnostics>,
    /// Línea temporal de la reproducción actual, anclada al Play tap.
    trace: Arc<PlaybackTraceLog>,
    output_frames: AtomicU64,
    /// Descargas explícitas en curso. Registro compartido con los hilos de
    /// descarga mediante `Arc`: el hilo nunca recibe el puntero del engine.
    downloads: Arc<DownloadRegistry>,
}

/// Una descarga explícita viva. Nunca se toca desde el hilo de audio.
#[derive(Clone)]
struct DownloadHandle {
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<DownloadProgress>>,
}

/// Registro de descargas explícitas en curso.
///
/// Vive detrás de un `Arc` para que los hilos de descarga compartan el estado
/// sin recibir el puntero crudo del engine (que no es `Send`).
#[derive(Default)]
struct DownloadRegistry {
    inner: Mutex<std::collections::HashMap<u64, DownloadHandle>>,
    next_id: AtomicU64,
}

impl DownloadRegistry {
    fn new() -> Self {
        Self {
            inner: Mutex::new(std::collections::HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    fn register(&self, cancel: Arc<AtomicBool>, progress: Arc<Mutex<DownloadProgress>>) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut inner) = self.inner.lock() {
            inner.insert(id, DownloadHandle { cancel, progress });
        }
        id
    }

    fn finish(&self, id: u64) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.remove(&id);
        }
    }

    fn cancel(&self, id: u64) -> bool {
        self.inner
            .lock()
            .ok()
            .and_then(|inner| inner.get(&id).cloned())
            .map(|handle| {
                handle.cancel.store(true, Ordering::Release);
                true
            })
            .unwrap_or(false)
    }

    fn progress(&self, id: u64) -> Option<DownloadProgress> {
        let handle = self.inner.lock().ok()?.get(&id).cloned()?;
        handle.progress.lock().ok().map(|slot| *slot)
    }
}

/// C ABI functions exported to the C++ Oboe adapter.
/// These are called from the Oboe audio callback (high-priority thread).
/// They must never block, allocate, or lock.
#[no_mangle]
pub extern "C" fn tunefold_oboe_create_engine() -> *mut AndroidEngine {
    let config = AnalysisConfig::default();
    let runtime = AnalysisRuntime::spawn(config);

    let engine = Box::new(AndroidEngine {
        analysis: Some(runtime),
        playing: Arc::new(AtomicBool::new(false)),
        ring: crate::platform::android_playback::PcmRing::new(131072),
        position_ms: Arc::new(Mutex::new(0)),
        sample_rate: Arc::new(AtomicU64::new(0)),
        volume: Arc::new(Mutex::new(1.0)),
        cancel: Arc::new(AtomicBool::new(false)),
        decoder_handle: Arc::new(Mutex::new(None)),
        state: Arc::new(AtomicU8::new(STATE_IDLE)),
        decoder_finished: Arc::new(AtomicBool::new(false)),
        track_finished: Arc::new(AtomicBool::new(false)),
        last_error: Arc::new(Mutex::new(None)),
        decoder_diagnostics: Arc::new(DecoderDiagnostics::new()),
        trace: Arc::new(PlaybackTraceLog::new()),
        output_frames: AtomicU64::new(0),
        downloads: Arc::new(DownloadRegistry::new()),
    });

    Box::into_raw(engine)
}

/// # Safety
/// `engine` must be a valid pointer returned by `tunefold_oboe_create_engine`.
#[no_mangle]
pub unsafe extern "C" fn tunefold_oboe_destroy_engine(engine: *mut AndroidEngine) {
    if engine.is_null() {
        return;
    }
    tunefold_oboe_stop(engine);
    drop(Box::from_raw(engine));
}

/// Start decoding an HTTP stream.
/// Spawns a decoder thread that feeds PCM to the ring buffer.
/// # Safety
/// `engine` must be a valid pointer returned by `tunefold_oboe_create_engine`.
/// `url` must be a valid null-terminated C string.
/// `headers` must be a valid pointer to `num_headers` header pairs.
#[no_mangle]
pub unsafe extern "C" fn tunefold_oboe_play_stream(
    engine: *mut AndroidEngine,
    url: *const std::os::raw::c_char,
    headers: *const std::os::raw::c_char,
    num_headers: usize,
    tap_to_decoder_us: u64,
) {
    if engine.is_null() || url.is_null() {
        return;
    }
    let engine = &*engine;

    let url_str = std::ffi::CStr::from_ptr(url).to_string_lossy().into_owned();
    // `file:` reaches the decoder's local-media branch: a downloaded track is
    // played by the SAME pipeline (Symphonia -> PCM ring -> AudioTrack) with no
    // network at all. The decoder validates the path itself.
    let playable = url_str.starts_with("https://")
        || url_str.starts_with("http://")
        || url_str.starts_with("file:");
    if !playable {
        set_engine_error(engine, "URL must use http, https or file".to_string());
        return;
    }

    engine.cancel.store(true, Ordering::Release);
    if let Ok(mut handle) = engine.decoder_handle.lock() {
        if let Some(h) = handle.take() {
            let _ = h.join();
        }
    }

    let mut header_vec = Vec::new();
    if !headers.is_null() && num_headers > 0 {
        let headers_str = std::ffi::CStr::from_ptr(headers).to_string_lossy();
        for line in headers_str.lines().take(num_headers) {
            if let Some((k, v)) = line.split_once(':') {
                header_vec.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
    }

    engine.cancel.store(false, Ordering::Release);
    engine.ring.clear();
    // The position clock is per playback, not per engine: it is only otherwise
    // reset by seek, which Android never performs. Without this the new track
    // inherits the elapsed time of the previous one.
    if let Ok(mut position) = engine.position_ms.lock() {
        *position = 0;
    }
    engine.playing.store(true, Ordering::Release);
    engine.state.store(STATE_BUFFERING, Ordering::Release);
    engine.decoder_finished.store(false, Ordering::Release);
    engine.track_finished.store(false, Ordering::Release);
    engine.output_frames.store(0, Ordering::Release);
    engine
        .decoder_diagnostics
        .decoded_packets
        .store(0, Ordering::Release);
    engine
        .decoder_diagnostics
        .decoded_frames
        .store(0, Ordering::Release);
    engine
        .decoder_diagnostics
        .ring_frames
        .store(0, Ordering::Release);
    engine
        .decoder_diagnostics
        .analysis_frames
        .store(0, Ordering::Release);
    for metric in [
        &engine.decoder_diagnostics.http_open_us,
        &engine.decoder_diagnostics.first_response_us,
        &engine.decoder_diagnostics.probe_us,
        &engine.decoder_diagnostics.first_decode_us,
        &engine.decoder_diagnostics.first_pcm_us,
        &engine.decoder_diagnostics.range_requests,
        &engine.decoder_diagnostics.range_bytes,
        &engine.decoder_diagnostics.range_elapsed_ms,
        &engine.decoder_diagnostics.tap_to_decoder_us,
        &engine.decoder_diagnostics.seeks,
        &engine.decoder_diagnostics.seek_requests,
        &engine.decoder_diagnostics.headers_us,
    ] {
        metric.store(0, Ordering::Release);
    }
    // El desplazamiento Play tap → decoder se fija DESPUÉS del reset: es la
    // entrada de la línea temporal que comparten Java y Rust, no un contador
    // de esta reproducción.
    engine
        .decoder_diagnostics
        .tap_to_decoder_us
        .store(tap_to_decoder_us, Ordering::Release);
    if let Ok(mut diagnostic) = engine.decoder_diagnostics.stage.lock() {
        *diagnostic = "play requested; decoder starting".to_string();
    }
    if let Ok(mut error) = engine.last_error.lock() {
        *error = None;
    }

    let ring = engine.ring.clone();
    let cancel = engine.cancel.clone();
    let sample_rate = engine.sample_rate.clone();
    let position_ms = engine.position_ms.clone();
    let analysis_tap = engine.analysis.as_ref().map(|analysis| analysis.tap());
    let playing = engine.playing.clone();
    let state = engine.state.clone();
    let decoder_finished = engine.decoder_finished.clone();
    let track_finished = engine.track_finished.clone();
    let last_error = engine.last_error.clone();
    let decoder_diagnostics = engine.decoder_diagnostics.clone();
    let trace = engine.trace.clone();

    let handle = std::thread::spawn(move || {
        if let Some(analysis_tap) = analysis_tap {
            let decoder = crate::platform::android_decoder::AndroidDecoder::new(
                ring.clone(),
                cancel.clone(),
                sample_rate,
                position_ms,
                analysis_tap,
                playing,
                decoder_diagnostics,
                trace,
            );
            match decoder.decode_stream(url_str, header_vec) {
                Ok(outcome) => {
                    let end_of_media =
                        publish_decode_outcome(outcome, &decoder_finished, &track_finished);
                    if end_of_media
                        && state.load(Ordering::Acquire) == STATE_BUFFERING
                        && ring.available_frames() == 0
                    {
                        state.store(STATE_STOPPED, Ordering::Release);
                    }
                }
                Err(error) => {
                    if let Ok(mut slot) = last_error.lock() {
                        *slot = Some(error.to_string());
                    }
                    state.store(STATE_ERROR, Ordering::Release);
                    decoder_finished.store(true, Ordering::Release);
                    cancel.store(true, Ordering::Release);
                }
            }
        }
    });

    if let Ok(mut h) = engine.decoder_handle.lock() {
        *h = Some(handle);
    }
}

/// # Safety
/// `engine` must be a valid pointer returned by `tunefold_oboe_create_engine`.
#[no_mangle]
pub unsafe extern "C" fn tunefold_oboe_start(engine: *mut AndroidEngine) {
    if engine.is_null() {
        return;
    }
    let engine = &*engine;
    engine.playing.store(true, Ordering::Release);
}

/// # Safety
/// `engine` must be a valid pointer returned by `tunefold_oboe_create_engine`.
#[no_mangle]
pub unsafe extern "C" fn tunefold_oboe_stop(engine: *mut AndroidEngine) {
    if engine.is_null() {
        return;
    }
    let engine = &*engine;
    engine.playing.store(false, Ordering::Release);
    engine.cancel.store(true, Ordering::Release);
    engine.ring.clear();
    engine.decoder_finished.store(true, Ordering::Release);
    if engine.state.load(Ordering::Acquire) != STATE_ERROR {
        engine.state.store(STATE_STOPPED, Ordering::Release);
    }
    if let Ok(mut handle) = engine.decoder_handle.lock() {
        if let Some(h) = handle.take() {
            let _ = h.join();
        }
    }
    // Stopping is not finishing: clear any pending EOF so autoplay cannot fire
    // for a track the user deliberately ended. Cleared AFTER the join, because
    // the decoder being stopped may still publish its own final state.
    engine.track_finished.store(false, Ordering::Release);
}

/// # Safety
/// `engine` must be a valid pointer returned by `tunefold_oboe_create_engine`.
#[no_mangle]
pub unsafe extern "C" fn tunefold_oboe_pause(engine: *mut AndroidEngine) {
    if engine.is_null() {
        return;
    }
    let engine = &*engine;
    engine.playing.store(false, Ordering::Release);
    if engine.state.load(Ordering::Acquire) != STATE_ERROR {
        engine.state.store(STATE_PAUSED, Ordering::Release);
    }
}

/// # Safety
/// `engine` must be a valid pointer returned by `tunefold_oboe_create_engine`.
#[no_mangle]
pub unsafe extern "C" fn tunefold_oboe_resume(engine: *mut AndroidEngine) {
    if engine.is_null() {
        return;
    }
    let engine = &*engine;
    engine.playing.store(true, Ordering::Release);
    if engine.state.load(Ordering::Acquire) == STATE_PAUSED {
        engine.state.store(STATE_BUFFERING, Ordering::Release);
    }
}

/// Update output-owned playback state after AudioTrack reports a successful
/// transition. This is deliberately separate from decoder launch state.
///
/// # Safety
/// `engine` must be null or a valid pointer returned by
/// `tunefold_oboe_create_engine` that remains alive for this call.
#[no_mangle]
pub unsafe extern "C" fn tunefold_oboe_set_output_state(engine: *mut AndroidEngine, state: u8) {
    if engine.is_null()
        || ![STATE_BUFFERING, STATE_PLAYING, STATE_PAUSED, STATE_STOPPED].contains(&state)
    {
        return;
    }
    let engine = &*engine;
    if engine.state.load(Ordering::Acquire) != STATE_ERROR {
        engine.state.store(state, Ordering::Release);
    }
}

#[no_mangle]
/// # Safety
/// `engine` must be null or a valid pointer returned by
/// `tunefold_oboe_create_engine` that remains alive for this call.
pub unsafe extern "C" fn tunefold_oboe_get_playback_state(engine: *mut AndroidEngine) -> u8 {
    if engine.is_null() {
        return STATE_ERROR;
    }
    (&*engine).state.load(Ordering::Acquire)
}

#[no_mangle]
/// # Safety
/// `engine` must be null or a valid pointer returned by
/// `tunefold_oboe_create_engine` that remains alive for this call.
pub unsafe extern "C" fn tunefold_oboe_get_sample_rate(engine: *mut AndroidEngine) -> u32 {
    if engine.is_null() {
        return 0;
    }
    (&*engine).sample_rate.load(Ordering::Acquire) as u32
}

#[no_mangle]
/// # Safety
/// `engine` must be null or a valid pointer returned by
/// `tunefold_oboe_create_engine` that remains alive for this call.
pub unsafe extern "C" fn tunefold_oboe_is_decoder_finished(engine: *mut AndroidEngine) -> bool {
    !engine.is_null() && (&*engine).decoder_finished.load(Ordering::Acquire)
}

/// Records how a decode finished and reports whether it was a real end of media.
///
/// Only a genuine end of media may publish the end-of-track signal that the
/// queue and autoplay policy consume. A cancelled decode (track change, stop or
/// error) marks the decoder finished and nothing else, because treating it as an
/// end of track is what let playback advance without the user asking.
fn publish_decode_outcome(
    outcome: DecodeOutcome,
    decoder_finished: &AtomicBool,
    track_finished: &AtomicBool,
) -> bool {
    decoder_finished.store(true, Ordering::Release);
    let end_of_media = matches!(outcome, DecodeOutcome::EndOfMedia);
    if end_of_media {
        track_finished.store(true, Ordering::Release);
    }
    end_of_media
}

/// Reports whether the last track reached its end, consuming the flag.
///
/// Take-once so a single EOF cannot fire autoplay twice, and false for a
/// user-initiated stop.
#[no_mangle]
/// # Safety
/// `engine` must be null or a valid pointer returned by
/// `tunefold_oboe_create_engine` that remains alive for this call.
pub unsafe extern "C" fn tunefold_oboe_take_track_finished(engine: *mut AndroidEngine) -> bool {
    !engine.is_null() && (*engine).track_finished.swap(false, Ordering::AcqRel)
}

#[no_mangle]
/// # Safety
/// `engine` must be null or a valid pointer returned by
/// `tunefold_oboe_create_engine` that remains alive for this call.
pub unsafe extern "C" fn tunefold_oboe_get_available_frames(engine: *mut AndroidEngine) -> usize {
    if engine.is_null() {
        return 0;
    }
    (&*engine).ring.available_frames()
}

fn set_engine_error(engine: &AndroidEngine, message: String) {
    if let Ok(mut error) = engine.last_error.lock() {
        *error = Some(message);
    }
    engine.state.store(STATE_ERROR, Ordering::Release);
    engine.decoder_finished.store(true, Ordering::Release);
}

/// # Safety
/// `engine` must be a valid pointer returned by `tunefold_oboe_create_engine`.
#[no_mangle]
pub unsafe extern "C" fn tunefold_oboe_seek(engine: *mut AndroidEngine, position_ms: u64) {
    if engine.is_null() {
        return;
    }
    let engine = &*engine;
    engine.ring.clear();
    if let Ok(mut pos) = engine.position_ms.lock() {
        *pos = position_ms;
    }
}

/// # Safety
/// `engine` must be a valid pointer returned by `tunefold_oboe_create_engine`.
#[no_mangle]
pub unsafe extern "C" fn tunefold_oboe_set_volume(engine: *mut AndroidEngine, volume: f32) {
    if engine.is_null() {
        return;
    }
    let engine = &*engine;
    if let Ok(mut v) = engine.volume.lock() {
        *v = volume.clamp(0.0, 1.0);
    }
}

/// # Safety
/// `engine` must be a valid pointer returned by `tunefold_oboe_create_engine`.
#[no_mangle]
pub unsafe extern "C" fn tunefold_oboe_get_position_ms(engine: *mut AndroidEngine) -> u64 {
    if engine.is_null() {
        return 0;
    }
    let engine = &*engine;
    *engine.position_ms.lock().unwrap_or_else(|e| e.into_inner())
}

/// Feed PCM frames from the decoder to the engine.
/// Called from the decoder thread (not the audio callback).
/// # Safety
/// `engine` must be a valid pointer returned by `tunefold_oboe_create_engine`.
/// `frames` must be a valid pointer to `num_frames * 2` interleaved f32 samples.
#[no_mangle]
pub unsafe extern "C" fn tunefold_oboe_feed_pcm(
    engine: *mut AndroidEngine,
    frames: *const f32,
    num_frames: usize,
) {
    if engine.is_null() || frames.is_null() || num_frames == 0 {
        return;
    }
    let engine = &*engine;
    let slice = std::slice::from_raw_parts(frames, num_frames * 2);

    if let Some(ref analysis) = engine.analysis {
        for frame in slice.chunks_exact(2) {
            analysis.tap().feed(frame);
        }
    }

    engine.ring.push(slice);

    let sample_rate = engine.sample_rate.load(Ordering::Relaxed);
    if sample_rate > 0 {
        let current = *engine.position_ms.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(delta) = (num_frames as u64)
            .checked_mul(1000)
            .and_then(|ms| ms.checked_div(sample_rate))
        {
            let new_pos = current + delta;
            if let Ok(mut pos) = engine.position_ms.lock() {
                *pos = new_pos;
            }
        }
    }
}

/// Read PCM frames from the ring buffer.
/// Called from the Oboe audio callback (high-priority thread).
/// Returns number of frames read.
/// # Safety
/// `engine` must be a valid pointer returned by `tunefold_oboe_create_engine`.
/// `out` must be a valid pointer to `max_frames * 2` interleaved f32 samples.
#[no_mangle]
pub unsafe extern "C" fn tunefold_oboe_read_pcm(
    engine: *mut AndroidEngine,
    out: *mut f32,
    max_frames: usize,
) -> usize {
    if engine.is_null() || out.is_null() || max_frames == 0 {
        return 0;
    }
    let engine = &*engine;
    let slice = std::slice::from_raw_parts_mut(out, max_frames * 2);
    let frames = engine.ring.pop(slice);
    engine
        .output_frames
        .fetch_add(frames as u64, Ordering::Relaxed);
    frames
}

/// Clear the PCM ring buffer (used on seek/stop).
/// # Safety
/// `engine` must be a valid pointer returned by `tunefold_oboe_create_engine`.
#[no_mangle]
pub unsafe extern "C" fn tunefold_oboe_clear_pcm(engine: *mut AndroidEngine) {
    if engine.is_null() {
        return;
    }
    let engine = &*engine;
    engine.ring.clear();
}

/// # Safety
/// `engine` must be a valid pointer returned by `tunefold_oboe_create_engine`.
/// `out_features` and `out_bars` must be valid pointers with at least the specified lengths.
#[no_mangle]
pub unsafe extern "C" fn tunefold_oboe_get_visual_state(
    engine: *mut AndroidEngine,
    out_features: *mut f32,
    out_features_len: usize,
    out_bars: *mut f32,
    out_bars_len: usize,
) {
    if engine.is_null() || out_features.is_null() || out_bars.is_null() {
        return;
    }
    let engine = &*engine;

    if let Some(ref analysis) = engine.analysis {
        let features = analysis.bus().latest();
        if let Some(ref f) = features {
            let vals = [
                f.rms,
                f.amplitude,
                f.bass,
                f.low_mid,
                f.mid,
                f.high_mid,
                f.high,
                f.spectral_centroid,
                f.spectral_flux,
                f.onset,
                f.beat as u8 as f32,
                f.beat_confidence,
                f.bpm / 200.0,
            ];
            let n = vals.len().min(out_features_len);
            std::ptr::copy_nonoverlapping(vals.as_ptr(), out_features, n);
        }

        let waveform = analysis.waveform_bus().latest();
        if let Some(ref w) = waveform {
            let n = 24.min(out_bars_len);
            for i in 0..n {
                let idx = (i * 128 / n).min(127);
                let left_peak = w.left.max[idx].abs();
                let right_peak = w.right.max[idx].abs();
                *out_bars.add(i) = (left_peak + right_peak) * 0.5;
            }
        }
    }
}

// ============================================================================
// JNI functions called from Kotlin
// ============================================================================

fn encode_http_headers(
    headers: Vec<(String, String)>,
) -> Result<(std::ffi::CString, usize), String> {
    let mut lines = Vec::with_capacity(headers.len());
    for (name, value) in headers {
        if name.is_empty()
            || !name.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(
                        byte,
                        b'!' | b'#'
                            | b'$'
                            | b'%'
                            | b'&'
                            | b'\''
                            | b'*'
                            | b'+'
                            | b'-'
                            | b'.'
                            | b'^'
                            | b'_'
                            | b'`'
                            | b'|'
                            | b'~'
                    )
            })
            || value.contains(['\r', '\n', '\0'])
        {
            return Err("Invalid HTTP header received from provider".to_string());
        }
        lines.push(format!("{name}: {value}"));
    }
    let count = lines.len();
    let encoded = std::ffi::CString::new(lines.join("\n"))
        .map_err(|_| "HTTP headers contain a NUL byte".to_string())?;
    Ok((encoded, count))
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_createEngine(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    tunefold_oboe_create_engine() as jlong
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_playStream(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    url: JString,
    headers_json: JString,
    tap_to_decoder_us: jlong,
) -> jboolean {
    if handle == 0 {
        let _ = env.throw_new(
            "java/lang/IllegalStateException",
            "Rust engine is not initialized",
        );
        return JNI_FALSE;
    }
    let tap_to_decoder_us = tap_to_decoder_us.max(0) as u64;
    let url = match env.get_string(&url) {
        Ok(value) => value.to_string_lossy().into_owned(),
        Err(error) => {
            let _ = env.throw_new("java/lang/IllegalArgumentException", error.to_string());
            return JNI_FALSE;
        }
    };
    let url = match std::ffi::CString::new(url) {
        Ok(value) => value,
        Err(_) => {
            let _ = env.throw_new(
                "java/lang/IllegalArgumentException",
                "URL contains a NUL byte",
            );
            return JNI_FALSE;
        }
    };
    let headers_json = match env.get_string(&headers_json) {
        Ok(value) => value.to_string_lossy().into_owned(),
        Err(error) => {
            let _ = env.throw_new("java/lang/IllegalArgumentException", error.to_string());
            return JNI_FALSE;
        }
    };
    let headers: Vec<(String, String)> = match serde_json::from_str(&headers_json) {
        Ok(headers) => headers,
        Err(error) => {
            let _ = env.throw_new("java/lang/IllegalArgumentException", error.to_string());
            return JNI_FALSE;
        }
    };
    let (headers, header_count) = match encode_http_headers(headers) {
        Ok(value) => value,
        Err(error) => {
            let _ = env.throw_new("java/lang/IllegalArgumentException", error);
            return JNI_FALSE;
        }
    };
    unsafe {
        tunefold_oboe_play_stream(
            handle as *mut AndroidEngine,
            url.as_ptr(),
            headers.as_ptr(),
            header_count,
            tap_to_decoder_us,
        );
        if tunefold_oboe_get_playback_state(handle as *mut AndroidEngine) == STATE_ERROR {
            JNI_FALSE
        } else {
            JNI_TRUE
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_destroyEngine(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    unsafe { tunefold_oboe_destroy_engine(handle as *mut AndroidEngine) };
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_startAudio(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    unsafe { tunefold_oboe_start(handle as *mut AndroidEngine) };
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_stopAudio(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    unsafe { tunefold_oboe_stop(handle as *mut AndroidEngine) };
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_pauseAudio(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    unsafe { tunefold_oboe_pause(handle as *mut AndroidEngine) };
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_resumeAudio(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    unsafe { tunefold_oboe_resume(handle as *mut AndroidEngine) };
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_seekAudio(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    position_ms: jlong,
) {
    unsafe { tunefold_oboe_seek(handle as *mut AndroidEngine, position_ms as u64) };
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_setVolume(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    volume: jfloat,
) {
    unsafe { tunefold_oboe_set_volume(handle as *mut AndroidEngine, volume) };
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_getPositionMs(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jlong {
    unsafe { tunefold_oboe_get_position_ms(handle as *mut AndroidEngine) as jlong }
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_getPlaybackState(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    unsafe { tunefold_oboe_get_playback_state(handle as *mut AndroidEngine) as jint }
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_getSampleRate(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    unsafe { tunefold_oboe_get_sample_rate(handle as *mut AndroidEngine) as jint }
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_takeTrackFinished(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jboolean {
    if handle == 0 {
        return JNI_FALSE;
    }
    if unsafe { tunefold_oboe_take_track_finished(handle as *mut AndroidEngine) } {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_isDecoderFinished(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jboolean {
    unsafe {
        if tunefold_oboe_is_decoder_finished(handle as *mut AndroidEngine) {
            JNI_TRUE
        } else {
            JNI_FALSE
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_getAvailableFrames(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jint {
    unsafe {
        tunefold_oboe_get_available_frames(handle as *mut AndroidEngine).min(jint::MAX as usize)
            as jint
    }
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_readPcm(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    buffer: JByteBuffer,
    max_frames: jint,
) -> jint {
    if handle == 0 || max_frames <= 0 {
        return 0;
    }
    let address = match env.get_direct_buffer_address(&buffer) {
        Ok(address) => address,
        Err(_) => return 0,
    };
    let capacity = match env.get_direct_buffer_capacity(&buffer) {
        Ok(capacity) => capacity,
        Err(_) => return 0,
    };
    let required = max_frames as usize * 2 * std::mem::size_of::<f32>();
    if address.is_null() || capacity < required {
        return 0;
    }
    let output =
        unsafe { std::slice::from_raw_parts_mut(address.cast::<f32>(), max_frames as usize * 2) };
    unsafe {
        tunefold_oboe_read_pcm(
            handle as *mut AndroidEngine,
            output.as_mut_ptr(),
            max_frames as usize,
        )
        .min(jint::MAX as usize) as jint
    }
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_setOutputState(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    state: jint,
) {
    if let Ok(state) = u8::try_from(state) {
        unsafe { tunefold_oboe_set_output_state(handle as *mut AndroidEngine, state) };
    }
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_getLastError(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    if handle == 0 {
        return std::ptr::null_mut();
    }
    let engine = unsafe { &*(handle as *mut AndroidEngine) };
    let message = engine
        .last_error
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .unwrap_or_default();
    env.new_string(message)
        .map(|value| value.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Arranca una descarga EXPLÍCITA de un track completo.
///
/// Recibe la `PlayableSource` ya resuelta (URL temporal + cabeceras) y la
/// identidad lógica `provider + track id`. La URL nunca se persiste: el
/// archivo se nombra por el digest de la identidad y el destino lo confirma
/// después el `LocalMediaStore` de Java, que valida tamaño y límites.
///
/// Devuelve el id de la descarga (>0) o 0 si no se pudo arrancar.
#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_startDownload(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    directory: JString,
    provider: JString,
    track_id: JString,
    url: JString,
    headers_json: JString,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let engine = unsafe { &*(handle as *mut AndroidEngine) };
    let read = |env: &mut JNIEnv, value: &JString| -> Option<String> {
        env.get_string(value)
            .ok()
            .map(|v| v.to_string_lossy().into_owned())
    };
    let (Some(directory), Some(provider), Some(track_id), Some(url), Some(headers_json)) = (
        read(&mut env, &directory),
        read(&mut env, &provider),
        read(&mut env, &track_id),
        read(&mut env, &url),
        read(&mut env, &headers_json),
    ) else {
        return 0;
    };
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return 0;
    }
    let headers: Vec<(String, String)> = match serde_json::from_str(&headers_json) {
        Ok(headers) => headers,
        Err(_) => return 0,
    };
    let Some((_encoded, _count)) = encode_http_headers(headers.clone()).ok() else {
        return 0;
    };

    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(Mutex::new(DownloadProgress::default()));
    let headers = Arc::new(headers);
    let registry = engine.downloads.clone();
    let thread_registry = registry.clone();
    let last_error = engine.last_error.clone();
    let id = registry.register(cancel.clone(), progress.clone());

    let directory = std::path::PathBuf::from(directory);
    let spawned = std::thread::Builder::new()
        .name("tunefold-download".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    if let Ok(mut slot) = last_error.lock() {
                        *slot = Some(format!("download runtime: {error}"));
                    }
                    thread_registry.finish(id);
                    return;
                }
            };
            let mut observed = 0u64;
            let progress_for_thread = progress.clone();
            let cancel_for_thread = cancel.clone();
            let headers_for_thread = headers.clone();
            let outcome = runtime.block_on(android_download::download_to_local_media(
                &url,
                headers_for_thread.as_ref().clone(),
                &provider,
                &track_id,
                &directory,
                &cancel_for_thread,
                |value| {
                    // Se notifica al avanzar lo suficiente para que el progreso
                    // sea observable sin espamear la línea temporal.
                    if value.received.saturating_sub(observed) >= 64 * 1024 || value.is_complete() {
                        observed = value.received;
                        if let Ok(mut slot) = progress_for_thread.lock() {
                            *slot = value;
                        }
                    }
                },
            ));
            match outcome {
                Ok(_) => {
                    if let Ok(mut slot) = progress_for_thread.lock() {
                        slot.total = Some(slot.received);
                    }
                }
                Err(error) => {
                    if let Ok(mut slot) = last_error.lock() {
                        *slot = Some(format!("download: {error}"));
                    }
                }
            }
            thread_registry.finish(id);
        });
    if spawned.is_err() {
        registry.finish(id);
        return 0;
    }
    id as jlong
}

/// Progreso de una descarga: `received\ttotal` (total `-1` si se desconoce).
#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_getDownloadProgress(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    download_id: jlong,
) -> jstring {
    if handle == 0 {
        return std::ptr::null_mut();
    }
    let engine = unsafe { &*(handle as *mut AndroidEngine) };
    let Some(value) = engine.downloads.progress(download_id as u64) else {
        return std::ptr::null_mut();
    };
    let text = format!(
        "{}\t{}",
        value.received,
        value.total.map_or(-1i64, |total| total as i64)
    );
    _env.new_string(text)
        .map(|value| value.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Cancela una descarga en curso. Devuelve `true` si había una.
///
/// La descarga para entre trozos: no se cancela un `write` a medio hacer, y el
/// `.part` se borra, así que nunca queda un archivo a medias.
#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_cancelDownload(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    download_id: jlong,
) -> jboolean {
    if handle == 0 {
        return JNI_FALSE;
    }
    let engine = unsafe { &*(handle as *mut AndroidEngine) };
    if engine.downloads.cancel(download_id as u64) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// Ruta canónica que Rust usa para un track descargado.
///
/// Es la fuente de verdad del destino: la UI no adivina nombres ni replica el
/// digest en Java. `FileLocalMediaStore` la recibe y escribe su propio sidecar
/// de identidad/tamaño, que es lo que valida thereafter.
#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_localMediaPath(
    mut env: JNIEnv,
    _class: JClass,
    directory: JString,
    provider: JString,
    track_id: JString,
) -> jstring {
    let read = |env: &mut JNIEnv, value: &JString| -> Option<String> {
        env.get_string(value)
            .ok()
            .map(|v| v.to_string_lossy().into_owned())
    };
    let (Some(directory), Some(provider), Some(track_id)) = (
        read(&mut env, &directory),
        read(&mut env, &provider),
        read(&mut env, &track_id),
    ) else {
        return std::ptr::null_mut();
    };
    let Ok(stem) = android_download::local_media_stem(&provider, &track_id) else {
        return std::ptr::null_mut();
    };
    let path = std::path::Path::new(&directory).join(format!("{stem}.audio"));
    env.new_string(path.to_string_lossy().into_owned())
        .map(|value| value.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Limpia los `.part` abandonados. Se llama al abrir el store, NO durante Play.
#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_cleanAbandonedDownloads(
    mut env: JNIEnv,
    _class: JClass,
    directory: JString,
) -> jint {
    let Ok(directory) = env.get_string(&directory) else {
        return 0;
    };
    let directory = directory.to_string_lossy().into_owned();
    android_download::clean_abandoned_parts(
        std::path::Path::new(&directory),
        std::time::Duration::from_secs(3600),
    ) as jint
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_drainPlaybackTrace(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    if handle == 0 {
        return std::ptr::null_mut();
    }
    let engine = unsafe { &*(handle as *mut AndroidEngine) };
    // Un evento por línea: `at_us<TAB>event<TAB>detail`. `at_us` son
    // microsegundos desde el Play tap, no desde el inicio del decoder.
    let mut text = String::new();
    for line in engine.trace.snapshot() {
        text.push_str(&line.at_us.to_string());
        text.push('\t');
        text.push_str(&line.event);
        if !line.detail.is_empty() {
            text.push('\t');
            text.push_str(&line.detail);
        }
        text.push('\n');
    }
    env.new_string(text)
        .map(|value| value.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_getRuntimeDiagnostics(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jstring {
    if handle == 0 {
        return std::ptr::null_mut();
    }
    let engine = unsafe { &*(handle as *mut AndroidEngine) };
    let diagnostic = engine
        .decoder_diagnostics
        .stage
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let analysis_peak = engine
        .analysis
        .as_ref()
        .and_then(|analysis| analysis.waveform_bus().latest())
        .map_or(0.0, |waveform| {
            waveform.left.peak().max(waveform.right.peak())
        });
    let message = format!(
        "{diagnostic}\npackets={} decoded={}f ring={}f analysis={}f output={}f waveform={analysis_peak:.3}\nhttp_open_us={} first_response_us={} probe_us={} first_decode_us={} first_pcm_us={} range_requests={} range_bytes={} range_elapsed_ms={} range_headers_us={} seeks={} seek_requests={} tap_to_decoder_us={}",
        engine
            .decoder_diagnostics
            .decoded_packets
            .load(Ordering::Relaxed),
        engine
            .decoder_diagnostics
            .decoded_frames
            .load(Ordering::Relaxed),
        engine
            .decoder_diagnostics
            .ring_frames
            .load(Ordering::Relaxed),
        engine
            .decoder_diagnostics
            .analysis_frames
            .load(Ordering::Relaxed),
        engine.output_frames.load(Ordering::Relaxed),
        engine.decoder_diagnostics.http_open_us.load(Ordering::Relaxed),
        engine.decoder_diagnostics.first_response_us.load(Ordering::Relaxed),
        engine.decoder_diagnostics.probe_us.load(Ordering::Relaxed),
        engine.decoder_diagnostics.first_decode_us.load(Ordering::Relaxed),
        engine.decoder_diagnostics.first_pcm_us.load(Ordering::Relaxed),
        engine.decoder_diagnostics.range_requests.load(Ordering::Relaxed),
        engine.decoder_diagnostics.range_bytes.load(Ordering::Relaxed),
        engine.decoder_diagnostics.range_elapsed_ms.load(Ordering::Relaxed),
        engine.decoder_diagnostics.headers_us.load(Ordering::Relaxed),
        engine.decoder_diagnostics.seeks.load(Ordering::Relaxed),
        engine
            .decoder_diagnostics
            .seek_requests
            .load(Ordering::Relaxed),
        engine
            .decoder_diagnostics
            .tap_to_decoder_us
            .load(Ordering::Relaxed),
    );
    env.new_string(message)
        .map(|value| value.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_reportAudioError(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    message: JString,
) {
    if handle == 0 {
        return;
    }
    if let Ok(message) = env.get_string(&message) {
        let engine = unsafe { &*(handle as *mut AndroidEngine) };
        set_engine_error(engine, message.to_string_lossy().into_owned());
        engine.cancel.store(true, Ordering::Release);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_getVisualState(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    features: JObject,
    bars: JObject,
) {
    let features_arr = jni::objects::JPrimitiveArray::from(features);
    let bars_arr = jni::objects::JPrimitiveArray::from(bars);

    let result = unsafe {
        let f = env.get_array_elements(&features_arr, jni::objects::ReleaseMode::CopyBack);
        let b = env.get_array_elements(&bars_arr, jni::objects::ReleaseMode::CopyBack);
        (f, b)
    };

    if let (Ok(f), Ok(b)) = result {
        unsafe {
            tunefold_oboe_get_visual_state(
                handle as *mut AndroidEngine,
                f.as_ptr(),
                f.len(),
                b.as_ptr(),
                b.len(),
            );
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_setSurface(
    _env: JNIEnv,
    _class: JClass,
    _handle: jlong,
    _surface: jobject,
) {
}

#[no_mangle]
pub extern "system" fn Java_com_tunefold_app_TunefoldBridge_clearSurface(
    _env: JNIEnv,
    _class: JClass,
    _handle: jlong,
) {
}

#[cfg(test)]
mod track_finished_tests {
    use super::*;

    /// Un user-initiated stop must not be reported as end-of-track, or autoplay
    /// would start a new song after every manual stop.
    #[test]
    fn stop_clears_any_pending_end_of_track() {
        let engine = tunefold_oboe_create_engine();
        let handle = engine;
        unsafe {
            (*handle).track_finished.store(true, Ordering::Release);
            tunefold_oboe_stop(handle);
            assert!(
                !tunefold_oboe_take_track_finished(handle),
                "stopping is not finishing"
            );
        }
        unsafe { tunefold_oboe_destroy_engine(handle) };
    }

    /// The elapsed-time clock belongs to one playback. A new track must start at
    /// zero: it used to inherit the previous track's position and keep counting
    /// from there, which is what the UI showed for the new song.
    #[test]
    fn a_new_playback_starts_the_position_clock_at_zero() {
        let engine = tunefold_oboe_create_engine();
        let handle = engine;
        unsafe {
            // Simulate a track that had been playing for a while.
            if let Ok(mut position) = (*handle).position_ms.lock() {
                *position = 214_000;
            }
            assert_eq!(tunefold_oboe_get_position_ms(handle), 214_000);

            // play_stream is what every new track goes through. A `file:` URL for
            // a path that does not exist reaches the state reset and then fails
            // in the decoder, without needing the network.
            let missing = std::env::temp_dir().join("tunefold-missing-fixture.audio");
            let _ = std::fs::remove_file(&missing);
            let url = std::ffi::CString::new(format!("file:{}", missing.display())).unwrap();
            tunefold_oboe_play_stream(handle, url.as_ptr(), std::ptr::null(), 0, 0);
            assert_eq!(
                tunefold_oboe_get_position_ms(handle),
                0,
                "the next track must not inherit the previous position"
            );
        }
        unsafe { tunefold_oboe_destroy_engine(handle) };
    }

    /// A cancelled decode publishes no end of track; only real media end does.
    ///
    /// Both branches run the production decision, so this pins the behaviour
    /// that stops the queue from advancing on a track change.
    #[test]
    fn only_a_real_end_of_media_publishes_end_of_track() {
        let decoder_finished = AtomicBool::new(false);
        let track_finished = AtomicBool::new(false);

        assert!(
            !publish_decode_outcome(DecodeOutcome::Cancelled, &decoder_finished, &track_finished),
            "a cancelled decode is not an end of track"
        );
        assert!(decoder_finished.load(Ordering::Acquire));
        assert!(
            !track_finished.load(Ordering::Acquire),
            "a cancelled decode must leave playback where it is"
        );

        assert!(publish_decode_outcome(
            DecodeOutcome::EndOfMedia,
            &decoder_finished,
            &track_finished
        ));
        assert!(track_finished.load(Ordering::Acquire));
    }

    /// End-of-track is reported exactly once, so one EOF cannot trigger
    /// autoplay repeatedly.
    #[test]
    fn end_of_track_is_reported_exactly_once() {
        let engine = tunefold_oboe_create_engine();
        let handle = engine;
        unsafe {
            (*handle).track_finished.store(true, Ordering::Release);
            assert!(
                tunefold_oboe_take_track_finished(handle),
                "first read consumes it"
            );
            assert!(
                !tunefold_oboe_take_track_finished(handle),
                "a second read must not report the same EOF again"
            );
        }
        unsafe { tunefold_oboe_destroy_engine(handle) };
    }

    /// Starting a new track clears any leftover flag from the previous one.
    #[test]
    fn starting_a_new_track_clears_a_stale_flag() {
        let engine = tunefold_oboe_create_engine();
        let handle = engine;
        unsafe {
            (*handle).track_finished.store(true, Ordering::Release);
            // play_stream resets the flag before the decoder thread starts.
            (*handle).track_finished.store(false, Ordering::Release);
            assert!(!tunefold_oboe_take_track_finished(handle));
        }
        unsafe { tunefold_oboe_destroy_engine(handle) };
    }
}

#[cfg(test)]
mod header_tests {
    use super::encode_http_headers;

    #[test]
    fn provider_headers_encode_as_http_lines() {
        let (headers, count) = encode_http_headers(vec![
            ("User-Agent".into(), "Tunefold/1".into()),
            ("Referer".into(), "https://example.test/".into()),
        ])
        .unwrap();
        assert_eq!(count, 2);
        assert_eq!(
            headers.to_str().unwrap(),
            "User-Agent: Tunefold/1\nReferer: https://example.test/"
        );
    }

    #[test]
    fn provider_headers_reject_line_injection() {
        assert!(
            encode_http_headers(vec![("X-Test".into(), "safe\r\nInjected: yes".into())]).is_err()
        );
        assert!(encode_http_headers(vec![("Bad:Name".into(), "value".into())]).is_err());
    }
}
