//! No voice's internal state may go subnormal.
//!
//! Subnormal (denormal) floats take a slow microcode path on x86, natively
//! and in WebAssembly, so a decaying state that drifts into that range keeps
//! the render thread busy for as long as the voice rings. Every voice
//! flushes its decaying states to zero well before then. The voices keep
//! their state private, so this test reads it from their `Debug` output,
//! which prints every field.

use dsp::{Kit, Param, VOICE_COUNT};

/// The subnormal numbers that appear in `text`.
fn subnormals(text: &str) -> Vec<f32> {
    text.split(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | 'e' | '-' | '+')))
        .filter_map(|t| t.parse::<f32>().ok())
        .filter(|v| v.is_subnormal())
        .collect()
}

#[test]
fn the_parser_sees_subnormals() {
    let text = format!("{:?}", (1.0e-39f32, 0.5f32, -2.0e-40f32, 0.0f32));
    assert_eq!(subnormals(&text).len(), 2, "{text}");
}

#[test]
fn no_voice_state_goes_subnormal() {
    for slot in 0..VOICE_COUNT {
        for velocity in [1.0f32, 0.42] {
            let mut kit = Kit::new(48_000.0);
            // Longest decay: the slowest envelopes linger longest near zero.
            for param in Param::ALL {
                kit.set_param(slot, param, 1.0);
            }
            kit.trigger(slot, velocity);
            for i in 0..48_000 * 5 {
                kit.process();
                if i % 64 == 0 {
                    let state = format!("{kit:?}");
                    let found = subnormals(&state);
                    assert!(
                        found.is_empty(),
                        "slot {slot}, velocity {velocity}, sample {i}: {found:?}\n{state}"
                    );
                }
            }
        }
    }
}
