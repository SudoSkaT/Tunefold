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
use crate::platform::android_decoder::DecoderDiagnostics;

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
    last_error: Arc<Mutex<Option<String>>>,
    decoder_diagnostics: Arc<DecoderDiagnostics>,
    output_frames: AtomicU64,
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
        last_error: Arc::new(Mutex::new(None)),
        decoder_diagnostics: Arc::new(DecoderDiagnostics::new()),
        output_frames: AtomicU64::new(0),
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
) {
    if engine.is_null() || url.is_null() {
        return;
    }
    let engine = &*engine;

    let url_str = std::ffi::CStr::from_ptr(url).to_string_lossy().into_owned();
    if !(url_str.starts_with("https://") || url_str.starts_with("http://")) {
        set_engine_error(engine, "URL must use http or https".to_string());
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
    engine.playing.store(true, Ordering::Release);
    engine.state.store(STATE_BUFFERING, Ordering::Release);
    engine.decoder_finished.store(false, Ordering::Release);
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
    let last_error = engine.last_error.clone();
    let decoder_diagnostics = engine.decoder_diagnostics.clone();

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
            );
            match decoder.decode_stream(url_str, header_vec) {
                Ok(()) => {
                    decoder_finished.store(true, Ordering::Release);
                    if state.load(Ordering::Acquire) == STATE_BUFFERING
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
) -> jboolean {
    if handle == 0 {
        let _ = env.throw_new(
            "java/lang/IllegalStateException",
            "Rust engine is not initialized",
        );
        return JNI_FALSE;
    }
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
    let empty_headers = std::ffi::CString::default();
    unsafe {
        tunefold_oboe_play_stream(
            handle as *mut AndroidEngine,
            url.as_ptr(),
            empty_headers.as_ptr(),
            0,
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
        "{diagnostic}\npackets={} decoded={}f ring={}f analysis={}f output={}f waveform={analysis_peak:.3}",
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
