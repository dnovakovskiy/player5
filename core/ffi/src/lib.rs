//! C ABI for the Swift shells.
//!
//! The surface is deliberately tiny and grows only when a shell needs
//! something. Session 1 exposes just enough to prove the link: create an
//! engine, load a pattern file, render mono blocks. The mac shell (session 2)
//! adds what its audio callback needs — and splits the engine so the render
//! half lives on the audio thread.
//!
//! Header generation: `scripts/gen-header.sh` (requires `cargo install
//! cbindgen`).

use std::ffi::{c_char, CStr};
use std::ptr;

use engine::{Engine, PatternSpec};

/// Opaque engine handle.
pub struct P5Engine(Engine);

/// ABI version. Bump on any breaking change to this file.
#[no_mangle]
pub extern "C" fn p5_abi_version() -> u32 {
    1
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
}
