//! The "dumb" master output: one gain control and an optional soft safety
//! limiter. No EQ, no compression; the mixer's channel strip does that job.

use crate::math;

/// Default limiter knee (≈ −2 dBFS). Signal below this is untouched.
pub const DEFAULT_LIMITER_THRESHOLD: f32 = 0.8;

/// Mono master section.
#[derive(Clone, Debug)]
pub struct Master {
    output_gain: f32,
    limiter_enabled: bool,
    threshold: f32,
}

impl Default for Master {
    fn default() -> Self {
        Self {
            output_gain: 1.0,
            limiter_enabled: false,
            threshold: DEFAULT_LIMITER_THRESHOLD,
        }
    }
}

impl Master {
    /// Linear output gain (`0..=4`, i.e. up to +12 dB).
    pub fn set_output_gain(&mut self, gain: f32) {
        self.output_gain = gain.clamp(0.0, 4.0);
    }

    /// Current output gain.
    #[must_use]
    pub fn output_gain(&self) -> f32 {
        self.output_gain
    }

    /// Enables or disables the soft safety limiter.
    pub fn set_limiter_enabled(&mut self, enabled: bool) {
        self.limiter_enabled = enabled;
    }

    /// Whether the soft safety limiter is engaged.
    #[must_use]
    pub fn limiter_enabled(&self) -> bool {
        self.limiter_enabled
    }

    /// Processes one sample.
    #[inline]
    #[must_use]
    pub fn process(&self, x: f32) -> f32 {
        let y = x * self.output_gain;
        if self.limiter_enabled {
            soft_limit(y, self.threshold)
        } else {
            y
        }
    }
}

/// Transparent below `threshold`, then bends smoothly (unit slope at the
/// knee) and never exceeds ±1.
#[inline]
#[must_use]
pub fn soft_limit(x: f32, threshold: f32) -> f32 {
    let a = x.abs();
    if a <= threshold {
        return x;
    }
    let headroom = 1.0 - threshold;
    let excess = (a - threshold) / headroom;
    let shaped = threshold + headroom * math::soft_clip(excess);
    if x < 0.0 {
        -shaped
    } else {
        shaped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_applies() {
        let mut m = Master::default();
        m.set_output_gain(0.5);
        assert_eq!(m.process(0.4), 0.2);
    }

    #[test]
    fn limiter_is_transparent_below_knee_and_bounded_above() {
        let mut m = Master::default();
        m.set_limiter_enabled(true);
        assert_eq!(m.process(0.5), 0.5);
        assert_eq!(m.process(-0.79), -0.79);
        assert!(m.process(3.0) <= 1.0);
        assert!(m.process(-3.0) >= -1.0);
        assert!(m.process(0.9) > 0.8 && m.process(0.9) < 0.9);
        // Monotonic through the knee.
        let mut prev = 0.0;
        let mut x = 0.0;
        while x < 2.0 {
            let y = m.process(x);
            assert!(y >= prev);
            prev = y;
            x += 0.01;
        }
    }
}
