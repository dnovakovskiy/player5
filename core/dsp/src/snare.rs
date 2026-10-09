//! TR-inspired snare drum.
//!
//! The classic analogue snare is two small resonators (the drum's shell
//! modes) struck by the trigger pulse, plus a burst of filtered white noise
//! standing in for the snare wires. This model keeps that structure:
//!
//! * **body** – two sines a little less than an octave apart (180 Hz and
//!   330 Hz at the centre of `tune`). Each starts sharp and drops onto its
//!   pitch within about 20 ms (the pitch blip of a struck resonator) and
//!   decays exponentially, the upper mode faster than the lower one. Their
//!   sum goes through a soft clip whose drive follows velocity, so accented
//!   hits are denser, not just louder.
//! * **snares** – white noise, high-passed (two-pole, 0.9–2 kHz) to keep it
//!   out of the body's range, then low-passed (two-pole, 3.5–14 kHz) for
//!   colour. The filtered noise is scaled to a fixed RMS (so `tone` changes
//!   its colour, not its level, and the drum sounds the same at 44.1, 48 and
//!   96 kHz) and soft-clipped, which rounds off its tallest peaks. Its
//!   envelope is a short snap on top of an exponential tail whose length
//!   follows `decay`.
//!
//! Controls (all `0..=1`):
//!
//! * `tune` – body pitch, ±6 semitones around 180 / 330 Hz;
//! * `decay` – noise tail, 0.08–0.4 s to −60 dB (the body lengthens a little
//!   with it);
//! * `tone` – low settings give a fuller lower body mode and darker noise,
//!   high settings a thinner body and brighter, crisper noise;
//! * `snappy` – amount of noise, linear in amplitude, from none (a pure
//!   two-mode body) to wire-heavy (6 dB more wires than the default 0.5);
//! * `level` – output level. It applies immediately, smoothed over a few
//!   milliseconds so moving it mid-hit never clicks; a new hit starts at
//!   the current level.
//!
//! `tune`, `decay`, `tone` and `snappy` take effect on the next hit. A hit
//! with velocity 0 is ignored.
//!
//! Accent (velocity): louder, more body drive, relatively more and brighter
//! noise and a sharper snap. A full-velocity hit at default controls and
//! `level = 1.0` peaks close to −9 dBFS, leaving room for the kick in a full
//! kit.
//!
//! Retriggering while the drum still rings (flams, rolls) restarts the hit
//! cleanly: the previous output is carried as an offset that fades out over
//! a couple of milliseconds, so the waveform never jumps. A safety knee at
//! the output (exactly transparent below −3 dBFS) keeps even the most
//! extreme settings inside full scale without a hard clip.

use crate::blocks::{Noise, Svf};
use crate::math;
use crate::voice::Voice;
use crate::VoiceParams;

/// Lower body mode at `tune = 0.5`.
const LOW_BODY_HZ: f32 = 180.0;
/// Upper body mode at `tune = 0.5`.
const HIGH_BODY_HZ: f32 = 330.0;
/// Pitch multiplier at `tune = 0`: 2^(−6/12).
const TUNE_LOW_RATIO: f32 = core::f32::consts::FRAC_1_SQRT_2;
/// ln(2): the full `tune` travel spans one octave (±6 semitones).
const TUNE_LN_RATIO: f32 = math::LN_2;
/// Phase increments are kept below this many turns per sample.
const MAX_INC: f32 = 0.45;
/// Sample rates are clamped to `1..=MAX_SAMPLE_RATE` so every derived
/// coefficient stays finite.
const MAX_SAMPLE_RATE: f32 = 1.0e6;

/// Body ring-down (−60 dB) at `decay = 0.5`, lower and upper mode.
const LOW_BODY_T60_S: f32 = 0.2;
const HIGH_BODY_T60_S: f32 = 0.11;
/// The body lengthens with `decay` by a factor of 0.8 → 1.25.
const BODY_DECAY_LOW: f32 = 0.8;
/// ln(1.25 / 0.8).
const BODY_DECAY_LN_RATIO: f32 = 0.446_287_103;
/// Body mode levels: the lower mode thins out as `tone` rises.
const LOW_BODY_GAIN: f32 = 1.0;
const LOW_BODY_TONE_CUT: f32 = 0.45;
const HIGH_BODY_GAIN: f32 = 0.5;

/// Depth of the pitch blip at full velocity, as a fraction of the tuned
/// pitch (lower, upper mode); half of it at velocity 0.
const LOW_BLIP_DEPTH: f32 = 0.30;
const HIGH_BLIP_DEPTH: f32 = 0.22;
/// Time constant of the pitch blip.
const BLIP_TAU_S: f32 = 0.006;

/// Body saturation drive at velocity 0 and the extra drive at velocity 1.
const DRIVE_BASE: f32 = 1.0;
const DRIVE_VELOCITY: f32 = 0.5;

/// Noise tail (−60 dB): 0.08 s at `decay = 0`, 0.4 s at `decay = 1`.
const NOISE_T60_LOW_S: f32 = 0.08;
/// ln(0.4 / 0.08).
const NOISE_T60_LN_RATIO: f32 = 1.609_437_912;
/// Noise level at `snappy = 1`, velocity 1 (after the soft clip, whose
/// output RMS is about [`NOISE_DRIVE_RMS`]). `snappy` scales it linearly, so
/// the default `snappy = 0.5` sits 6 dB below the wire-heavy maximum.
const NOISE_GAIN: f32 = 1.8;
/// Share of the noise level that does not depend on velocity (on top of the
/// overall velocity gain), so accents tilt the balance towards the wires.
const NOISE_VELOCITY_FLOOR: f32 = 0.75;
/// The snap: a fast extra burst at the start of the noise.
const SNAP_GAIN: f32 = 0.9;
const SNAP_TAU_S: f32 = 0.004;
/// Share of the snap that does not depend on velocity.
const SNAP_VELOCITY_FLOOR: f32 = 0.4;

/// Noise high-pass: 900 Hz at `tone = 0`, 2 kHz at `tone = 1`.
const NOISE_HP_LOW_HZ: f32 = 900.0;
/// ln(2000 / 900).
const NOISE_HP_LN_RATIO: f32 = 0.798_507_696;
const NOISE_HP_Q: f32 = 0.707;
/// Noise low-pass: 3.5 kHz at `tone = 0`, 14 kHz at `tone = 1`, before the
/// velocity brightening below.
const NOISE_LP_LOW_HZ: f32 = 3_500.0;
/// ln(14000 / 3500).
const NOISE_LP_LN_RATIO: f32 = 1.386_294_361;
/// A touch of resonance gives the wires some presence.
const NOISE_LP_Q: f32 = 0.9;
/// Low-pass cutoff multiplier `BASE + VELOCITY · v` (≈ 1 unaccented).
const BRIGHT_BASE: f32 = 0.8;
const BRIGHT_VELOCITY: f32 = 0.3;
/// Keeps the low-pass well below Nyquist at every sample rate.
const NOISE_LP_MAX_NYQUIST: f32 = 0.42;
/// Noise-equivalent bandwidth of the two-pole low-pass, per hertz of
/// cutoff: `π · Q / 2`.
const NOISE_LP_ENBW: f32 = 1.413_716_694;
/// Bandwidth the two-pole Butterworth high-pass removes from white noise,
/// per hertz of cutoff: `π / (2 √2)`.
const NOISE_HP_ENBW: f32 = 1.110_720_735;
/// The filtered noise is scaled to this RMS before its soft clip. The clip
/// barely touches typical samples (−0.6 dB at 1σ) but rounds off the tall
/// peaks (−2 dB at 2σ), which steadies the hit-to-hit peak level, bounds
/// the noise path and adds a little analogue grit.
const NOISE_DRIVE_RMS: f32 = 0.5;
/// Seed of the noise generator (reset with the sample rate).
const NOISE_SEED: u32 = 0x2545_F491;

/// Time constant of the retrigger declick offset.
const DECLICK_TAU_S: f32 = 0.002;
/// Time constant of the `level` smoother.
const LEVEL_TAU_S: f32 = 0.005;
/// Above this magnitude the output bends smoothly towards ±1 (−3.1 dBFS);
/// below it the safety knee is exactly transparent.
const SAFETY_KNEE: f32 = 0.7;

/// Output scaling so a full hit at default controls peaks near −9 dBFS.
const CALIBRATION: f32 = 0.163;
/// Envelope level below which the voice goes idle (−100 dB).
const IDLE_THRESHOLD: f32 = 1e-5;
/// Decaying states are flushed to zero below this (−120 dB), long before
/// they could become denormal.
const FLUSH_THRESHOLD: f32 = 1e-6;

/// Zeroes a decaying state once it is inaudible, so it never goes denormal.
#[inline]
fn flush(x: f32) -> f32 {
    if x.abs() < FLUSH_THRESHOLD {
        0.0
    } else {
        x
    }
}

/// Identity below [`SAFETY_KNEE`], then a smooth (first-derivative
/// continuous) bend that never exceeds ±1.
#[inline]
fn safety_knee(x: f32) -> f32 {
    let a = x.abs();
    if a <= SAFETY_KNEE {
        return x;
    }
    let room = 1.0 - SAFETY_KNEE;
    let y = SAFETY_KNEE + room * math::soft_clip((a - SAFETY_KNEE) / room);
    if x < 0.0 {
        -y
    } else {
        y
    }
}

/// Clamps a control to `0..=1`, mapping non-finite values to 0.
#[inline]
fn unit(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The snare drum voice. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct Snare {
    sample_rate: f32,
    params: VoiceParams,

    // Derived per sample rate.
    blip_coef: f32,
    snap_coef: f32,
    declick_coef: f32,
    level_coef: f32,

    // Derived per trigger.
    low_inc: f32,
    high_inc: f32,
    low_blip_depth: f32,
    high_blip_depth: f32,
    low_coef: f32,
    high_coef: f32,
    noise_coef: f32,
    noise_scale: f32,
    drive: f32,
    drive_norm: f32,
    hit_gain: f32,
    noise_hp: Svf,
    noise_lp: Svf,

    // State.
    active: bool,
    noise: Noise,
    low_phase: f32,
    high_phase: f32,
    low_env: f32,
    high_env: f32,
    blip_env: f32,
    noise_env: f32,
    snap_env: f32,
    offset: f32,
    level_now: f32,
    last_out: f32,
}

impl Snare {
    /// Creates an idle voice for the given sample rate.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        let mut snare = Self {
            sample_rate,
            params: VoiceParams::default(),
            blip_coef: 0.0,
            snap_coef: 0.0,
            declick_coef: 0.0,
            level_coef: 0.0,
            low_inc: 0.0,
            high_inc: 0.0,
            low_blip_depth: 0.0,
            high_blip_depth: 0.0,
            low_coef: 0.0,
            high_coef: 0.0,
            noise_coef: 0.0,
            noise_scale: 0.0,
            drive: 1.0,
            drive_norm: 1.0,
            hit_gain: 0.0,
            noise_hp: Svf::default(),
            noise_lp: Svf::default(),
            active: false,
            noise: Noise::new(NOISE_SEED),
            low_phase: 0.0,
            high_phase: 0.0,
            low_env: 0.0,
            high_env: 0.0,
            blip_env: 0.0,
            noise_env: 0.0,
            snap_env: 0.0,
            offset: 0.0,
            level_now: 0.0,
            last_out: 0.0,
        };
        snare.set_sample_rate(sample_rate);
        snare
    }

    /// Current controls.
    #[must_use]
    pub fn params(&self) -> VoiceParams {
        self.params
    }

    /// Lower and upper body frequencies (Hz) the current `tune` resolves to
    /// once the pitch blip has settled.
    #[must_use]
    pub fn body_frequencies_hz(&self) -> (f32, f32) {
        let ratio = math::exp_range(self.params.tune, TUNE_LOW_RATIO, TUNE_LN_RATIO);
        (LOW_BODY_HZ * ratio, HIGH_BODY_HZ * ratio)
    }

    /// Noise tail (seconds to −60 dB) the current `decay` resolves to.
    #[must_use]
    pub fn noise_decay_seconds(&self) -> f32 {
        math::exp_range(self.params.decay, NOISE_T60_LOW_S, NOISE_T60_LN_RATIO)
    }

    /// Clears the per-hit envelopes and oscillator phases. The noise filters
    /// keep running across a retrigger (their input never stops), and the
    /// noise generator's sequence carries across hits like an analogue noise
    /// source does.
    fn reset_hit(&mut self) {
        self.low_phase = 0.0;
        self.high_phase = 0.0;
        self.low_env = 0.0;
        self.high_env = 0.0;
        self.blip_env = 0.0;
        self.noise_env = 0.0;
        self.snap_env = 0.0;
    }

    fn go_idle(&mut self) {
        self.reset_hit();
        self.noise_hp.reset();
        self.noise_lp.reset();
        self.active = false;
        self.offset = 0.0;
        self.last_out = 0.0;
    }
}

impl Voice for Snare {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = if sample_rate.is_finite() {
            sample_rate.clamp(1.0, MAX_SAMPLE_RATE)
        } else {
            48_000.0
        };
        let sr = self.sample_rate;
        self.blip_coef = math::tau_coefficient(BLIP_TAU_S, sr);
        self.snap_coef = math::tau_coefficient(SNAP_TAU_S, sr);
        self.declick_coef = math::tau_coefficient(DECLICK_TAU_S, sr);
        self.level_coef = 1.0 - math::tau_coefficient(LEVEL_TAU_S, sr);
        self.noise = Noise::new(NOISE_SEED);
        self.go_idle();
    }

    fn apply_params(&mut self, params: &VoiceParams) {
        self.params = VoiceParams {
            tune: unit(params.tune),
            decay: unit(params.decay),
            tone: unit(params.tone),
            snappy: unit(params.snappy),
            level: unit(params.level),
        };
    }

    fn trigger(&mut self, velocity: f32) {
        let velocity = unit(velocity);
        if velocity <= 0.0 {
            return;
        }
        let sr = self.sample_rate;
        let p = self.params;

        // Carry whatever is sounding now (already scaled by the old level);
        // it fades out under the new hit. The new hit starts from silence
        // (body phase 0, noise envelope onset), so it can take the current
        // `level` at once: a hit right after a level move, ringing or not,
        // plays at the new level instead of gliding in from the old one.
        self.offset = if self.active { self.last_out } else { 0.0 };
        self.level_now = p.level;
        self.reset_hit();

        // Body.
        let (low_hz, high_hz) = self.body_frequencies_hz();
        self.low_inc = (low_hz / sr).min(MAX_INC);
        self.high_inc = (high_hz / sr).min(MAX_INC);
        let blip = 0.5 + 0.5 * velocity;
        self.low_blip_depth = LOW_BLIP_DEPTH * blip;
        self.high_blip_depth = HIGH_BLIP_DEPTH * blip;
        let body_scale = math::exp_range(p.decay, BODY_DECAY_LOW, BODY_DECAY_LN_RATIO);
        self.low_coef = math::decay_coefficient(LOW_BODY_T60_S * body_scale, sr);
        self.high_coef = math::decay_coefficient(HIGH_BODY_T60_S * body_scale, sr);
        self.drive = DRIVE_BASE + DRIVE_VELOCITY * velocity;
        self.drive_norm = 1.0 / math::soft_clip(self.drive);

        // Snares.
        let hp_hz = math::exp_range(p.tone, NOISE_HP_LOW_HZ, NOISE_HP_LN_RATIO);
        let lp_hz = (math::exp_range(p.tone, NOISE_LP_LOW_HZ, NOISE_LP_LN_RATIO)
            * (BRIGHT_BASE + BRIGHT_VELOCITY * velocity))
            .min(NOISE_LP_MAX_NYQUIST * sr);
        self.noise_hp.set(hp_hz, NOISE_HP_Q, sr);
        self.noise_lp.set(lp_hz, NOISE_LP_Q, sr);
        self.noise_coef = math::decay_coefficient(self.noise_decay_seconds(), sr);
        // White noise uniform in [-1, 1) has variance 1/3, spread evenly up
        // to Nyquist; the filters keep roughly `bandwidth` hertz of it. Scale
        // the filtered noise to a fixed RMS, so neither `tone` nor the sample
        // rate changes its level, only its colour.
        let bandwidth = (NOISE_LP_ENBW * lp_hz - NOISE_HP_ENBW * hp_hz).max(100.0);
        let rms = (bandwidth * 2.0 / (3.0 * sr)).sqrt();
        self.noise_scale = NOISE_DRIVE_RMS / rms;
        // Linear in amplitude, like the noise VCA's level pot: every part of
        // the travel stays useful (the top half adds 6 dB of wires).
        let noise_level = NOISE_GAIN
            * p.snappy
            * (NOISE_VELOCITY_FLOOR + (1.0 - NOISE_VELOCITY_FLOOR) * velocity);

        // Start the hit.
        self.low_env = LOW_BODY_GAIN - LOW_BODY_TONE_CUT * p.tone;
        self.high_env = HIGH_BODY_GAIN;
        self.blip_env = 1.0;
        self.noise_env = noise_level;
        self.snap_env = noise_level
            * SNAP_GAIN
            * (SNAP_VELOCITY_FLOOR + (1.0 - SNAP_VELOCITY_FLOOR) * velocity);
        self.hit_gain = velocity * CALIBRATION;
        self.active = true;
    }

    #[inline]
    fn process(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }

        // Body: two pitch-blipped sines, summed and soft-clipped.
        let low = math::sin_turns(self.low_phase) * self.low_env;
        let high = math::sin_turns(self.high_phase) * self.high_env;
        self.low_phase += self.low_inc * (1.0 + self.low_blip_depth * self.blip_env);
        if self.low_phase >= 1.0 {
            self.low_phase -= 1.0;
        }
        self.high_phase += self.high_inc * (1.0 + self.high_blip_depth * self.blip_env);
        if self.high_phase >= 1.0 {
            self.high_phase -= 1.0;
        }
        let body = math::soft_clip((low + high) * self.drive) * self.drive_norm;

        // Snares: white noise → high-pass → low-pass → soft clip → envelope.
        let hp = self.noise_hp.process(self.noise.tick()).high;
        let lp = self.noise_lp.process(hp).low;
        let wires = math::soft_clip(lp * self.noise_scale) * (self.noise_env + self.snap_env);

        // Level: follows the control within a few milliseconds.
        self.level_now += (self.params.level - self.level_now) * self.level_coef;
        if (self.params.level - self.level_now).abs() < FLUSH_THRESHOLD {
            self.level_now = self.params.level;
        }
        let out = safety_knee((body + wires) * self.hit_gain * self.level_now + self.offset);

        // Advance envelopes.
        self.low_env = flush(self.low_env * self.low_coef);
        self.high_env = flush(self.high_env * self.high_coef);
        self.blip_env = flush(self.blip_env * self.blip_coef);
        self.noise_env = flush(self.noise_env * self.noise_coef);
        self.snap_env = flush(self.snap_env * self.snap_coef);
        self.offset = flush(self.offset * self.declick_coef);
        if self.low_env < IDLE_THRESHOLD
            && self.high_env < IDLE_THRESHOLD
            && self.noise_env < IDLE_THRESHOLD
            && self.snap_env < IDLE_THRESHOLD
            && self.offset.abs() < IDLE_THRESHOLD
        {
            self.go_idle();
        } else {
            self.last_out = out;
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

    const SR: f32 = 48_000.0;

    fn snare_with(sr: f32, f: impl FnOnce(&mut VoiceParams)) -> Snare {
        let mut s = Snare::new(sr);
        let mut p = VoiceParams::default();
        f(&mut p);
        s.apply_params(&p);
        s
    }

    fn render(s: &mut Snare, n: usize) -> Vec<f32> {
        (0..n).map(|_| s.process()).collect()
    }

    fn hit(sr: f32, velocity: f32, f: impl FnOnce(&mut VoiceParams)) -> Vec<f32> {
        let mut s = snare_with(sr, f);
        s.trigger(velocity);
        render(&mut s, (sr * 0.5) as usize)
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    fn db(x: f32) -> f32 {
        20.0 * x.log10()
    }

    fn energy(x: &[f32]) -> f64 {
        x.iter().map(|&s| f64::from(s) * f64::from(s)).sum()
    }

    fn max_step(x: &[f32]) -> f32 {
        x.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()))
    }

    /// Power of `x` at `hz` (Goertzel).
    fn power_at(x: &[f32], sr: f32, hz: f32) -> f64 {
        let w = std::f64::consts::TAU * f64::from(hz) / f64::from(sr);
        let c = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &v in x {
            let s0 = f64::from(v) + c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        s1 * s1 + s2 * s2 - c * s1 * s2
    }

    /// `x` under a Hann window.
    fn hann(x: &[f32]) -> Vec<f32> {
        let n = x.len() as f32;
        x.iter()
            .enumerate()
            .map(|(i, &v)| v * (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n).cos()))
            .collect()
    }

    /// Frequency of the strongest component between `lo` and `hi`.
    fn dominant_hz(x: &[f32], sr: f32, lo: f32, hi: f32) -> f32 {
        let mut best = (lo, 0.0f64);
        let mut f = lo;
        while f <= hi {
            let p = power_at(x, sr, f);
            if p > best.1 {
                best = (f, p);
            }
            f += 0.5;
        }
        best.0
    }

    /// Power-weighted spectral centroid over 50 Hz – 16 kHz.
    fn centroid_hz(x: &[f32], sr: f32) -> f64 {
        let (mut num, mut den) = (0.0f64, 0.0f64);
        let mut f = 50.0f32;
        while f <= 16_000.0 {
            let p = power_at(x, sr, f);
            num += f64::from(f) * p;
            den += p;
            f += 50.0;
        }
        num / den
    }

    /// Mean power over a frequency range (coarse grid).
    fn band_power(x: &[f32], sr: f32, lo: f32, hi: f32) -> f64 {
        let (mut sum, mut n) = (0.0f64, 0.0f64);
        let mut f = lo;
        while f <= hi {
            sum += power_at(x, sr, f);
            n += 1.0;
            f += 25.0;
        }
        sum / n
    }

    #[test]
    fn idle_voice_is_silent() {
        let mut s = Snare::new(SR);
        assert!(!s.is_active());
        assert!(render(&mut s, 1_000).iter().all(|&x| x == 0.0));
        // A zero-velocity (or garbage) hit is ignored.
        s.trigger(0.0);
        s.trigger(f32::NAN);
        assert!(!s.is_active());
        assert!(render(&mut s, 1_000).iter().all(|&x| x == 0.0));
    }

    #[test]
    fn zero_velocity_hit_leaves_a_ringing_drum_alone() {
        let mut a = Snare::new(SR);
        let mut b = Snare::new(SR);
        a.trigger(1.0);
        b.trigger(1.0);
        assert_eq!(render(&mut a, 500), render(&mut b, 500));
        b.trigger(0.0);
        assert_eq!(render(&mut a, 5_000), render(&mut b, 5_000));
    }

    #[test]
    fn noise_level_is_independent_of_tone_and_sample_rate() {
        // The filtered noise is normalised to a fixed RMS ahead of its soft
        // clip; check the estimate holds across the control and rate range.
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            for tone in [0.0, 0.5, 1.0] {
                for velocity in [0.1, 1.0] {
                    let mut s = snare_with(sr, |p| p.tone = tone);
                    s.trigger(velocity);
                    let n = 100_000;
                    let mut acc = 0.0f64;
                    for _ in 0..n {
                        let hp = s.noise_hp.process(s.noise.tick()).high;
                        let lp = s.noise_lp.process(hp).low * s.noise_scale;
                        acc += f64::from(lp) * f64::from(lp);
                    }
                    let rms = (acc / f64::from(n)).sqrt() as f32;
                    let error = db(rms / NOISE_DRIVE_RMS);
                    assert!(
                        error.abs() < 1.5,
                        "{sr} Hz tone {tone} v {velocity}: {error} dB"
                    );
                }
            }
        }
    }

    #[test]
    fn full_hit_peaks_near_minus_9_dbfs() {
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            // A fresh voice's first hit (what a pattern's first step plays).
            let first = db(peak(&hit(sr, 1.0, |_| {})));
            assert!(
                (-10.5..=-7.5).contains(&first),
                "{sr} Hz: first hit {first} dBFS"
            );

            // Consecutive hits see different noise, so the peak wanders a
            // little from hit to hit: the typical hit sits on target and
            // nearly all land within the window.
            let mut s = Snare::new(sr);
            let mut peaks: Vec<f32> = (0..200)
                .map(|_| {
                    s.trigger(1.0);
                    db(peak(&render(&mut s, (sr * 0.3) as usize)))
                })
                .collect();
            peaks.sort_by(f32::total_cmp);
            let median = peaks[100];
            assert!(
                (-9.5..=-8.5).contains(&median),
                "{sr} Hz: median {median} dBFS"
            );
            let inside = peaks
                .iter()
                .filter(|&&p| (-10.5..=-7.5).contains(&p))
                .count();
            assert!(
                inside >= 196,
                "{sr} Hz: {inside}/200 hits in window, {peaks:?}"
            );
            assert!(peaks[0] > -11.5 && peaks[199] < -6.5, "{sr} Hz: {peaks:?}");
        }
    }

    #[test]
    fn bounded_and_finite_for_extreme_controls() {
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            for corner in 0..32u32 {
                let bit = |b: u32| if corner & (1 << b) != 0 { 1.0 } else { 0.0 };
                let mut s = snare_with(sr, |p| {
                    p.tune = bit(0);
                    p.decay = bit(1);
                    p.tone = bit(2);
                    p.snappy = bit(3);
                    p.level = bit(4);
                });
                for velocity in [0.1, 0.7, 1.0] {
                    s.trigger(velocity);
                    // Retrigger mid-ring as well.
                    let mut out = render(&mut s, (sr * 0.012) as usize);
                    s.trigger(velocity);
                    out.extend(render(&mut s, (sr * 0.3) as usize));
                    for x in out {
                        assert!(x.is_finite() && x.abs() <= 1.0, "{sr} Hz {corner:05b}: {x}");
                    }
                }
            }
        }
    }

    #[test]
    fn extreme_settings_stay_below_full_scale_without_the_knee() {
        // The safety knee is a safety net, not part of the sound: even the
        // hottest settings at full accent peak below it.
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            for (tune, tone) in [(0.0, 0.0), (0.0, 1.0), (1.0, 0.0), (1.0, 1.0)] {
                let mut s = snare_with(sr, |p| {
                    p.tune = tune;
                    p.tone = tone;
                    p.snappy = 1.0;
                    p.decay = 1.0;
                });
                let mut worst = 0.0f32;
                for _ in 0..32 {
                    s.trigger(1.0);
                    worst = worst.max(peak(&render(&mut s, (sr * 0.25) as usize)));
                }
                assert!(worst < SAFETY_KNEE * 0.85, "{sr} Hz {tune} {tone}: {worst}");
            }
        }
    }

    #[test]
    fn safety_knee_is_transparent_then_bounded() {
        assert_eq!(safety_knee(0.5), 0.5);
        assert_eq!(safety_knee(-SAFETY_KNEE), -SAFETY_KNEE);
        assert_eq!(safety_knee(10.0), 1.0);
        assert_eq!(safety_knee(-10.0), -1.0);
        let mut prev = safety_knee(-2.0);
        let mut x = -2.0f32;
        while x <= 2.0 {
            let y = safety_knee(x);
            assert!(y >= prev - 1e-6 && y.abs() <= 1.0, "{x}");
            prev = y;
            x += 0.001;
        }
    }

    #[test]
    fn decays_to_silence_and_goes_idle() {
        for decay in [0.0, 0.5, 1.0] {
            let mut s = snare_with(SR, |p| {
                p.decay = decay;
                p.snappy = 1.0;
            });
            s.trigger(1.0);
            let mut out = Vec::new();
            while s.is_active() {
                out.push(s.process());
                assert!(out.len() < 48_000, "decay {decay}: still active after 1 s");
            }
            // The last active stretch is already below -100 dBFS.
            let tail = &out[out.len() - 100..];
            assert!(peak(tail) < 1e-5, "decay {decay}: tail {}", peak(tail));
            // Idle means exact zeros.
            assert!(render(&mut s, 4_800).iter().all(|&x| x == 0.0));
        }
    }

    #[test]
    fn tune_moves_the_body_modes() {
        for (tune, low, high) in [
            (0.0, 127.28, 233.35),
            (0.5, 180.0, 330.0),
            (1.0, 254.56, 466.69),
        ] {
            let s = snare_with(SR, |p| p.tune = tune);
            let (l, h) = s.body_frequencies_hz();
            assert!((l - low).abs() < 0.1 && (h - high).abs() < 0.1, "{l} {h}");

            // Body only; measure after the pitch blip has settled.
            let out = hit(SR, 1.0, |p| {
                p.tune = tune;
                p.snappy = 0.0;
            });
            let window = hann(&out[960..4_800]); // 20–100 ms
            let f = dominant_hz(&window, SR, 80.0, 600.0);
            assert!(
                (f - low).abs() < low * 0.02,
                "tune {tune}: {f} Hz, expected {low}"
            );
            let upper = dominant_hz(&window, SR, high * 0.85, high * 1.15);
            assert!(
                (upper - high).abs() < high * 0.02,
                "tune {tune}: upper {upper} Hz, expected {high}"
            );
            // Two distinct modes, not one smeared lump.
            let gap = power_at(&window, SR, (low + high) * 0.5);
            assert!(power_at(&window, SR, high) > 30.0 * gap, "tune {tune}");
        }
        let freq = |tune| {
            let out = hit(SR, 1.0, |p| {
                p.tune = tune;
                p.snappy = 0.0;
            });
            dominant_hz(&hann(&out[960..4_800]), SR, 80.0, 600.0)
        };
        assert!(freq(0.25) < freq(0.5) && freq(0.5) < freq(0.75));
    }

    #[test]
    fn same_drum_at_every_sample_rate() {
        let reference = centroid_hz(&hit(SR, 1.0, |_| {})[..4_800], SR);
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            let ms = |t: f32| (t * sr / 1_000.0) as usize;
            let body = hit(sr, 1.0, |p| p.snappy = 0.0);
            let f = dominant_hz(&hann(&body[ms(20.0)..ms(100.0)]), sr, 80.0, 600.0);
            assert!((f - 180.0).abs() < 180.0 * 0.02, "{sr} Hz: body at {f} Hz");
            let full = hit(sr, 1.0, |_| {});
            let c = centroid_hz(&full[..ms(100.0)], sr);
            assert!(
                (c / reference - 1.0).abs() < 0.1,
                "{sr} Hz: centroid {c} vs {reference}"
            );
        }
    }

    #[test]
    fn pitch_blips_down_and_settles() {
        // Unwrap the lower mode's phase: early on it runs sharp, then
        // settles onto the tuned pitch.
        let mut s = Snare::new(SR);
        s.trigger(1.0);
        let turns = |s: &mut Snare, n: usize| {
            let mut total = 0.0f64;
            for _ in 0..n {
                let before = s.low_phase;
                s.process();
                let mut d = f64::from(s.low_phase) - f64::from(before);
                if d < 0.0 {
                    d += 1.0;
                }
                total += d;
            }
            total
        };
        let settled = f64::from(s.body_frequencies_hz().0) / f64::from(SR);
        let early = turns(&mut s, 240) / 240.0; // first 5 ms
        assert!(early > settled * 1.12, "{early} vs {settled}");
        turns(&mut s, 1_200); // skip to 30 ms
        let late = turns(&mut s, 480) / 480.0;
        assert!((late / settled - 1.0).abs() < 0.005, "{late} vs {settled}");
    }

    #[test]
    fn decay_lengthens_the_noise_tail() {
        let tail = |decay| {
            let out = hit(SR, 1.0, |p| p.decay = decay);
            energy(&out[7_200..14_400]) // 150–300 ms
        };
        let (short, mid, long) = (tail(0.0), tail(0.5), tail(1.0));
        assert!(mid > short * 5.0, "{short} {mid}");
        assert!(long > mid * 5.0, "{mid} {long}");
        let s = snare_with(SR, |p| p.decay = 0.0);
        assert!((s.noise_decay_seconds() - 0.08).abs() < 1e-4);
        let s = snare_with(SR, |p| p.decay = 1.0);
        assert!((s.noise_decay_seconds() - 0.4).abs() < 1e-3);
    }

    #[test]
    fn tone_raises_the_spectral_centroid() {
        let centroid = |tone| {
            let out = hit(SR, 1.0, |p| p.tone = tone);
            centroid_hz(&out[..4_800], SR)
        };
        let (dark, mid, bright) = (centroid(0.0), centroid(0.5), centroid(1.0));
        assert!(mid > dark * 1.15, "{dark} {mid}");
        assert!(bright > mid * 1.15, "{mid} {bright}");

        // Low tone keeps more of the lower body mode.
        let body = |tone| {
            let out = hit(SR, 1.0, |p| {
                p.tone = tone;
                p.snappy = 0.0;
            });
            power_at(&out[..4_800], SR, 180.0)
        };
        assert!(body(0.0) > body(1.0) * 2.0);
    }

    #[test]
    fn snappy_raises_noise_energy() {
        let noise = |snappy| {
            let out = hit(SR, 1.0, |p| p.snappy = snappy);
            band_power(&out[..9_600], SR, 2_000.0, 10_000.0)
        };
        let (none, half, full) = (noise(0.0), noise(0.5), noise(1.0));
        assert!(half > none * 100.0, "{none} {half}");
        assert!(full > half * 1.5, "{half} {full}");

        // The body is untouched by snappy.
        let body = |snappy| {
            let out = hit(SR, 1.0, |p| p.snappy = snappy);
            power_at(&out[..4_800], SR, 180.0)
        };
        let (a, b) = (body(0.0), body(1.0));
        assert!((b / a - 1.0).abs() < 0.2, "{a} {b}");
    }

    #[test]
    fn accent_is_louder_and_brighter() {
        let ghost = hit(SR, 0.42, |_| {});
        let plain = hit(SR, 0.7, |_| {});
        let accent = hit(SR, 1.0, |_| {});
        let (pg, pp, pa) = (peak(&ghost), peak(&plain), peak(&accent));
        assert!(pp > pg * 1.4 && pa > pp * 1.3, "{pg} {pp} {pa}");
        let (cp, ca) = (
            centroid_hz(&plain[..4_800], SR),
            centroid_hz(&accent[..4_800], SR),
        );
        assert!(ca > cp * 1.05, "{cp} {ca}");
    }

    #[test]
    fn level_applies_immediately_and_smoothly() {
        let mut a = Snare::new(SR);
        let mut b = Snare::new(SR);
        a.trigger(1.0);
        b.trigger(1.0);
        let ra = render(&mut a, 1_200);
        let rb = render(&mut b, 1_200);
        assert_eq!(ra, rb);
        b.apply_params(&VoiceParams {
            level: 0.5,
            ..VoiceParams::default()
        });
        let ra = render(&mut a, 4_800);
        let rb = render(&mut b, 4_800);
        // The gain starts moving on the very next sample, glides down
        // without a jump and has settled at one half within 50 ms.
        let gains: Vec<f32> = ra
            .iter()
            .zip(&rb)
            .filter(|(x, _)| x.abs() > 1e-3)
            .map(|(x, y)| y / x)
            .collect();
        assert!(gains[0] < 1.0 && gains[0] > 0.99, "{}", gains[0]);
        assert!(gains.windows(2).all(|w| w[1] <= w[0] + 1e-4));
        for (x, y) in ra[2_400..].iter().zip(&rb[2_400..]) {
            assert!((y - 0.5 * x).abs() <= 1e-4 * x.abs() + 1e-7, "{x} {y}");
        }

        // Level 0 is silence.
        let out = hit(SR, 1.0, |p| p.level = 0.0);
        assert!(out.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn deterministic_renders() {
        let run = || {
            let mut s = Snare::new(SR);
            let mut out = Vec::new();
            for (velocity, tone) in [(1.0, 0.5), (0.7, 0.2), (0.42, 0.9)] {
                s.apply_params(&VoiceParams {
                    tone,
                    ..VoiceParams::default()
                });
                s.trigger(velocity);
                out.extend(render(&mut s, 7_000));
            }
            out
        };
        let (a, b) = (run(), run());
        assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
        assert!(peak(&a) > 0.0);
    }

    /// Largest sample-to-sample step around a retrigger at `at` samples,
    /// against the largest step anywhere in an undisturbed hit.
    fn retrigger_steps(snappy: f32, at: usize, grace: f32) -> (f32, f32) {
        let fresh = hit(SR, 1.0, |p| p.snappy = snappy);
        let mut s = snare_with(SR, |p| p.snappy = snappy);
        s.trigger(grace);
        let mut out = render(&mut s, at);
        s.trigger(1.0);
        out.extend(render(&mut s, 960));
        let around = &out[at.saturating_sub(48)..at + 240];
        (max_step(around), max_step(&fresh))
    }

    #[test]
    fn retrigger_mid_ring_does_not_click() {
        // Body only: the waveform is smooth, so any jump would stand out.
        // A hard reset here would step by the full ringing amplitude.
        for at in [48, 384, 720, 1_152, 1_920] {
            for grace in [0.42, 1.0] {
                let (step, normal) = retrigger_steps(0.0, at, grace);
                assert!(step < normal * 1.6, "body, at {at}: {step} vs {normal}");
            }
        }
        // Full voice: no larger steps than a fresh hit's own attack.
        for at in [48, 384, 720, 1_152, 1_920] {
            let (step, normal) = retrigger_steps(0.5, at, 0.42);
            assert!(step < normal * 1.3, "full, at {at}: {step} vs {normal}");
        }
    }

    #[test]
    fn sample_rate_change_resets() {
        let mut s = Snare::new(SR);
        s.trigger(1.0);
        render(&mut s, 100);
        s.set_sample_rate(96_000.0);
        assert!(!s.is_active());
        assert_eq!(s.process(), 0.0);
    }

    #[test]
    fn render_is_cheap() {
        let mut s = Snare::new(SR);
        let start = std::time::Instant::now();
        let mut acc = 0.0f32;
        for i in 0..480_000 {
            if i % 6_000 == 0 {
                s.trigger(if i % 12_000 == 0 { 1.0 } else { 0.7 });
            }
            acc += s.process();
        }
        std::hint::black_box(acc);
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() < 100, "10 s of snare took {elapsed:?}");
    }
}
