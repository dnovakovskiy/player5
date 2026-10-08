//! C ABI for the platform shells.
//!
//! The surface is deliberately small and grows only when a shell needs
//! something. It is consumed two ways with no changes:
//!
//! * as a static library by Swift (macOS, iOS), through the two-handle
//!   split API in [`split`]: `P5Control` on a control thread, `P5Renderer`
//!   inside the audio callback;
//! * as a `cdylib` compiled to `wasm32-unknown-unknown` and instantiated
//!   inside an AudioWorklet (`apps/web`), through the single-handle
//!   lockstep API below (`P5Engine`). The module has no imports; the
//!   worklet writes JSON into memory obtained from [`p5_alloc`] and reads
//!   rendered audio from a buffer it allocated the same way.
//!
//! Integer codes shared by both APIs:
//!
//! | code | clock mode (`p5_*_set_clock_mode`) | phase kind (`*_observe`) | MIDI message |
//! |------|------------------------------------|--------------------------|--------------|
//! | 0 | internal | bar position `0..4` | clock (0xF8) |
//! | 1 | follow, exact (Link, CDJ-3000 precise) | beat position `0..1` | start (0xFA) |
//! | 2 | follow, fine (beat packets, bridge) | tempo only | continue (0xFB) |
//! | 3 | follow, coarse (Opus Quad) | — | stop (0xFC) |
//! | 4 | follow, jittery (MIDI clock) | — | — |
//!
//! Header generation: `scripts/gen-header.sh` (requires `cargo install
//! cbindgen`).

pub mod split;

use std::alloc::Layout;
use std::ffi::{c_char, CStr};
use std::ptr;

use engine::{ClockMode, Engine, PatternSpec};
use sync::{MidiMessage, Observation, Phase, Precision};

/// Opaque single-threaded engine handle (control and render in lockstep).
pub struct P5Engine(Engine);

/// ABI version. Bump on any breaking change to this crate's exports.
#[no_mangle]
pub extern "C" fn p5_abi_version() -> u32 {
    3
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

pub(crate) fn clock_mode_from(code: i32) -> Option<ClockMode> {
    Some(match code {
        0 => ClockMode::Internal,
        1 => ClockMode::Follow(Precision::Exact),
        2 => ClockMode::Follow(Precision::Fine),
        3 => ClockMode::Follow(Precision::Coarse),
        4 => ClockMode::Follow(Precision::Jittery),
        _ => return None,
    })
}

pub(crate) fn phase_from(kind: i32, value: f64) -> Option<Phase> {
    if !value.is_finite() {
        return None;
    }
    Some(match kind {
        0 => Phase::Bar(value.rem_euclid(4.0)),
        1 => Phase::Beat(value.rem_euclid(1.0)),
        2 => Phase::TempoOnly,
        _ => return None,
    })
}

pub(crate) fn bpm_from(bpm: f64) -> Option<f64> {
    (bpm.is_finite() && bpm > 0.0).then_some(bpm)
}

pub(crate) fn midi_from(code: i32) -> Option<MidiMessage> {
    Some(match code {
        0 => MidiMessage::Clock,
        1 => MidiMessage::Start,
        2 => MidiMessage::Continue,
        3 => MidiMessage::Stop,
        _ => return None,
    })
}

/// Parses NUL-terminated JSON into a spec. Errors: 1 invalid, 2 null.
///
/// # Safety
/// `json` must be null or a NUL-terminated string.
pub(crate) unsafe fn spec_from(json: *const c_char) -> Result<PatternSpec, i32> {
    if json.is_null() {
        return Err(2);
    }
    // SAFETY: per the caller contract.
    let text = unsafe { CStr::from_ptr(json) };
    let text = text.to_str().map_err(|_| 1)?;
    PatternSpec::from_json(text).map_err(|_| 1)
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
/// applying tempo, pattern, every voice's controls and master settings.
/// Only controls that changed are sent to the renderer. Returns 0 on
/// success, 1 on invalid JSON, 2 on null arguments.
///
/// # Safety
/// `engine` must be a live handle; `json` must be a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_load_pattern_json(
    engine: *mut P5Engine,
    json: *const c_char,
) -> i32 {
    // SAFETY: per the caller contract.
    let Some(engine) = (unsafe { engine.as_mut() }) else {
        return 2;
    };
    // SAFETY: per the caller contract.
    match unsafe { spec_from(json) } {
        Ok(spec) => match engine.0.load_spec(&spec) {
            Ok(()) => 0,
            Err(_) => 1,
        },
        Err(code) => code,
    }
}

/// Starts playback at the current render position (internal clock: from
/// step 0; following: joins the source's bar phase at the next step).
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

/// Tempo of the active clock in BPM.
///
/// # Safety
/// `engine` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_tempo(engine: *const P5Engine) -> f64 {
    unsafe { engine.as_ref() }.map_or(0.0, |e| e.0.tempo())
}

/// Continuous beat (nudge and latency applied) at the current position.
/// `beat % 4` is the bar position.
///
/// # Safety
/// `engine` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_beat(engine: *const P5Engine) -> f64 {
    unsafe { engine.as_ref() }.map_or(0.0, |e| e.0.beat())
}

/// `1` if the active clock is tracking its source (always for internal).
///
/// # Safety
/// `engine` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_clock_locked(engine: *const P5Engine) -> i32 {
    unsafe { engine.as_ref() }.map_or(0, |e| i32::from(e.0.is_locked()))
}

/// Selects the clock (see the code table in the crate docs). Returns 0, or
/// 1 for an unknown code.
///
/// # Safety
/// `engine` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_set_clock_mode(engine: *mut P5Engine, mode: i32) -> i32 {
    match (unsafe { engine.as_mut() }, clock_mode_from(mode)) {
        (Some(e), Some(m)) => {
            e.0.set_clock_mode(m);
            0
        }
        _ => 1,
    }
}

/// Feeds an external observation: at sample position `sample` (fractional,
/// engine sample clock) the source was at `phase` (meaning per
/// `phase_kind`), at `bpm` (`<= 0` = unknown). Returns 0, or 1 on bad input.
///
/// # Safety
/// `engine` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_observe(
    engine: *mut P5Engine,
    sample: f64,
    phase_kind: i32,
    phase: f64,
    bpm: f64,
) -> i32 {
    let (Some(e), Some(phase)) = (unsafe { engine.as_mut() }, phase_from(phase_kind, phase)) else {
        return 1;
    };
    if !sample.is_finite() {
        return 1;
    }
    e.0.observe(&Observation {
        sample,
        phase,
        bpm: bpm_from(bpm),
    });
    0
}

/// Feeds a MIDI clock message (code table in the crate docs) received at
/// `sample`. Returns 0, or 1 on bad input.
///
/// # Safety
/// `engine` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_midi(engine: *mut P5Engine, message: i32, sample: f64) -> i32 {
    match (unsafe { engine.as_mut() }, midi_from(message)) {
        (Some(e), Some(m)) if sample.is_finite() => {
            e.0.midi(m, sample);
            0
        }
        _ => 1,
    }
}

/// Registers a tap at `sample`.
///
/// # Safety
/// `engine` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_tap(engine: *mut P5Engine, sample: f64) {
    if let Some(e) = unsafe { engine.as_mut() } {
        if sample.is_finite() {
            e.0.tap(sample);
        }
    }
}

/// Quantized re-sync (see `engine::Control::resync`).
///
/// # Safety
/// `engine` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_resync(engine: *mut P5Engine) {
    if let Some(e) = unsafe { engine.as_mut() } {
        e.0.resync();
    }
}

/// Phase nudge in milliseconds (positive = later).
///
/// # Safety
/// `engine` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_set_nudge_ms(engine: *mut P5Engine, ms: f64) {
    if let Some(e) = unsafe { engine.as_mut() } {
        e.0.set_nudge_ms(ms);
    }
}

/// Output-path latency compensation in milliseconds (positive = earlier).
///
/// # Safety
/// `engine` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_engine_set_latency_ms(engine: *mut P5Engine, ms: f64) {
    if let Some(e) = unsafe { engine.as_mut() } {
        e.0.set_latency_ms(ms);
    }
}

/// Renders `frames` mono samples into `out`, ticking the scheduler first.
/// Single-threaded by design.
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

    #[test]
    fn follow_mode_takes_tempo_from_observations() {
        let engine = p5_engine_new(48_000.0);
        unsafe {
            assert_eq!(p5_engine_set_clock_mode(engine, 9), 1);
            assert_eq!(p5_engine_set_clock_mode(engine, 1), 0);
            assert_eq!(p5_engine_observe(engine, 0.0, 0, 0.0, 128.0), 0);
            assert_eq!(p5_engine_observe(engine, 0.0, 7, 0.0, 128.0), 1);
            assert!((p5_engine_tempo(engine) - 128.0).abs() < 1e-9);
            assert_eq!(p5_engine_clock_locked(engine), 1);
            assert_eq!(p5_engine_midi(engine, 0, 10.0), 0);
            assert_eq!(p5_engine_midi(engine, 5, 10.0), 1);
            p5_engine_free(engine);
        }
    }
}
