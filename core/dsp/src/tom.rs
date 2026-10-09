//! TR-inspired toms: one voice type, three ranges.
//!
//! The classic analogue tom is a close cousin of the bass drum: a damped
//! resonator kicked into oscillation by the trigger pulse, pushed briefly
//! sharp by that pulse (the "doom" glide), with a burst of low-passed noise
//! on top for the stick attack. This model keeps those ingredients:
//!
//! * **body** – a decaying sine whose frequency starts about 1.4–1.5× above
//!   the tuned pitch and glides down to it exponentially, settled within
//!   roughly 45 ms (high) to 65 ms (low);
//! * **attack** – a few milliseconds of white noise, enveloped and then
//!   low-passed by a two-pole state-variable filter, so its onset is smooth;
//! * **saturation** – a gentle soft clip after the mix. Velocity sets the
//!   level going *into* it, so accented hits are louder, glide further and
//!   are brighter (more saturation, brighter noise) – TR-style accent.
//!
//! Controls are normalised `0..=1`:
//!
//! | control | effect |
//! |---------|--------|
//! | `tune`  | settled pitch, exponential: Low 70–120 Hz, Mid 110–180 Hz, High 170–280 Hz |
//! | `decay` | body ring-down, 0.15–1.2 s to −60 dB (exponential) |
//! | `tone`  | attack brightness: noise low-pass 1.5–7.5 kHz (scaled per range and by velocity) and a little more noise |
//! | `level` | linear output level, smoothed over a few milliseconds |
//!
//! `snappy` is ignored. A full-velocity hit at `level = 1.0` peaks close to
//! −8 dBFS in every range.
//!
//! Retriggering a ringing tom (a flam, a fast fill) re-excites it instead of
//! restarting it: the phase runs on, the new strike's energy adds to what is
//! still ringing, and the amplitude moves to its new value over a fraction
//! of a millisecond, so there is no click. A hit from silence starts at a
//! zero crossing with full amplitude, for the sharpest attack.

use crate::blocks::{Noise, Svf};
use crate::math;
use crate::voice::Voice;
use crate::VoiceParams;

/// Which tom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TomRange {
    /// Floor tom (lowest).
    Low,
    /// Middle tom.
    Mid,
    /// High tom.
    High,
}

/// The constants that differ between ranges.
#[derive(Clone, Copy, Debug)]
struct RangeSpec {
    /// Settled pitch at `tune = 0`.
    tune_low_hz: f32,
    /// ln(top / bottom) of the tuning range.
    tune_ln_ratio: f32,
    /// Time constant of the pitch glide.
    sweep_tau_s: f32,
    /// Time constant of the noise attack.
    noise_tau_s: f32,
    /// Multiplier on the noise low-pass cutoff.
    noise_cutoff_scale: f32,
    /// Noise generator seed (distinct per range, so toms hit together do
    /// not share one noise sequence).
    noise_seed: u32,
}

const LOW: RangeSpec = RangeSpec {
    tune_low_hz: 70.0,
    // ln(120 / 70).
    tune_ln_ratio: 0.538_996_501,
    sweep_tau_s: 0.016,
    noise_tau_s: 0.003,
    noise_cutoff_scale: 0.8,
    noise_seed: 0x7A3C_51E1,
};

const MID: RangeSpec = RangeSpec {
    tune_low_hz: 110.0,
    // ln(180 / 110).
    tune_ln_ratio: 0.492_476_485,
    sweep_tau_s: 0.013,
    noise_tau_s: 0.0025,
    noise_cutoff_scale: 1.0,
    noise_seed: 0x2C91_D04B,
};

const HIGH: RangeSpec = RangeSpec {
    tune_low_hz: 170.0,
    // ln(280 / 170).
    tune_ln_ratio: 0.498_991_166,
    sweep_tau_s: 0.011,
    noise_tau_s: 0.002,
    noise_cutoff_scale: 1.25,
    noise_seed: 0x5E06_B87F,
};

impl TomRange {
    fn spec(self) -> &'static RangeSpec {
        match self {
            TomRange::Low => &LOW,
            TomRange::Mid => &MID,
            TomRange::High => &HIGH,
        }
    }
}

const DECAY_LOW_S: f32 = 0.15;
/// ln(1.2 / 0.15) = ln 8.
const DECAY_LN_RATIO: f32 = 2.079_441_542;

/// Pitch glide start, as a fraction of the settled pitch above it:
/// `base + velocity · extra` (1.3× at velocity 0, 1.5× at full accent).
const SWEEP_DEPTH_BASE: f32 = 0.3;
const SWEEP_DEPTH_VELOCITY: f32 = 0.2;

/// Noise low-pass cutoff at `tone = 0`, before range and velocity scaling.
const NOISE_CUTOFF_LOW_HZ: f32 = 1_500.0;
/// ln(7500 / 1500) = ln 5.
const NOISE_CUTOFF_LN_RATIO: f32 = 1.609_437_912;
/// The cutoff is scaled by `base + (1 − base) · velocity`.
const NOISE_CUTOFF_VELOCITY_BASE: f32 = 0.6;
/// Butterworth: no resonant peak, so the attack stays a breath, not a ping.
const NOISE_Q: f32 = 0.707;
/// Noise level relative to the body at `tone = 0.5` and full velocity.
/// `tone` moves it by ±25 %; lower velocities scale it by
/// `base + (1 − base) · velocity` on top of the body's own velocity curve,
/// so an accent brings the stick forward.
const NOISE_GAIN: f32 = 0.8;
const NOISE_GAIN_TONE: f32 = 0.5;
const NOISE_VELOCITY_BASE: f32 = 0.5;
/// Noise envelope level below which the attack path switches off.
const NOISE_OFF: f32 = 1e-6;

/// Velocity curve into the saturator: `v · (c + (1 − c) · v)`. Together with
/// the saturation this puts an unaccented hit (0.7) about 3 dB below a full
/// accent, and a flam grace note (0.42) about 10 dB below.
const VELOCITY_CURVE: f32 = 0.1;
/// Gain into the soft clipper for a full-scale body.
const DRIVE: f32 = 1.5;
/// `1 / soft_clip(DRIVE)`: a full-scale body leaves the saturator at 1.
const DRIVE_NORM: f32 = 1.076_923_077;
/// Output scaling so a full hit at `level = 1` peaks near −8 dBFS.
const CALIBRATION: f32 = 0.396;

/// Time constant of the amplitude ramp on a retrigger.
const RETRIGGER_TAU_S: f32 = 0.000_3;
/// Time constant of `level` changes.
const LEVEL_TAU_S: f32 = 0.005;

/// Largest settled phase increment (turns per sample); with the glide on
/// top it stays under 0.5.
const MAX_BASE_INC: f32 = 0.3;

/// Body envelope level below which the voice goes idle (−100 dB).
const IDLE_THRESHOLD: f32 = 1e-5;
/// Small states are flushed to zero below this, keeping clear of denormals.
const FLUSH: f32 = 1e-7;

/// A tom of one [`TomRange`]. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct Tom {
    sample_rate: f32,
    range: TomRange,
    spec: &'static RangeSpec,
    params: VoiceParams,

    // Derived per sample rate.
    sweep_coef: f32,
    noise_coef: f32,
    ramp_coef: f32,
    level_coef: f32,

    // Derived per trigger.
    base_inc: f32,
    amp_coef: f32,

    // State.
    active: bool,
    phase: f32,
    /// Body amplitude the envelope is heading along.
    amp_env: f32,
    /// What is still to be added to reach `amp_env` after a retrigger; the
    /// body plays `amp_env − ramp`.
    ramp: f32,
    pitch_env: f32,
    noise_env: f32,
    noise: Noise,
    noise_lp: Svf,
    /// Smoothed `params.level`.
    level: f32,
}

impl Tom {
    /// Creates an idle tom of the given range.
    #[must_use]
    pub fn new(sample_rate: f32, range: TomRange) -> Self {
        let spec = range.spec();
        let params = VoiceParams::default();
        let mut tom = Self {
            sample_rate,
            range,
            spec,
            params,
            sweep_coef: 0.0,
            noise_coef: 0.0,
            ramp_coef: 0.0,
            level_coef: 0.0,
            base_inc: 0.0,
            amp_coef: 0.0,
            active: false,
            phase: 0.0,
            amp_env: 0.0,
            ramp: 0.0,
            pitch_env: 0.0,
            noise_env: 0.0,
            noise: Noise::new(spec.noise_seed),
            noise_lp: Svf::default(),
            level: params.level,
        };
        tom.set_sample_rate(sample_rate);
        tom
    }

    /// The tom's range.
    #[must_use]
    pub fn range(&self) -> TomRange {
        self.range
    }

    /// Current controls (clamped to `0..=1`).
    #[must_use]
    pub fn params(&self) -> VoiceParams {
        self.params
    }

    /// Body frequency (Hz) the current `tune` resolves to once the glide has
    /// settled.
    #[must_use]
    pub fn tuned_frequency_hz(&self) -> f32 {
        math::exp_range(
            self.params.tune,
            self.spec.tune_low_hz,
            self.spec.tune_ln_ratio,
        )
    }

    /// Body ring-down time (seconds to −60 dB) the current `decay` resolves
    /// to.
    #[must_use]
    pub fn decay_seconds(&self) -> f32 {
        math::exp_range(self.params.decay, DECAY_LOW_S, DECAY_LN_RATIO)
    }

    /// Low-pass cutoff of the noise attack for the current `tone` at the
    /// given velocity.
    fn noise_cutoff_hz(&self, velocity: f32) -> f32 {
        math::exp_range(self.params.tone, NOISE_CUTOFF_LOW_HZ, NOISE_CUTOFF_LN_RATIO)
            * self.spec.noise_cutoff_scale
            * (NOISE_CUTOFF_VELOCITY_BASE + (1.0 - NOISE_CUTOFF_VELOCITY_BASE) * velocity)
    }

    /// Silences the voice and clears every state that carries sound. The
    /// noise generator runs on, so consecutive hits differ slightly.
    fn reset_state(&mut self) {
        self.active = false;
        self.phase = 0.0;
        self.amp_env = 0.0;
        self.ramp = 0.0;
        self.pitch_env = 0.0;
        self.noise_env = 0.0;
        self.noise_lp.reset();
        self.level = self.params.level;
    }
}

/// `x` clamped to `0..=1`, with non-finite values mapped to 0.
fn unit(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

impl Voice for Tom {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.sweep_coef = math::tau_coefficient(self.spec.sweep_tau_s, self.sample_rate);
        self.noise_coef = math::tau_coefficient(self.spec.noise_tau_s, self.sample_rate);
        self.ramp_coef = math::tau_coefficient(RETRIGGER_TAU_S, self.sample_rate);
        self.level_coef = 1.0 - math::tau_coefficient(LEVEL_TAU_S, self.sample_rate);
        self.reset_state();
    }

    fn apply_params(&mut self, params: &VoiceParams) {
        self.params = VoiceParams {
            tune: unit(params.tune),
            decay: unit(params.decay),
            tone: unit(params.tone),
            snappy: unit(params.snappy),
            level: unit(params.level),
        };
        if !self.active {
            self.level = self.params.level;
        }
    }

    fn trigger(&mut self, velocity: f32) {
        if velocity.is_nan() || velocity <= 0.0 {
            return;
        }
        let velocity = velocity.min(1.0);
        let strike = velocity * (VELOCITY_CURVE + (1.0 - VELOCITY_CURVE) * velocity);

        // The clamp only matters at absurdly low sample rates; it keeps the
        // swept increment below Nyquist and the phase wrap a single step.
        self.base_inc = (self.tuned_frequency_hz() / self.sample_rate).min(MAX_BASE_INC);
        self.amp_coef = math::decay_coefficient(self.decay_seconds(), self.sample_rate);
        self.noise_lp
            .set(self.noise_cutoff_hz(velocity), NOISE_Q, self.sample_rate);
        let noise_level = strike
            * NOISE_GAIN
            * (1.0 - 0.5 * NOISE_GAIN_TONE + NOISE_GAIN_TONE * self.params.tone)
            * (NOISE_VELOCITY_BASE + (1.0 - NOISE_VELOCITY_BASE) * velocity);
        let depth = SWEEP_DEPTH_BASE + SWEEP_DEPTH_VELOCITY * velocity;

        if self.active {
            // Re-excite the ringing body: the strike adds energy to what is
            // left, the phase runs on, and the amplitude ramps over to the
            // new value from where it is now, so the waveform stays
            // continuous.
            let current = self.amp_env - self.ramp;
            let target = (current * current + strike * strike).sqrt().min(1.0);
            self.amp_env = target;
            self.ramp = target - current;
            self.pitch_env = self.pitch_env.max(depth);
            self.noise_env = self.noise_env.max(noise_level);
        } else {
            // From silence: start at a zero crossing at full amplitude.
            self.phase = 0.0;
            self.amp_env = strike;
            self.ramp = 0.0;
            self.pitch_env = depth;
            self.noise_env = noise_level;
            self.noise_lp.reset();
            self.level = self.params.level;
            self.active = true;
        }
    }

    #[inline]
    fn process(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }

        // Body: a sine read before the phase advances, so a hit from silence
        // starts exactly at zero.
        let body = math::sin_turns(self.phase) * (self.amp_env - self.ramp);
        self.phase += self.base_inc * (1.0 + self.pitch_env);
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        // Attack: enveloped white noise through the low-pass.
        let noise = if self.noise_env > 0.0 {
            let n = self
                .noise_lp
                .process(self.noise.tick() * self.noise_env)
                .low;
            self.noise_env *= self.noise_coef;
            if self.noise_env < NOISE_OFF {
                self.noise_env = 0.0;
                self.noise_lp.reset();
            }
            n
        } else {
            0.0
        };

        let shaped = math::soft_clip((body + noise) * DRIVE) * DRIVE_NORM;

        // Level glides to its target; flush once there.
        let level_error = self.params.level - self.level;
        if level_error.abs() < FLUSH {
            self.level = self.params.level;
        } else {
            self.level += level_error * self.level_coef;
        }
        let out = shaped * self.level * CALIBRATION;

        // Advance the body envelopes.
        self.amp_env *= self.amp_coef;
        if self.ramp != 0.0 {
            self.ramp *= self.ramp_coef;
            if self.ramp.abs() < FLUSH {
                self.ramp = 0.0;
            }
        }
        if self.pitch_env != 0.0 {
            self.pitch_env *= self.sweep_coef;
            if self.pitch_env < FLUSH {
                self.pitch_env = 0.0;
            }
        }
        if self.amp_env < IDLE_THRESHOLD && self.noise_env == 0.0 {
            self.reset_state();
        }

        out
    }

    #[inline]
    fn is_active(&self) -> bool {
        self.active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RANGES: [TomRange; 3] = [TomRange::Low, TomRange::Mid, TomRange::High];

    fn tom_with(sr: f32, range: TomRange, f: impl FnOnce(&mut VoiceParams)) -> Tom {
        let mut tom = Tom::new(sr, range);
        let mut p = VoiceParams::default();
        f(&mut p);
        tom.apply_params(&p);
        tom
    }

    fn render(tom: &mut Tom, n: usize) -> Vec<f32> {
        (0..n).map(|_| tom.process()).collect()
    }

    fn hit(sr: f32, range: TomRange, velocity: f32, f: impl FnOnce(&mut VoiceParams)) -> Vec<f32> {
        let mut tom = tom_with(sr, range, f);
        tom.trigger(velocity);
        render(&mut tom, (sr * 1.0) as usize)
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    fn db(x: f32) -> f32 {
        20.0 * x.log10()
    }

    fn energy(samples: &[f32]) -> f64 {
        samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum()
    }

    /// Times (in samples, linearly interpolated) of upward zero crossings.
    fn rising_crossings(samples: &[f32]) -> Vec<f64> {
        samples
            .windows(2)
            .enumerate()
            .filter(|(_, w)| w[0] < 0.0 && w[1] >= 0.0)
            .map(|(i, w)| {
                let (a, b) = (f64::from(w[0]), f64::from(w[1]));
                i as f64 + a / (a - b)
            })
            .collect()
    }

    /// Mean frequency over a window, from its first to last rising zero
    /// crossing.
    fn frequency(samples: &[f32], sr: f32) -> f64 {
        let c = rising_crossings(samples);
        assert!(c.len() >= 3, "too few crossings");
        (c.len() - 1) as f64 * f64::from(sr) / (c[c.len() - 1] - c[0])
    }

    /// `(frequency, power)` per bin of a Hann-windowed DFT (DC excluded).
    fn power_spectrum(samples: &[f32], sr: f32) -> Vec<(f64, f64)> {
        let n = samples.len();
        let w: Vec<f64> = samples
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                let hann = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
                f64::from(s) * hann
            })
            .collect();
        (1..n / 2)
            .map(|k| {
                let (mut re, mut im) = (0.0, 0.0);
                for (i, x) in w.iter().enumerate() {
                    let ph = std::f64::consts::TAU * ((k * i) % n) as f64 / n as f64;
                    re += x * ph.cos();
                    im -= x * ph.sin();
                }
                (k as f64 * f64::from(sr) / n as f64, re * re + im * im)
            })
            .collect()
    }

    /// Power-weighted mean frequency over the bins at or above `min_hz`.
    fn spectral_centroid(samples: &[f32], sr: f32, min_hz: f64) -> f64 {
        let (num, den) = power_spectrum(samples, sr)
            .into_iter()
            .filter(|&(f, _)| f >= min_hz)
            .fold((0.0, 0.0), |(n, d), (f, p)| (n + f * p, d + p));
        num / den
    }

    /// Fraction of the power at or above `split_hz`.
    fn power_above(samples: &[f32], sr: f32, split_hz: f64) -> f64 {
        let spectrum = power_spectrum(samples, sr);
        let total: f64 = spectrum.iter().map(|&(_, p)| p).sum();
        let high: f64 = spectrum
            .iter()
            .filter(|&&(f, _)| f >= split_hz)
            .map(|&(_, p)| p)
            .sum();
        high / total
    }

    #[test]
    fn drive_norm_matches_drive() {
        assert!((DRIVE_NORM * math::soft_clip(DRIVE) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn idle_voice_is_silent() {
        for range in RANGES {
            let mut tom = Tom::new(48_000.0, range);
            assert!(!tom.is_active());
            assert!(render(&mut tom, 1_000).iter().all(|&s| s == 0.0));
            tom.trigger(0.0);
            assert!(!tom.is_active());
            tom.trigger(f32::NAN);
            assert!(!tom.is_active());
            assert!(render(&mut tom, 1_000).iter().all(|&s| s == 0.0));
        }
    }

    #[test]
    fn full_hit_peaks_near_minus_8_dbfs() {
        for range in RANGES {
            for sr in crate::SUPPORTED_SAMPLE_RATES {
                for tune in [0.0, 0.5, 1.0] {
                    let out = hit(sr, range, 1.0, |p| p.tune = tune);
                    let d = db(peak(&out));
                    assert!(
                        (-9.5..=-6.5).contains(&d),
                        "{range:?} at {sr} Hz, tune {tune}: peak {d} dBFS"
                    );
                }
            }
        }
    }

    #[test]
    fn output_is_finite_and_bounded() {
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            for range in RANGES {
                for combo in 0..32u32 {
                    let bit = |b: u32| if combo & (1 << b) != 0 { 1.0 } else { 0.0 };
                    let params = VoiceParams {
                        tune: bit(0),
                        decay: bit(1),
                        tone: bit(2),
                        snappy: bit(3),
                        level: bit(4),
                    };
                    for velocity in [0.1, 0.7, 1.0] {
                        let mut tom = Tom::new(sr, range);
                        tom.apply_params(&params);
                        tom.trigger(velocity);
                        let mut out = render(&mut tom, (sr * 0.01) as usize);
                        // Hammer it: retriggers pile energy into the body.
                        for _ in 0..8 {
                            tom.trigger(1.0);
                            out.extend(render(&mut tom, (sr * 0.008) as usize));
                        }
                        out.extend(render(&mut tom, (sr * 0.1) as usize));
                        for s in out {
                            assert!(
                                s.is_finite() && s.abs() <= 1.0,
                                "{range:?} {sr} {params:?} v{velocity}: {s}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn decays_to_silence_and_goes_idle() {
        for range in RANGES {
            for (decay, limit_s) in [(0.0, 0.4), (1.0, 2.4)] {
                let sr = 48_000.0;
                let mut tom = tom_with(sr, range, |p| p.decay = decay);
                tom.trigger(1.0);
                let n = (sr * limit_s) as usize;
                let out = render(&mut tom, n);
                assert!(!tom.is_active(), "{range:?} decay {decay} still active");
                let idle_at = out.iter().rposition(|&s| s != 0.0).unwrap() + 1;
                assert!(out[idle_at - 1].abs() < 1e-5, "{}", out[idle_at - 1]);
                assert!(out[idle_at..].iter().all(|&s| s == 0.0));
                assert!(render(&mut tom, 4_800).iter().all(|&s| s == 0.0));
                // Every state that could linger is cleared.
                assert_eq!(tom.amp_env, 0.0);
                assert_eq!(tom.noise_env, 0.0);
                assert_eq!(tom.pitch_env, 0.0);
                assert_eq!(tom.ramp, 0.0);
            }
        }
    }

    #[test]
    fn small_states_flush_to_zero_while_ringing() {
        for range in RANGES {
            let mut tom = tom_with(48_000.0, range, |p| p.decay = 1.0);
            tom.trigger(1.0);
            render(&mut tom, 24_000);
            assert!(tom.is_active());
            assert_eq!(tom.noise_env, 0.0);
            assert_eq!(tom.pitch_env, 0.0);
        }
    }

    #[test]
    fn decay_sets_ring_time() {
        for range in RANGES {
            let tail = |decay: f32| {
                let out = hit(48_000.0, range, 1.0, |p| p.decay = decay);
                energy(&out[4_800..7_200])
            };
            let (short, mid, long) = (tail(0.0), tail(0.5), tail(1.0));
            assert!(
                long > mid * 4.0 && mid > short * 4.0,
                "{short} {mid} {long}"
            );

            // In the linear tail the envelope falls at the nominal rate.
            let sr = 48_000.0;
            let tom = tom_with(sr, range, |p| p.decay = 0.5);
            let t60 = tom.decay_seconds();
            let out = hit(sr, range, 1.0, |p| p.decay = 0.5);
            let at = |t: f32| {
                let i = (t * sr) as usize;
                peak(&out[i..i + 1_200])
            };
            let fall_db = db(at(0.2)) - db(at(0.4));
            let expect = 60.0 * 0.2 / t60;
            assert!(
                (fall_db - expect).abs() < expect * 0.1,
                "{range:?}: fell {fall_db} dB in 0.2 s, expected {expect}"
            );
        }
    }

    /// Settled pitch, measured well after the glide.
    fn settled_frequency(sr: f32, range: TomRange, tune: f32) -> f64 {
        let out = hit(sr, range, 1.0, |p| {
            p.tune = tune;
            p.decay = 1.0;
        });
        let from = (sr * 0.15) as usize;
        let to = (sr * 0.45) as usize;
        frequency(&out[from..to], sr)
    }

    #[test]
    fn tune_maps_to_expected_frequencies() {
        let table = [
            (TomRange::Low, [70.0, 91.652, 120.0]),
            (TomRange::Mid, [110.0, 140.712, 180.0]),
            (TomRange::High, [170.0, 218.174, 280.0]),
        ];
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            for (range, expect) in table {
                for (tune, want) in [0.0, 0.5, 1.0].into_iter().zip(expect) {
                    let f = settled_frequency(sr, range, tune);
                    assert!(
                        (f / want - 1.0).abs() < 0.005,
                        "{range:?} tune {tune} at {sr}: {f} Hz, expected {want}"
                    );
                    let nominal = tom_with(sr, range, |p| p.tune = tune).tuned_frequency_hz();
                    assert!((f64::from(nominal) / want - 1.0).abs() < 1e-4);
                }
            }
        }
    }

    #[test]
    fn ranges_are_ordered_at_equal_tune() {
        for tune in [0.0, 0.5, 1.0] {
            let low = settled_frequency(48_000.0, TomRange::Low, tune);
            let mid = settled_frequency(48_000.0, TomRange::Mid, tune);
            let high = settled_frequency(48_000.0, TomRange::High, tune);
            assert!(low < mid && mid < high, "tune {tune}: {low} {mid} {high}");
        }
    }

    #[test]
    fn pitch_glides_down_and_settles() {
        let sr = 48_000.0;
        for range in RANGES {
            let out = hit(sr, range, 1.0, |p| p.decay = 1.0);
            let settled = settled_frequency(sr, range, 0.5);
            // The first half cycle starts at sample 0 (a rising zero
            // crossing); time it to the first falling crossing.
            let half = out
                .windows(2)
                .position(|w| w[0] > 0.0 && w[1] <= 0.0)
                .map(|i| {
                    let (a, b) = (f64::from(out[i]), f64::from(out[i + 1]));
                    i as f64 + a / (a - b)
                })
                .unwrap();
            let early = 0.5 * f64::from(sr) / half;
            let ratio = early / settled;
            assert!(
                (1.3..=1.6).contains(&ratio),
                "{range:?}: starts at {ratio}x the settled pitch"
            );
            // Settled within 80 ms.
            let late = frequency(&out[(sr * 0.08) as usize..(sr * 0.18) as usize], sr);
            assert!(
                (late / settled - 1.0).abs() < 0.01,
                "{range:?}: {late} Hz at 80 ms vs settled {settled}"
            );
            // Still clearly gliding over the first 30 ms.
            let early_mean = frequency(&out[..(sr * 0.03) as usize], sr);
            assert!(
                early_mean > settled * 1.05,
                "{range:?}: {early_mean} vs {settled}"
            );
        }
    }

    /// The first ~10 ms of a hit, centred in a 1024-sample frame so the
    /// analysis window does not taper the attack away.
    fn attack_frame(out: &[f32]) -> Vec<f32> {
        let mut frame = vec![0.0; 512];
        frame.extend_from_slice(&out[..512]);
        frame
    }

    /// Spectral centroid of the attack, over bins from `min_hz` up. Above
    /// 1 kHz the body's fundamental and low harmonics drop out and the
    /// attack's colour shows.
    fn attack_centroid(range: TomRange, velocity: f32, tone: f32, min_hz: f64) -> f64 {
        let sr = 48_000.0;
        let out = hit(sr, range, velocity, |p| p.tone = tone);
        spectral_centroid(&attack_frame(&out), sr, min_hz)
    }

    #[test]
    fn tone_brightens_the_attack() {
        for range in RANGES {
            let dark = attack_centroid(range, 1.0, 0.0, 1_000.0);
            let mid = attack_centroid(range, 1.0, 0.5, 1_000.0);
            let bright = attack_centroid(range, 1.0, 1.0, 1_000.0);
            assert!(
                bright > mid * 1.1 && mid > dark * 1.1,
                "{range:?}: {dark} {mid} {bright}"
            );
            // Whole-band centroid moves the same way, if only a little:
            // the body dominates the energy.
            let dark = attack_centroid(range, 1.0, 0.0, 0.0);
            let bright = attack_centroid(range, 1.0, 1.0, 0.0);
            assert!(bright > dark, "{range:?}: {dark} {bright}");
        }
    }

    #[test]
    fn velocity_raises_level_and_brightness() {
        for range in RANGES {
            let p = |v: f32| peak(&hit(48_000.0, range, v, |_| {}));
            let (full, normal, grace) = (p(1.0), p(0.7), p(0.42));
            let accent_db = db(full) - db(normal);
            assert!(
                (2.0..=5.0).contains(&accent_db),
                "{range:?}: accent adds {accent_db} dB"
            );
            assert!(grace < normal * 0.6, "{range:?}: {grace} vs {normal}");
            // Accent glides further (whole-band centroid rises), saturates
            // harder and opens the attack filter (more power above 1 kHz).
            let soft = attack_centroid(range, 0.4, 0.5, 0.0);
            let hard = attack_centroid(range, 1.0, 0.5, 0.0);
            assert!(hard > soft * 1.03, "{range:?}: centroid {soft} vs {hard}");
            let hf = |v: f32| {
                let out = hit(48_000.0, range, v, |_| {});
                power_above(&attack_frame(&out), 48_000.0, 1_000.0)
            };
            let (soft, hard) = (hf(0.4), hf(1.0));
            assert!(hard > soft * 1.5, "{range:?}: HF share {soft} vs {hard}");
        }
    }

    #[test]
    fn level_scales_output_and_snappy_is_ignored() {
        for range in RANGES {
            let full = hit(48_000.0, range, 1.0, |_| {});
            let half = hit(48_000.0, range, 1.0, |p| p.level = 0.5);
            for (a, b) in full.iter().zip(&half) {
                assert!((a * 0.5 - b).abs() < 1e-6);
            }
            let silent = hit(48_000.0, range, 1.0, |p| p.level = 0.0);
            assert!(silent.iter().all(|&s| s == 0.0));
            let snappy = hit(48_000.0, range, 1.0, |p| p.snappy = 1.0);
            assert_eq!(full, snappy);
        }
    }

    #[test]
    fn level_change_mid_ring_is_smooth() {
        for range in RANGES {
            let mut tom = Tom::new(48_000.0, range);
            tom.trigger(1.0);
            let before = render(&mut tom, 4_800);
            tom.apply_params(&VoiceParams {
                level: 0.0,
                ..VoiceParams::default()
            });
            let after = render(&mut tom, 4_800);
            let jump = (after[0] - before[before.len() - 1]).abs();
            let swing = max_step(&before[before.len() - 960..]);
            assert!(jump <= swing * 1.1, "{range:?}: {jump} vs {swing}");
            assert!(peak(&after[2_400..]) < 1e-3 * peak(&before));
        }
    }

    fn max_step(samples: &[f32]) -> f32 {
        samples
            .windows(2)
            .fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()))
    }

    #[test]
    fn retrigger_mid_ring_has_no_discontinuity() {
        let sr = 48_000.0;
        for range in RANGES {
            let fresh = hit(sr, range, 1.0, |_| {});
            let (fresh_peak, fresh_step) = (peak(&fresh), max_step(&fresh));
            let mut hard_reset_worst = 0.0f32;
            for (first, second) in [(0.42, 0.7), (0.6, 1.0), (1.0, 1.0), (1.0, 0.42)] {
                for at_ms in [3.0, 8.0, 13.0, 21.0, 40.0, 77.0, 150.0] {
                    let mut tom = Tom::new(sr, range);
                    tom.trigger(first);
                    let at = (sr * at_ms / 1_000.0) as usize;
                    let mut out = render(&mut tom, at);
                    // What the ring would have done next without the hit,
                    // and what a restart from scratch would do.
                    let carry_on = tom.clone().process();
                    let mut restarted = tom.clone();
                    restarted.reset_state();
                    restarted.trigger(second);
                    hard_reset_worst = hard_reset_worst.max((restarted.process() - carry_on).abs());

                    tom.trigger(second);
                    out.extend(render(&mut tom, 4_800));
                    // The first retriggered sample only departs from the
                    // ring by the onset of the new attack.
                    let deviation = (out[at] - carry_on).abs();
                    let around = max_step(&out[at - 1..at + 480]);
                    assert!(
                        deviation < 0.15 * fresh_peak && around <= fresh_step * 2.0,
                        "{range:?} {first}->{second} at {at_ms} ms: deviation \
                         {deviation}, steepest step {around} (fresh hit {fresh_step})"
                    );
                }
            }
            // The measure would catch a hard restart.
            assert!(
                hard_reset_worst > 0.3 * fresh_peak,
                "{range:?}: {hard_reset_worst}"
            );
        }
    }

    #[test]
    fn flam_stays_within_a_full_hit() {
        let sr = 48_000.0;
        for range in RANGES {
            let mut tom = Tom::new(sr, range);
            tom.trigger(0.6);
            let mut out = render(&mut tom, 960);
            tom.trigger(1.0);
            out.extend(render(&mut tom, 24_000));
            let single = peak(&hit(sr, range, 1.0, |_| {}));
            let p = peak(&out);
            assert!(
                (single * 0.9..=single * 1.05).contains(&p),
                "{range:?}: {p} vs {single}"
            );
        }
    }

    #[test]
    fn no_dc_offset() {
        for range in RANGES {
            for tune in [0.0, 1.0] {
                let out = hit(48_000.0, range, 1.0, |p| p.tune = tune);
                let mean = out.iter().map(|&s| f64::from(s)).sum::<f64>() / out.len() as f64;
                assert!(
                    mean.abs() < 3e-3 * f64::from(peak(&out)),
                    "{range:?} tune {tune}: mean {mean}"
                );
            }
        }
    }

    #[test]
    fn renders_are_deterministic() {
        for range in RANGES {
            let run = || {
                let mut tom = tom_with(48_000.0, range, |p| p.tone = 0.8);
                tom.trigger(0.7);
                let mut out = render(&mut tom, 1_000);
                tom.trigger(1.0);
                out.extend(render(&mut tom, 30_000));
                out
            };
            let (a, b) = (run(), run());
            assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
        }
    }

    #[test]
    fn ten_seconds_render_quickly() {
        let sr = 48_000.0;
        for range in RANGES {
            let mut tom = tom_with(sr, range, |p| p.decay = 1.0);
            let start = std::time::Instant::now();
            let mut acc = 0.0f32;
            for i in 0..(10 * 48_000) {
                if i % 6_000 == 0 {
                    tom.trigger(if i % 12_000 == 0 { 1.0 } else { 0.7 });
                }
                acc += tom.process().abs();
            }
            let elapsed = start.elapsed();
            assert!(acc.is_finite() && acc > 0.0);
            assert!(
                elapsed.as_millis() < 100,
                "{range:?}: 10 s took {elapsed:?}"
            );
        }
    }

    #[test]
    fn attack_noise_sits_under_the_body() {
        // Split the first 10 ms of a full hit into its two parts by
        // silencing the other one, and compare their energies (dB).
        let noise_to_body = |range: TomRange, tone: f32| {
            let mut a = tom_with(48_000.0, range, |p| p.tone = tone);
            a.trigger(1.0);
            a.amp_env = 0.0;
            let noise = energy(&render(&mut a, 480));
            let mut b = tom_with(48_000.0, range, |p| p.tone = tone);
            b.trigger(1.0);
            b.noise_env = 0.0;
            let body = energy(&render(&mut b, 480));
            10.0 * (noise / body).log10()
        };
        for range in RANGES {
            let dark = noise_to_body(range, 0.0);
            let mid = noise_to_body(range, 0.5);
            let bright = noise_to_body(range, 1.0);
            assert!((-22.0..=-14.0).contains(&mid), "{range:?}: {mid} dB");
            assert!(
                dark < mid - 2.0 && mid < bright - 2.0 && bright < -10.0,
                "{range:?}: {dark} {mid} {bright} dB"
            );
        }
    }
}
