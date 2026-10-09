//! The render path allocates nothing.
//!
//! `Engine::render` is what the browser's AudioWorklet runs on its audio
//! thread for every 128-sample quantum (`p5_engine_render`): a control tick
//! (follower, scheduler, queue pushes) and then `Renderer::process`. The
//! worklet also feeds clock observations and MIDI on that thread. None of
//! it may touch the heap, and neither may `Renderer::process_at`, the
//! audio-callback body of the native shells. A counting global allocator
//! (per thread, so parallel tests do not interfere) checks that, with every
//! voice, flams, shuffle and the limiter in play.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

use engine::{split, ClockMode, Engine, PatternSpec};
use sequencer::VoiceParam;
use sync::{MidiMessage, Observation, Phase, Precision};

struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
}
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

fn note() {
    if COUNTING.with(Cell::get) {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    }
}

// SAFETY: forwards every call to the system allocator unchanged; the
// bookkeeping touches only a const-initialised thread local and an atomic,
// neither of which allocates.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note();
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Heap operations `f` performs on this thread.
fn allocations_in(f: impl FnOnce()) -> u64 {
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    COUNTING.with(|c| c.set(true));
    f();
    COUNTING.with(|c| c.set(false));
    ALLOCATIONS.load(Ordering::Relaxed) - before
}

/// Every voice, accents, flams, shuffle and the limiter.
const BUSY: &str = r#"{
  "bpm": 174,
  "accent": 0.8,
  "shuffle": 0.4,
  "voices": {
    "kick":       { "steps": "X--x --X- F-x- -x-F", "decay": 1.0 },
    "snare":      { "steps": "--x- X--F -x-- F--x", "snappy": 1.0 },
    "low_tom":    { "steps": "x--- F--- x--- --xX" },
    "mid_tom":    { "steps": "-x-- -F-- -x-- -xX-" },
    "high_tom":   { "steps": "--x- --F- --x- xX--" },
    "rim":        { "steps": "xFxF xFxF xFxF xFxF" },
    "clap":       { "steps": "---- F--- ---- X--F" },
    "closed_hat": { "steps": "xXxF xXxF xXxF xXxF" },
    "open_hat":   { "steps": "-X-F -X-F -X-F -X-F", "decay": 1.0 },
    "cowbell":    { "steps": "XF-x -FX- x-F- FX-x" }
  },
  "render": { "output_gain": 2.0, "limiter": true }
}"#;

const BLOCK: usize = 128;

#[test]
fn lockstep_engine_render_does_not_allocate() {
    let spec = PatternSpec::from_json(BUSY).unwrap();
    let mut engine = Engine::new(48_000.0);
    engine.load_spec(&spec).unwrap();
    engine.start();
    let mut out = [0.0f32; BLOCK];
    // Warm up past the first scheduled steps.
    for _ in 0..100 {
        engine.render(&mut out);
    }
    let n = allocations_in(|| {
        // Ten seconds of audio, with fader moves on the way.
        for block in 0..3_750u32 {
            if block % 50 == 0 {
                let level = if block % 100 == 0 { 0.2 } else { 1.0 };
                engine.set_voice_param(sequencer::VoiceId::OpenHat, VoiceParam::Level, level);
            }
            engine.render(&mut out);
        }
    });
    assert_eq!(n, 0, "Engine::render allocated {n} times");
    assert!(engine.renderer().kit().is_active());
}

#[test]
fn following_an_external_clock_does_not_allocate() {
    let spec = PatternSpec::from_json(BUSY).unwrap();
    for precision in [
        Precision::Exact,
        Precision::Fine,
        Precision::Coarse,
        Precision::Jittery,
    ] {
        let mut engine = Engine::new(48_000.0);
        engine.load_spec(&spec).unwrap();
        engine.set_clock_mode(ClockMode::Follow(precision));
        engine.start();
        let mut out = [0.0f32; BLOCK];
        let samples_per_beat = 48_000.0 * 60.0 / 126.0;
        let mut beat = 0u32;
        let mut pulses = 0u32;
        let mut render = |engine: &mut Engine, counting: bool| {
            let pos = engine.position() as f64;
            // A bar-phase report per beat, slightly off the grid, plus
            // MIDI clock pulses (the worklet feeds both on its thread).
            if pos >= f64::from(beat) * samples_per_beat {
                let jitter = if beat % 2 == 0 { 30.0 } else { -30.0 };
                engine.observe(&Observation {
                    sample: f64::from(beat) * samples_per_beat + jitter,
                    phase: Phase::Bar(f64::from(beat % 4)),
                    bpm: Some(126.0),
                });
                beat += 1;
            }
            if pos >= f64::from(pulses) * samples_per_beat / 24.0 {
                let message = if pulses == 0 {
                    MidiMessage::Start
                } else {
                    MidiMessage::Clock
                };
                engine.midi(message, f64::from(pulses) * samples_per_beat / 24.0);
                pulses += 1;
            }
            if counting && beat % 16 == 0 {
                // A quantized re-sync now and then: the snap path.
                engine.resync();
            }
            engine.render(&mut out);
        };
        for _ in 0..400 {
            render(&mut engine, false);
        }
        let n = allocations_in(|| {
            for _ in 0..3_750 {
                render(&mut engine, true);
            }
        });
        assert_eq!(n, 0, "{precision:?}: following allocated {n} times");
        assert!(engine.is_locked() && engine.is_playing(), "{precision:?}");
        assert!((engine.tempo() - 126.0).abs() < 1.0, "{precision:?}");
    }
}

#[test]
fn split_renderer_does_not_allocate() {
    let spec = PatternSpec::from_json(BUSY).unwrap();
    let (mut control, mut renderer) = split(48_000.0);
    control.set_tempo(spec.bpm, 0);
    control.set_pattern(spec.pattern().unwrap());
    for voice in sequencer::VoiceId::ALL {
        control.set_voice_params(voice, &spec.voice_params(voice), 0);
    }
    control.set_output_gain(spec.render.output_gain, 0);
    control.set_limiter(true, 0);
    control.start(0);
    let mut out = [0.0f32; 512];
    let mut host = 1u64;
    let mut total = 0;
    for _ in 0..1_000 {
        // The control thread runs between callbacks; only the callback is
        // counted.
        control.tick_shared();
        host += 10_000_000;
        total += allocations_in(|| renderer.process_at(&mut out, host));
    }
    assert_eq!(total, 0, "Renderer::process_at allocated {total} times");
}

#[test]
fn the_counter_sees_allocations() {
    let n = allocations_in(|| {
        std::hint::black_box(vec![0u8; 64]);
    });
    assert!(n >= 1, "{n}");
}
