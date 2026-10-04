//! Android JNI bridge for Phase 3: real decoder pipeline.
//!
//! Provides the FFI surface for:
//! 1. Real PCM playback via Oboe (HTTP → Symphonia → PCM → Oboe)
//! 2. Playback controls (play/pause/resume/stop/seek/volume)
//! 3. Analysis engine integration (same PCM feeds analysis)
//! 4. Visual state delivery to Kotlin

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use jni::objects::{JClass, JObject};
use jni::sys::{jfloat, jlong, jobject};
use jni::JNIEnv;

use crate::analysis::{AnalysisConfig, AnalysisRuntime};

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
        sample_rate: Arc::new(AtomicU64::new(44100)),
        volume: Arc::new(Mutex::new(1.0)),
        cancel: Arc::new(AtomicBool::new(false)),
        decoder_handle: Arc::new(Mutex::new(None)),
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
    let engine_ref = &*engine;
    engine_ref.cancel.store(true, Ordering::Release);
    if let Ok(mut handle) = engine_ref.decoder_handle.lock() {
        if let Some(h) = handle.take() {
            let _ = h.join();
        }
    }
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

    engine.cancel.store(true, Ordering::Release);
    if let Ok(mut handle) = engine.decoder_handle.lock() {
        if let Some(h) = handle.take() {
            let _ = h.join();
        }
    }

    let url_str = std::ffi::CStr::from_ptr(url).to_string_lossy().into_owned();
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

    let ring = engine.ring.clone();
    let cancel = engine.cancel.clone();
    let sample_rate = engine.sample_rate.clone();
    let position_ms = engine.position_ms.clone();

    let handle = std::thread::spawn(move || {
        let decoder = crate::platform::android_decoder::AndroidDecoder::new(
            ring, cancel, sample_rate, position_ms,
        );
        let _ = decoder.decode_stream(url_str, header_vec);
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
        if let Some(delta) = (num_frames as u64).checked_mul(1000).and_then(|ms| ms.checked_div(sample_rate)) {
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
    engine.ring.pop(slice)
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
        let f = env.get_array_elements(&features_arr, jni::objects::ReleaseMode::NoCopyBack);
        let b = env.get_array_elements(&bars_arr, jni::objects::ReleaseMode::NoCopyBack);
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
