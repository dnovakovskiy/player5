//! Deterministic, allocation-free math for the render path.
//!
//! The platform `libm` implementations of `exp`, `sin`, `tanh` and friends
//! differ in their last bits between Linux, macOS and WebAssembly. The
//! golden-master tests hash the rendered audio bit-for-bit, so every
//! transcendental function on the render path is implemented here from IEEE
//! 754 basic operations (add, mul, div, floor, bit casts), which *are*
//! correctly rounded everywhere. Rust does not contract `a * b + c` into an
//! FMA, so the results are identical on every target. See ADR-0002.
//!
//! Accuracy is far beyond what a drum synth needs (relative error on the order
//! of `f32::EPSILON`), and the functions are cheaper than `libm` anyway.

pub use core::f32::consts::{LN_10, LN_2, TAU};

/// ln(1000): the exponent that takes a decaying envelope down by 60 dB.
pub const LN_1000: f32 = 6.907_755_279;

/// `e^x`, deterministic across platforms.
///
/// The input is clamped to `[-87, 88]` so the result stays finite and normal.
/// Relative error is about `2e-7` near zero (where envelope coefficients
/// live) and grows with `|x|` as the argument loses `f32` precision, to
/// about `1e-7 · |x|` further out.
#[inline]
#[must_use]
pub fn exp(x: f32) -> f32 {
    use core::f32::consts::LOG2_E;
    let x = x.clamp(-87.0, 88.0);
    let t = x * LOG2_E;
    // Round-to-nearest split keeps the fractional part in [-0.5, 0.5], where
    // the degree-6 Taylor series of 2^f is accurate to ~1e-7.
    let ti = (t + 0.5).floor();
    let f = t - ti;
    // Coefficients are ln(2)^k / k!.
    let p = 1.0
        + f * (LN_2
            + f * (0.240_226_507
                + f * (0.055_504_109
                    + f * (0.009_618_129 + f * (0.001_333_356 + f * 0.000_154_035)))));
    // 2^ti via the exponent field. ti is within [-126, 127] after clamping.
    let bits = ((ti as i32 + 127) as u32) << 23;
    f32::from_bits(bits) * p
}

/// `sin(2π · turns)`. The argument is a phase in turns (cycles), which is what
/// phase accumulators naturally produce; any finite value is accepted.
///
/// Maximum absolute error is a few `f32` ULPs (about `3e-7`).
#[inline]
#[must_use]
pub fn sin_turns(turns: f32) -> f32 {
    // Reduce to [-0.5, 0.5).
    let mut x = turns - (turns + 0.5).floor();
    // Fold onto [-0.25, 0.25] using sin(π - θ) = sin(θ).
    if x > 0.25 {
        x = 0.5 - x;
    } else if x < -0.25 {
        x = -0.5 - x;
    }
    let r = x * TAU; // [-π/2, π/2]
    let r2 = r * r;
    // Odd Taylor series through r^11; error at π/2 is ~6e-8.
    r * (1.0
        + r2 * (-1.0 / 6.0
            + r2 * (1.0 / 120.0
                + r2 * (-1.0 / 5_040.0 + r2 * (1.0 / 362_880.0 - r2 / 39_916_800.0)))))
}

/// `cos(2π · turns)`.
#[inline]
#[must_use]
pub fn cos_turns(turns: f32) -> f32 {
    sin_turns(turns + 0.25)
}

/// Smooth, monotonic soft clipper: identity near zero, saturating to exactly
/// ±1 at |x| ≥ 3. A rational approximation of `tanh` with unit slope at the
/// origin, so signals well below the knee pass through untouched.
#[inline]
#[must_use]
pub fn soft_clip(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    let x2 = x * x;
    x * (27.0 + x2) / (27.0 + 9.0 * x2)
}

/// Linear gain for a level in decibels.
#[inline]
#[must_use]
pub fn db_to_gain(db: f32) -> f32 {
    exp(db * (LN_10 / 20.0))
}

/// Per-sample multiplier for an exponential decay that falls 60 dB in
/// `t60_seconds`.
#[inline]
#[must_use]
pub fn decay_coefficient(t60_seconds: f32, sample_rate: f32) -> f32 {
    exp(-LN_1000 / (t60_seconds.max(1e-4) * sample_rate))
}

/// Per-sample multiplier for a one-pole exponential with time constant
/// `tau_seconds` (falls to 1/e in that time).
#[inline]
#[must_use]
pub fn tau_coefficient(tau_seconds: f32, sample_rate: f32) -> f32 {
    exp(-1.0 / (tau_seconds.max(1e-5) * sample_rate))
}

/// Coefficient for a one-pole low-pass / high-pass at `cutoff_hz`.
#[inline]
#[must_use]
pub fn onepole_coefficient(cutoff_hz: f32, sample_rate: f32) -> f32 {
    exp(-TAU * cutoff_hz / sample_rate)
}

/// Maps a normalised `0..=1` control to an exponential range starting at
/// `low` and ending at `low · e^ln_ratio` (equal ratios per unit of control
/// travel). `ln_ratio` is `ln(high / low)`, precomputed by the caller so the
/// render path never needs a logarithm.
#[inline]
#[must_use]
pub fn exp_range(control: f32, low: f32, ln_ratio: f32) -> f32 {
    low * exp(control.clamp(0.0, 1.0) * ln_ratio)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exp_matches_std_within_tolerance() {
        let mut x = -80.0f32;
        while x <= 80.0 {
            let ours = exp(x);
            let theirs = x.exp();
            let rel = ((ours - theirs) / theirs).abs();
            // Tolerance scales with |x|: the argument itself carries
            // ~|x| * f32::EPSILON of rounding before we start.
            let tol = 5e-7 + x.abs() * 1e-7;
            assert!(rel < tol, "exp({x}): ours={ours} std={theirs} rel={rel}");
            x += 0.037;
        }
        assert_eq!(exp(0.0), 1.0);
    }

    #[test]
    fn exp_is_monotonic_near_zero() {
        // The envelope coefficients live here; a bad split would make a
        // decaying envelope grow.
        let a = exp(-1e-5);
        let b = exp(-2e-5);
        assert!(a < 1.0 && b < a, "{a} {b}");
    }

    #[test]
    fn sin_matches_std_within_tolerance() {
        let mut t = -3.0f32;
        while t <= 3.0 {
            let ours = sin_turns(t);
            let theirs = (t as f64 * std::f64::consts::TAU).sin() as f32;
            assert!((ours - theirs).abs() < 5e-7, "sin({t}): {ours} vs {theirs}");
            t += 0.001_3;
        }
        assert_eq!(sin_turns(0.0), 0.0);
        assert!((sin_turns(0.25) - 1.0).abs() < 5e-7);
        assert!((cos_turns(0.0) - 1.0).abs() < 5e-7);
    }

    #[test]
    fn soft_clip_shape() {
        assert_eq!(soft_clip(0.0), 0.0);
        assert_eq!(soft_clip(3.0), 1.0);
        assert_eq!(soft_clip(-10.0), -1.0);
        // Unit slope at the origin.
        assert!((soft_clip(1e-3) / 1e-3 - 1.0).abs() < 1e-4);
        // Monotonic (the derivative is 9(x²−9)²/(27+9x²)² ≥ 0; allow one
        // ULP of noise where it flattens out at ±3).
        let mut prev = -1.0;
        let mut x = -3.0;
        while x <= 3.0 {
            let y = soft_clip(x);
            assert!(y >= prev - 1e-6, "soft_clip not monotonic at {x}");
            prev = y;
            x += 0.01;
        }
    }

    #[test]
    fn db_conversions() {
        assert!((db_to_gain(0.0) - 1.0).abs() < 1e-6);
        assert!((db_to_gain(-6.020_6) - 0.5).abs() < 1e-5);
        assert!((db_to_gain(-20.0) - 0.1).abs() < 1e-6);
    }

    #[test]
    fn decay_coefficient_reaches_minus_60_db() {
        let sr = 48_000.0;
        let c = decay_coefficient(0.5, sr);
        let mut env = 1.0f32;
        for _ in 0..24_000 {
            env *= c;
        }
        assert!((env - 1e-3).abs() < 1e-5, "{env}");
    }
}
