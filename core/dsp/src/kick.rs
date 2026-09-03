//! TR-inspired bass drum.
//!
//! The classic analogue circuit is a bridged-T resonator: a damped second-
//! order network that is kicked into oscillation by the trigger pulse and
//! rings down at its own rate. The pulse also pushes the resonator briefly
//! sharp (the characteristic pitch drop in the first few tens of
//! milliseconds), and the pulse edge itself leaks through as a click.
//!
//! This model keeps the three ingredients and nothing else:
//!
//! * **body** – a decaying sine whose frequency starts above the tuned pitch
//!   and settles to it exponentially;
//! * **click** – a short exponential pulse, band-passed by a one-pole
//!   high-pass and low-pass, mixed in ahead of the saturation stage;
//! * **saturation** – a soft clip whose drive follows velocity, so accented
//!   hits are denser, not just louder.
//!
//! Controls are normalised `0..=1`: `tune` (40–90 Hz), `decay` (60 dB fall
//! time 0.1–2 s) and `level`. A full-velocity hit at `level = 1.0` peaks
//! close to −6 dBFS.

use crate::math;
use crate::voice::Voice;

/// Normalised kick parameters. Every field is `0..=1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KickParams {
    /// Pitch of the body: 0 = 40 Hz, 0.5 = 60 Hz, 1 = 90 Hz.
    pub tune: f32,
    /// Ring-down time: 0 = 0.1 s, 0.5 ≈ 0.45 s, 1 = 2 s (to −60 dB).
    pub decay: f32,
    /// Linear output level.
    pub level: f32,
}

impl Default for KickParams {
    fn default() -> Self {
        Self {
            tune: 0.5,
            decay: 0.5,
            level: 1.0,
        }
    }
}

const TUNE_LOW_HZ: f32 = 40.0;
/// ln(90 / 40).
const TUNE_LN_RATIO: f32 = 0.810_930_216;
const DECAY_LOW_S: f32 = 0.1;
/// ln(2.0 / 0.1).
const DECAY_LN_RATIO: f32 = 2.995_732_274;

/// How far above the tuned pitch a full-velocity hit starts (multiple of f0).
const SWEEP_DEPTH: f32 = 1.5;
/// Time constant of the pitch drop.
const SWEEP_TAU_S: f32 = 0.022;
/// Time constant of the click pulse.
const CLICK_TAU_S: f32 = 0.001;
const CLICK_HP_HZ: f32 = 1_500.0;
const CLICK_LP_HZ: f32 = 5_000.0;
const CLICK_GAIN: f32 = 0.7;
/// Saturation drive at velocity 0 and the extra drive at velocity 1.
const DRIVE_BASE: f32 = 1.0;
const DRIVE_VELOCITY: f32 = 0.5;
/// Output scaling so a full hit at `level = 1` peaks near −6 dBFS.
const CALIBRATION: f32 = 0.5;
/// Envelope level below which the voice goes idle (−100 dB).
const IDLE_THRESHOLD: f32 = 1e-5;

/// The bass drum voice. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct Kick {
    sample_rate: f32,
    params: KickParams,

    // Derived per trigger.
    base_freq_hz: f32,
    amp_coef: f32,
    velocity: f32,
    drive: f32,
    drive_norm: f32,

    // Derived per sample rate.
    sweep_coef: f32,
    click_coef: f32,
    click_hp_coef: f32,
    click_lp_coef: f32,

    // State.
    active: bool,
    phase: f32,
    amp_env: f32,
    pitch_env: f32,
    click_env: f32,
    hp_x1: f32,
    hp_y1: f32,
    lp_y1: f32,
}

impl Kick {
    /// Creates an idle voice for the given sample rate.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        let mut kick = Self {
            sample_rate,
            params: KickParams::default(),
            base_freq_hz: 0.0,
            amp_coef: 0.0,
            velocity: 0.0,
            drive: 1.0,
            drive_norm: 1.0,
            sweep_coef: 0.0,
            click_coef: 0.0,
            click_hp_coef: 0.0,
            click_lp_coef: 0.0,
            active: false,
            phase: 0.0,
            amp_env: 0.0,
            pitch_env: 0.0,
            click_env: 0.0,
            hp_x1: 0.0,
            hp_y1: 0.0,
            lp_y1: 0.0,
        };
        kick.set_sample_rate(sample_rate);
        kick
    }

    /// Current parameters.
    #[must_use]
    pub fn params(&self) -> KickParams {
        self.params
    }

    /// Replaces all parameters. Takes effect on the next trigger, except
    /// `level`, which applies immediately.
    pub fn set_params(&mut self, params: KickParams) {
        self.params = KickParams {
            tune: params.tune.clamp(0.0, 1.0),
            decay: params.decay.clamp(0.0, 1.0),
            level: params.level.clamp(0.0, 1.0),
        };
    }

    /// Sets `tune` (`0..=1`).
    pub fn set_tune(&mut self, tune: f32) {
        self.params.tune = tune.clamp(0.0, 1.0);
    }

    /// Sets `decay` (`0..=1`).
    pub fn set_decay(&mut self, decay: f32) {
        self.params.decay = decay.clamp(0.0, 1.0);
    }

    /// Sets `level` (`0..=1`).
    pub fn set_level(&mut self, level: f32) {
        self.params.level = level.clamp(0.0, 1.0);
    }

    /// Body frequency (Hz) the current `tune` resolves to once the sweep has
    /// settled.
    #[must_use]
    pub fn tuned_frequency_hz(&self) -> f32 {
        math::exp_range(self.params.tune, TUNE_LOW_HZ, TUNE_LN_RATIO)
    }

    /// Ring-down time (seconds to −60 dB) the current `decay` resolves to.
    #[must_use]
    pub fn decay_seconds(&self) -> f32 {
        math::exp_range(self.params.decay, DECAY_LOW_S, DECAY_LN_RATIO)
    }

    fn reset_state(&mut self) {
        self.active = false;
        self.phase = 0.0;
        self.amp_env = 0.0;
        self.pitch_env = 0.0;
        self.click_env = 0.0;
        self.hp_x1 = 0.0;
        self.hp_y1 = 0.0;
        self.lp_y1 = 0.0;
    }
}

impl Voice for Kick {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.sweep_coef = math::tau_coefficient(SWEEP_TAU_S, self.sample_rate);
        self.click_coef = math::tau_coefficient(CLICK_TAU_S, self.sample_rate);
        self.click_hp_coef = math::onepole_coefficient(CLICK_HP_HZ, self.sample_rate);
        self.click_lp_coef = math::onepole_coefficient(CLICK_LP_HZ, self.sample_rate);
        self.reset_state();
    }

    fn trigger(&mut self, velocity: f32) {
        let velocity = velocity.clamp(0.0, 1.0);
        self.velocity = velocity;
        self.base_freq_hz = self.tuned_frequency_hz();
        self.amp_coef = math::decay_coefficient(self.decay_seconds(), self.sample_rate);
        self.drive = DRIVE_BASE + DRIVE_VELOCITY * velocity;
        self.drive_norm = 1.0 / math::soft_clip(self.drive);

        // Hard retrigger: the analogue circuit restarts from the pulse edge.
        self.reset_state();
        self.active = velocity > 0.0;
        self.amp_env = 1.0;
        self.pitch_env = velocity;
        self.click_env = velocity;
    }

    #[inline]
    fn process(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }

        // Body: sine with exponentially settling pitch and exponential decay.
        let freq = self.base_freq_hz * (1.0 + SWEEP_DEPTH * self.pitch_env);
        self.phase += freq / self.sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let body = math::sin_turns(self.phase) * self.amp_env;

        // Click: pulse → one-pole high-pass → one-pole low-pass.
        let hp = self.click_hp_coef * (self.hp_y1 + self.click_env - self.hp_x1);
        self.hp_x1 = self.click_env;
        self.hp_y1 = hp;
        self.lp_y1 += (1.0 - self.click_lp_coef) * (hp - self.lp_y1);
        let click = self.lp_y1 * CLICK_GAIN;

        // Saturate the mix; normalise so a full-scale body still peaks at 1.
        let shaped = math::soft_clip((body + click) * self.drive) * self.drive_norm;

        // Advance envelopes.
        self.amp_env *= self.amp_coef;
        self.pitch_env *= self.sweep_coef;
        self.click_env *= self.click_coef;
        if self.amp_env < IDLE_THRESHOLD && self.click_env < IDLE_THRESHOLD {
            self.reset_state();
        }

        shaped * self.velocity * self.params.level * CALIBRATION
    }

    #[inline]
    fn is_active(&self) -> bool {
        self.active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(kick: &mut Kick, n: usize) -> Vec<f32> {
        (0..n).map(|_| kick.process()).collect()
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn idle_voice_is_silent() {
        let mut kick = Kick::new(48_000.0);
        assert!(!kick.is_active());
        assert!(render(&mut kick, 1_000).iter().all(|&s| s == 0.0));
    }

    #[test]
    fn full_hit_peaks_near_minus_6_dbfs() {
        let mut kick = Kick::new(48_000.0);
        kick.trigger(1.0);
        let out = render(&mut kick, 48_000);
        let p = peak(&out);
        let db = 20.0 * p.log10();
        assert!((-7.0..=-5.0).contains(&db), "peak {p} = {db} dBFS");
    }

    #[test]
    fn velocity_scales_output() {
        let mut a = Kick::new(48_000.0);
        let mut b = Kick::new(48_000.0);
        a.trigger(1.0);
        b.trigger(0.5);
        let pa = peak(&render(&mut a, 24_000));
        let pb = peak(&render(&mut b, 24_000));
        assert!(pb < pa * 0.8, "{pb} vs {pa}");
    }

    #[test]
    fn decays_to_silence_and_goes_idle() {
        let mut kick = Kick::new(48_000.0);
        kick.set_decay(0.0); // 0.1 s to -60 dB
        kick.trigger(1.0);
        let out = render(&mut kick, 48_000);
        assert!(!kick.is_active());
        assert_eq!(out[47_999], 0.0);
        let tail = peak(&out[24_000..]);
        assert!(tail < 1e-4, "tail {tail}");
    }

    #[test]
    fn longer_decay_rings_longer() {
        let energy_after = |decay: f32| {
            let mut kick = Kick::new(48_000.0);
            kick.set_decay(decay);
            kick.trigger(1.0);
            let out = render(&mut kick, 48_000);
            peak(&out[24_000..30_000])
        };
        assert!(energy_after(1.0) > energy_after(0.5) * 4.0);
        assert!(energy_after(0.5) > energy_after(0.0) * 4.0);
    }

    /// Estimates the settled body frequency from zero crossings late in the
    /// hit, after the pitch sweep has decayed.
    fn settled_frequency(tune: f32) -> f32 {
        let sr = 48_000.0;
        let mut kick = Kick::new(sr);
        kick.set_tune(tune);
        kick.set_decay(1.0);
        kick.trigger(1.0);
        let out = render(&mut kick, 48_000);
        let window = &out[24_000..44_000];
        let crossings = window
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        crossings as f32 * sr / window.len() as f32
    }

    #[test]
    fn tune_maps_to_expected_frequencies() {
        for (tune, expect) in [(0.0, 40.0), (0.5, 60.0), (1.0, 90.0)] {
            let f = settled_frequency(tune);
            assert!(
                (f - expect).abs() < 3.0,
                "tune {tune}: {f} Hz, expected {expect}"
            );
        }
    }

    #[test]
    fn pitch_sweep_starts_sharp() {
        let sr = 48_000.0;
        let mut kick = Kick::new(sr);
        kick.set_tune(0.5);
        kick.trigger(1.0);
        let out = render(&mut kick, 48_000);
        // First 20 ms should contain more cycles than the settled 60 Hz would
        // give (1.2 cycles).
        let early = &out[..960];
        let crossings = early
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        assert!(crossings >= 2, "{crossings} crossings in first 20 ms");
    }

    #[test]
    fn output_is_finite_and_bounded() {
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            let mut kick = Kick::new(sr);
            for velocity in [0.1, 0.7, 1.0] {
                kick.set_params(KickParams {
                    tune: 1.0,
                    decay: 1.0,
                    level: 1.0,
                });
                kick.trigger(velocity);
                for s in render(&mut kick, 4_096) {
                    assert!(s.is_finite() && s.abs() <= 1.0);
                }
            }
        }
    }
}
