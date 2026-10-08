//! Two-handle API for shells with a real audio thread (macOS, iOS).
//!
//! * `P5Renderer` lives in the audio callback. [`p5_renderer_render`] is
//!   real-time safe: no allocation, locks or I/O.
//! * `P5Control` lives on a control thread (a 5–10 ms timer). Everything
//!   else goes through it: patterns, transport, clock sources. Call
//!   [`p5_control_tick`] on every timer fire.
//!
//! The two halves share only the lock-free event queue and the render
//! timing the renderer publishes (position + host time).

use std::ffi::{c_char, CString};
use std::ptr;
use std::time::Duration;

use engine::{split as split_engine, Control, Renderer};
use sync::net::{DeviceInfo, FollowTarget, SourceCommand, SourceEvent, SourceHandle};
use sync::{host_time, Precision};

use crate::{bpm_from, clock_mode_from, midi_from, phase_from, spec_from};

/// Opaque control-thread handle.
pub struct P5Control {
    control: Control,
    source: Option<SourceHandle>,
    devices: Vec<DeviceInfo>,
    devices_json: CString,
    status: CString,
}

/// Opaque audio-thread handle.
pub struct P5Renderer(Renderer);

/// Creates a connected control/render pair. Writes both handles and returns
/// 0, or returns 1 on invalid input. Free each with its own `*_free`.
///
/// # Safety
/// `out_control` and `out_renderer` must be valid pointers to writable
/// pointer slots.
#[no_mangle]
pub unsafe extern "C" fn p5_split_new(
    sample_rate: f32,
    out_control: *mut *mut P5Control,
    out_renderer: *mut *mut P5Renderer,
) -> i32 {
    if out_control.is_null()
        || out_renderer.is_null()
        || !(sample_rate.is_finite() && sample_rate > 0.0)
    {
        return 1;
    }
    let (control, renderer) = split_engine(sample_rate);
    let control = Box::new(P5Control {
        control,
        source: None,
        devices: Vec::new(),
        devices_json: CString::new("[]").expect("no NUL"),
        status: CString::default(),
    });
    // SAFETY: per the caller contract.
    unsafe {
        *out_control = Box::into_raw(control);
        *out_renderer = Box::into_raw(Box::new(P5Renderer(renderer)));
    }
    0
}

/// Destroys a control handle (stopping any running clock source).
///
/// # Safety
/// `control` must be null or a live handle from [`p5_split_new`].
#[no_mangle]
pub unsafe extern "C" fn p5_control_free(control: *mut P5Control) {
    if !control.is_null() {
        // SAFETY: per the caller contract.
        drop(unsafe { Box::from_raw(control) });
    }
}

/// Destroys a renderer handle. Never call while the audio callback may
/// still use it.
///
/// # Safety
/// `renderer` must be null or a live handle from [`p5_split_new`].
#[no_mangle]
pub unsafe extern "C" fn p5_renderer_free(renderer: *mut P5Renderer) {
    if !renderer.is_null() {
        // SAFETY: per the caller contract.
        drop(unsafe { Box::from_raw(renderer) });
    }
}

/// Renders `frames` mono samples. `host_ticks` is the platform host time at
/// which the first sample is output (`AVAudioTimeStamp.mHostTime`), or 0 if
/// unknown. Real-time safe.
///
/// # Safety
/// `renderer` must be a live handle used from one thread at a time; `out`
/// must point to `frames` writable `f32`s.
#[no_mangle]
pub unsafe extern "C" fn p5_renderer_render(
    renderer: *mut P5Renderer,
    out: *mut f32,
    frames: usize,
    host_ticks: u64,
) {
    if renderer.is_null() || out.is_null() {
        return;
    }
    // SAFETY: per the caller contract.
    let (r, out) = unsafe {
        (
            &mut (*renderer).0,
            std::slice::from_raw_parts_mut(out, frames),
        )
    };
    r.process_at(out, host_ticks);
}

fn ctl<'a>(control: *mut P5Control) -> Option<&'a mut P5Control> {
    // SAFETY: every caller is an `unsafe extern` fn whose contract requires
    // a live handle used from one thread at a time.
    unsafe { control.as_mut() }
}

/// Loads a pattern file. Returns 0, 1 on invalid JSON, 2 on null input.
///
/// # Safety
/// `control` must be a live handle; `json` a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn p5_control_load_pattern_json(
    control: *mut P5Control,
    json: *const c_char,
) -> i32 {
    let Some(c) = ctl(control) else { return 2 };
    // SAFETY: per the caller contract.
    let spec = match unsafe { spec_from(json) } {
        Ok(s) => s,
        Err(code) => return code,
    };
    let Ok(pattern) = spec.pattern() else {
        return 1;
    };
    let now = c.control.render_timing().position;
    c.control.set_tempo(spec.bpm, now);
    c.control.set_pattern(pattern);
    for voice in sequencer::VoiceId::ALL {
        c.control
            .set_voice_params(voice, &spec.voice_params(voice), now);
    }
    c.control.set_output_gain(spec.render.output_gain, now);
    c.control.set_limiter(spec.render.limiter, now);
    0
}

/// One control tick: drains the clock source, follows it, schedules ahead.
/// Call every 5–10 ms.
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_tick(control: *mut P5Control) {
    let Some(c) = ctl(control) else { return };
    drain_source(c);
    c.control.tick_shared();
}

fn drain_source(c: &mut P5Control) {
    let Some(source) = c.source.as_ref() else {
        return;
    };
    let mut devices_changed = false;
    while let Some(event) = source.try_recv() {
        match event {
            SourceEvent::Observation {
                host_ns,
                phase,
                bpm,
                ..
            } => c.control.observe_host(host_ns, phase, bpm),
            SourceEvent::Devices(list) => {
                c.devices = list;
                devices_changed = true;
            }
            SourceEvent::Status { message, .. } => {
                c.status = CString::new(message.replace('\0', " ")).unwrap_or_default();
            }
        }
    }
    if devices_changed {
        let json: Vec<serde_json::Value> = c
            .devices
            .iter()
            .map(|d| {
                serde_json::json!({
                    "number": d.number, "name": d.name, "address": d.address,
                    "kind": d.kind.name(), "bpm": d.bpm, "playing": d.playing,
                    "master": d.master, "on_air": d.on_air,
                })
            })
            .collect();
        let text = serde_json::Value::Array(json).to_string();
        c.devices_json = CString::new(text).unwrap_or_default();
    }
}

/// Starts playback (see `p5_engine_start`).
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_start(control: *mut P5Control) {
    if let Some(c) = ctl(control) {
        let now = c.control.render_timing().position;
        c.control.start(now);
    }
}

/// Stops scheduling.
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_stop(control: *mut P5Control) {
    if let Some(c) = ctl(control) {
        c.control.stop();
    }
}

/// `1` while playing.
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_is_playing(control: *mut P5Control) -> i32 {
    ctl(control).map_or(0, |c| i32::from(c.control.is_playing()))
}

/// Pattern step audible now, or -1 when stopped.
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_playing_step(control: *mut P5Control) -> i32 {
    let Some(c) = ctl(control) else { return -1 };
    if !c.control.is_playing() {
        return -1;
    }
    let beat = c.control.beat_at(c.control.render_timing().position);
    if beat < 0.0 {
        return -1;
    }
    ((beat / sequencer::BEATS_PER_STEP).floor() as i64).rem_euclid(16) as i32
}

/// Tempo of the active clock.
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_tempo(control: *mut P5Control) -> f64 {
    ctl(control).map_or(0.0, |c| c.control.tempo())
}

/// Continuous beat at the renderer's current position.
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_beat(control: *mut P5Control) -> f64 {
    ctl(control).map_or(0.0, |c| {
        c.control.beat_at(c.control.render_timing().position)
    })
}

/// `1` if the active clock is tracking its source.
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_clock_locked(control: *mut P5Control) -> i32 {
    ctl(control).map_or(0, |c| i32::from(c.control.is_locked()))
}

/// Selects the clock mode (code table in the crate docs). 0 ok, 1 bad code.
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_set_clock_mode(control: *mut P5Control, mode: i32) -> i32 {
    match (ctl(control), clock_mode_from(mode)) {
        (Some(c), Some(m)) => {
            let now = c.control.render_timing().position;
            c.control.set_clock_mode(m, now);
            0
        }
        _ => 1,
    }
}

/// Feeds an observation timestamped in host nanoseconds
/// (`sync::host_time` scale; on Apple, `mach_absolute_time` in ns).
/// 0 ok, 1 bad input.
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_observe_host(
    control: *mut P5Control,
    host_ns: u64,
    phase_kind: i32,
    phase: f64,
    bpm: f64,
) -> i32 {
    match (ctl(control), phase_from(phase_kind, phase)) {
        (Some(c), Some(p)) => {
            c.control.observe_host(host_ns, p, bpm_from(bpm));
            0
        }
        _ => 1,
    }
}

/// Feeds a MIDI clock message received at `host_ns`. 0 ok, 1 bad input or
/// no host time published yet.
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_midi_host(
    control: *mut P5Control,
    message: i32,
    host_ns: u64,
) -> i32 {
    let (Some(c), Some(m)) = (ctl(control), midi_from(message)) else {
        return 1;
    };
    let Some(sample) = c.control.host_ns_to_sample(host_ns) else {
        return 1;
    };
    let now = c.control.render_timing().position;
    c.control.midi(m, sample, now);
    0
}

/// Registers a tap at `host_ns` (0 = now).
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_tap_host(control: *mut P5Control, host_ns: u64) {
    let Some(c) = ctl(control) else { return };
    let host_ns = if host_ns == 0 {
        host_time::now_ns()
    } else {
        host_ns
    };
    let now = c.control.render_timing().position;
    let sample = c.control.host_ns_to_sample(host_ns).unwrap_or(now as f64);
    c.control.tap(sample, now);
}

/// Quantized re-sync.
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_resync(control: *mut P5Control) {
    if let Some(c) = ctl(control) {
        let now = c.control.render_timing().position;
        c.control.resync(now);
    }
}

/// Phase nudge in milliseconds.
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_set_nudge_ms(control: *mut P5Control, ms: f64) {
    if let Some(c) = ctl(control) {
        c.control.set_nudge_ms(ms);
    }
}

/// Output latency compensation in milliseconds.
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_set_latency_ms(control: *mut P5Control, ms: f64) {
    if let Some(c) = ctl(control) {
        c.control.set_latency_ms(ms);
    }
}

/// Current host time in nanoseconds (`sync::host_time`), for timestamping
/// MIDI or taps on the shell side.
#[no_mangle]
pub extern "C" fn p5_host_time_ns() -> u64 {
    host_time::now_ns()
}

/// Starts a network clock source and switches to following it:
/// `kind` 1 = Pro DJ Link, 2 = Opus Quad, 3 = Ableton Link, 4 = simulated
/// (`bpm` used). `device_number` is the Pro DJ Link device number to claim
/// (0 = default). Returns 0, 1 on bad input, 2 if this build lacks the
/// source, 3 if starting failed (see [`p5_control_status`]).
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_start_source(
    control: *mut P5Control,
    kind: i32,
    device_number: i32,
    bpm: f64,
) -> i32 {
    let Some(c) = ctl(control) else { return 1 };
    let _ = device_number;
    c.source = None;
    let (result, precision) = match kind {
        4 => (
            sync::net::start_simulated(bpm_from(bpm).unwrap_or(120.0), Duration::from_millis(20)),
            Precision::Exact,
        ),
        1..=3 => return 2,
        _ => return 1,
    };
    match result {
        Ok(handle) => {
            c.source = Some(handle);
            let now = c.control.render_timing().position;
            c.control
                .set_clock_mode(engine::ClockMode::Follow(precision), now);
            0
        }
        Err(e) => {
            c.status = CString::new(e.to_string().replace('\0', " ")).unwrap_or_default();
            3
        }
    }
}

/// Stops the network clock source (the clock keeps free-running at the last
/// tempo until the mode is changed).
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_stop_source(control: *mut P5Control) {
    if let Some(c) = ctl(control) {
        c.source = None;
    }
}

/// Chooses which device a network source follows: 0 = tempo master,
/// 1–255 = that device number.
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_follow(control: *mut P5Control, device: i32) {
    if let Some(c) = ctl(control) {
        if let Some(s) = c.source.as_ref() {
            let target = match u8::try_from(device) {
                Ok(0) | Err(_) => FollowTarget::Master,
                Ok(n) => FollowTarget::Device(n),
            };
            s.command(SourceCommand::Follow(target));
        }
    }
}

/// JSON array of devices seen by the network source. The pointer stays
/// valid until the next [`p5_control_tick`].
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_devices_json(control: *mut P5Control) -> *const c_char {
    ctl(control).map_or(ptr::null(), |c| c.devices_json.as_ptr())
}

/// Last status message from the clock source (may be empty). The pointer
/// stays valid until the next [`p5_control_tick`].
///
/// # Safety
/// `control` must be a live handle.
#[no_mangle]
pub unsafe extern "C" fn p5_control_status(control: *mut P5Control) -> *const c_char {
    ctl(control).map_or(ptr::null(), |c| c.status.as_ptr())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn split_pair_renders_on_another_thread() {
        let mut control = ptr::null_mut();
        let mut renderer = ptr::null_mut();
        unsafe {
            assert_eq!(p5_split_new(48_000.0, &mut control, &mut renderer), 0);
            let json = CString::new(r#"{ "voices": { "kick": { "steps": "x---x---x---x---" } } }"#)
                .unwrap();
            assert_eq!(p5_control_load_pattern_json(control, json.as_ptr()), 0);
            p5_control_start(control);
            p5_control_tick(control);

            // Move the renderer to an "audio thread".
            let r = renderer as usize;
            let audio = std::thread::spawn(move || {
                let r = r as *mut P5Renderer;
                let mut out = vec![0.0f32; 512];
                let mut any = false;
                for _ in 0..8 {
                    p5_renderer_render(r, out.as_mut_ptr(), out.len(), 0);
                    any |= out.iter().any(|&s| s != 0.0);
                }
                any
            });
            assert!(audio.join().unwrap(), "kick on step 0 should sound");
            assert_eq!(p5_control_is_playing(control), 1);
            p5_control_free(control);
            p5_renderer_free(renderer);
        }
    }

    #[test]
    fn simulated_source_drives_follow_mode() {
        let mut control = ptr::null_mut();
        let mut renderer = ptr::null_mut();
        unsafe {
            p5_split_new(48_000.0, &mut control, &mut renderer);
            assert_eq!(p5_control_start_source(control, 9, 0, 0.0), 1);
            assert_eq!(p5_control_start_source(control, 4, 0, 133.0), 0);
            let mut out = vec![0.0f32; 480];
            // Render with real host times so observations can be mapped.
            for _ in 0..40 {
                let now = sync::host_time::now_ticks();
                p5_renderer_render(renderer, out.as_mut_ptr(), out.len(), now);
                p5_control_tick(control);
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(p5_control_clock_locked(control), 1);
            assert!((p5_control_tempo(control) - 133.0).abs() < 0.5);
            p5_control_stop_source(control);
            p5_control_free(control);
            p5_renderer_free(renderer);
        }
    }
}
