//! C ABI for the Swift shells.
//!
//! The surface is deliberately tiny and grows only when a shell needs
//! something. It is consumed two ways with no changes:
//!
//! * as a static library by Swift (macOS, iOS);
//! * as a `cdylib` compiled to `wasm32-unknown-unknown` and instantiated
//!   inside an AudioWorklet (`apps/web`). The module has no imports; the
//!   worklet writes JSON into memory obtained from [`p5_alloc`] and reads
//!   rendered audio from a buffer it allocated the same way.
//!
//! The engine is driven single-threaded here (control and render halves in
//! lockstep), which is exactly right for the single-threaded worklet. The
//! mac shell will split the halves so the render half lives on the audio
//! thread.
//!
//! Header generation: `scripts/gen-header.sh` (requires `cargo install
//! cbindgen`).

use std::alloc::Layout;
use std::ffi::{c_char, CStr};
use std::ptr;

use engine::{Engine, PatternSpec};

/// Opaque engine handle.
pub struct P5Engine(Engine);

/// ABI version. Bump on any breaking change to this file.
#[no_mangle]
pub extern "C" fn p5_abi_version() -> u32 {
    2
}

/// Allocates `bytes` of memory (8-byte aligned) for the host to fill, e.g.
/// with a NUL-terminated JSON string or an output buffer. Returns null for
/// zero bytes. Free with [`p5_free`] using the same size.
#[no_mangle]
pub extern "C" fn p5_alloc(bytes: usize) -> *mut u8 {
    let Ok(layout) = Layout::from_size_align(bytes, 8) else {
        return ptr::null_mut();
    };
    if layout.size() == 0 {
        return ptr::null_mut();
    }
    // SAFETY: layout has non-zero size.
    unsafe { std::alloc::alloc_zeroed(layout) }
}

/// Frees memory from [`p5_alloc`]. Null is ignored.
///
/// # Safety
/// `ptr` must be null or come from `p5_alloc(bytes)` with the same `bytes`.
#[no_mangle]
pub unsafe extern "C" fn p5_free(ptr: *mut u8, bytes: usize) {
    if ptr.is_null() {
        return;
    }
    if let Ok(layout) = Layout::from_size_align(bytes, 8) {
        // SAFETY: per the caller contract, same layout as the allocation.
        unsafe { std::alloc::dealloc(ptr, layout) };
    }
}

/// Creates an engine at `sample_rate` Hz. Returns null on invalid input.
/// Free with [`p5_engine_free`].
#[no_mangle]
pub extern "C" fn p5_engine_new(sample_rate: f32) -> *mut P5Engine {
    if !(sample_rate.is_finite() && sample_rate > 0.0) {
        return ptr::null_mut();
    }
    Box::into_raw(Box::new(P5Engine(Engine::new(sample_rate))))
}

/// Destroys an engine created by [`p5_engine_new`]. Null is ignored.
///
/// # Safety
/// `engine` must be null or a pointer returned by `p5_engine_new` that has
/// not been freed.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_free(engine: *mut P5Engine) {
    if !engine.is_null() {
        // SAFETY: the caller guarantees the pointer came from Box::into_raw
        // in p5_engine_new and is not used afterwards.
        drop(unsafe { Box::from_raw(engine) });
    }
}

/// Loads a pattern file (the JSON format documented in `engine::spec`),
/// applying tempo, pattern, voice controls and master settings. Returns 0 on
/// success, 1 on invalid JSON, 2 on null arguments.
///
/// # Safety
/// `engine` must be a live handle; `json` must be a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_load_pattern_json(
    engine: *mut P5Engine,
    json: *const c_char,
) -> i32 {
    if engine.is_null() || json.is_null() {
        return 2;
    }
    // SAFETY: both pointers are valid per the caller contract.
    let (engine, json) = unsafe { (&mut (*engine).0, CStr::from_ptr(json)) };
    let Ok(text) = json.to_str() else { return 1 };
    let Ok(spec) = PatternSpec::from_json(text) else {
        return 1;
    };
    let Ok(pattern) = spec.pattern() else {
        return 1;
    };
    engine.set_tempo(spec.bpm);
    engine.set_pattern(pattern);
    let kick = spec.kick_params();
    engine.set_kick_param(sequencer::VoiceParam::Tune, kick.tune);
    engine.set_kick_param(sequencer::VoiceParam::Decay, kick.decay);
    engine.set_kick_param(sequencer::VoiceParam::Level, kick.level);
    engine.set_output_gain(spec.render.output_gain);
    engine.set_limiter(spec.render.limiter);
    0
}

/// Starts playback from step 0 at the current render position.
///
/// # Safety
/// `engine` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_start(engine: *mut P5Engine) {
    if let Some(e) = unsafe { engine.as_mut() } {
        e.0.start();
    }
}

/// Stops scheduling new steps.
///
/// # Safety
/// `engine` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_stop(engine: *mut P5Engine) {
    if let Some(e) = unsafe { engine.as_mut() } {
        e.0.stop();
    }
}

/// Stops automatically after `steps` steps from the next start; `0` loops
/// forever.
///
/// # Safety
/// `engine` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_set_stop_after(engine: *mut P5Engine, steps: u64) {
    if let Some(e) = unsafe { engine.as_mut() } {
        e.0.set_stop_after((steps > 0).then_some(steps));
    }
}

/// Absolute render position in samples.
///
/// # Safety
/// `engine` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_position(engine: *const P5Engine) -> u64 {
    unsafe { engine.as_ref() }.map_or(0, |e| e.0.position())
}

/// Pattern step (`0..16`) audible at the current position, or `-1` when
/// stopped. For playhead displays.
///
/// # Safety
/// `engine` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_playing_step(engine: *const P5Engine) -> i32 {
    unsafe { engine.as_ref() }
        .and_then(|e| e.0.playing_step())
        .map_or(-1, |s| s as i32)
}

/// Renders `frames` mono samples into `out`. Session-1 convenience: drives
/// both halves in lockstep, so it must be called from a single thread.
///
/// # Safety
/// `engine` must be a live handle; `out` must point to `frames` writable
/// `f32`s.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_render(engine: *mut P5Engine, out: *mut f32, frames: usize) {
    if engine.is_null() || out.is_null() {
        return;
    }
    // SAFETY: per the caller contract.
    let (engine, out) = unsafe {
        (
            &mut (*engine).0,
            std::slice::from_raw_parts_mut(out, frames),
        )
    };
    engine.render(out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn create_load_render_free() {
        let engine = p5_engine_new(48_000.0);
        assert!(!engine.is_null());
        let json =
            CString::new(r#"{ "voices": { "kick": { "steps": "x---x---x---x---" } } }"#).unwrap();
        unsafe {
            assert_eq!(p5_engine_load_pattern_json(engine, json.as_ptr()), 0);
            p5_engine_start(engine);
            let mut out = vec![0.0f32; 4_800];
            p5_engine_render(engine, out.as_mut_ptr(), out.len());
            assert!(out.iter().any(|&s| s != 0.0));
            p5_engine_free(engine);
        }
        assert!(p5_engine_new(0.0).is_null());
    }

    #[test]
    fn alloc_free_and_playhead() {
        let p = p5_alloc(64);
        assert!(!p.is_null());
        unsafe {
            *p = 7;
            p5_free(p, 64);
        }
        assert!(p5_alloc(0).is_null());

        let engine = p5_engine_new(48_000.0);
        let json = CString::new(
            r#"{ "bpm": 120, "voices": { "kick": { "steps": "x---x---x---x---" } } }"#,
        )
        .unwrap();
        unsafe {
            assert_eq!(p5_engine_playing_step(engine), -1);
            p5_engine_load_pattern_json(engine, json.as_ptr());
            p5_engine_set_stop_after(engine, 16);
            p5_engine_start(engine);
            let mut out = vec![0.0f32; 6_000];
            p5_engine_render(engine, out.as_mut_ptr(), out.len());
            // 6 000 samples = one step at 120 BPM / 48 kHz.
            assert_eq!(p5_engine_playing_step(engine), 1);
            assert_eq!(p5_engine_position(engine), 6_000);
            for _ in 0..16 {
                p5_engine_render(engine, out.as_mut_ptr(), out.len());
            }
            // stop_after(16) has ended playback after one bar.
            assert_eq!(p5_engine_playing_step(engine), -1);
            p5_engine_free(engine);
        }
    }
}
