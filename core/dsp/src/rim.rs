//! TR-inspired rimshot.
//!
//! The classic analogue circuit is a pair of bridged-T resonators, one in
//! the mid range and one an octave and a bit above, struck by the same
//! trigger pulse. Both ring for only a few tens of milliseconds; their sum is
//! high-passed so nothing below the "tock" survives, and the output stage
//! clips the first peaks, which gives the sound its hard, woody bite.
//!
//! This model keeps those ingredients:
//!
//! * **pings** – two damped resonators (≈ 470 Hz and ≈ 1.66 kHz at
//!   `tune = 0.5`). Each is a decaying complex rotator: a trigger *adds* an
//!   impulse to its state rather than restarting it, exactly like striking a
//!   ringing resonator again, so retriggers (flams, rolls) never jump;
//! * **click** – a sub-millisecond exponential pulse, band-passed around
//!   4 kHz, for the stick attack;
//! * **high-pass** – a 12 dB/oct state-variable high-pass at 250 Hz on the
//!   mix, which also removes the click's DC;
//! * **saturation** – a fixed-drive soft clip. Velocity sets how hard the
//!   resonators are struck, so accented hits are louder *and* clip harder
//!   (brighter), as on the hardware.
//!
//! Controls (all `0..=1`): `tune` shifts both pings together over ±½ octave,
//! `decay` stretches the ring subtly (low ping 22–55 ms to −60 dB, the high
//! ping 0.6× that), `tone` balances the pings (0 = woody low ping, 1 = sharp
//! high ping) and `level` scales the output. `snappy` is ignored. A
//! full-velocity hit at the default controls peaks near −10 dBFS.

use crate::blocks::Svf;
use crate::math;
use crate::voice::Voice;
use crate::VoiceParams;

/// Low ping frequency at `tune = 0.5`.
const LOW_PING_HZ: f32 = 470.0;
/// High ping frequency at `tune = 0.5`.
const HIGH_PING_HZ: f32 = 1_660.0;
/// Tune factor at `tune = 0` (½ octave down).
const TUNE_LOW_FACTOR: f32 = 0.707_106_781;
/// ln(2): `tune` spans one octave, ½ octave either side of the centre.
const TUNE_LN_RATIO: f32 = 0.693_147_181;

/// Low-ping ring time (to −60 dB) at `decay = 0`.
const DECAY_LOW_S: f32 = 0.022;
/// ln(2.5): `decay = 1` rings 2.5× longer (55 ms).
const DECAY_LN_RATIO: f32 = 0.916_290_732;
/// The high ping dies away faster than the low one.
const HIGH_DECAY_RATIO: f32 = 0.6;

/// Low-ping strike strength at `tone = 0` and its change towards `tone = 1`.
const LOW_GAIN_AT_0: f32 = 1.0;
const LOW_GAIN_SPAN: f32 = -0.65;
/// High-ping strike strength at `tone = 0` and its change towards `tone = 1`.
const HIGH_GAIN_AT_0: f32 = 0.3;
const HIGH_GAIN_SPAN: f32 = 0.85;

/// Time constant of the click pulse.
const CLICK_TAU_S: f32 = 0.000_15;
/// Click band-pass centre at `tune = 0.5` (follows `tune`) and its Q.
const CLICK_HZ: f32 = 4_000.0;
const CLICK_Q: f32 = 0.7;
/// Click strength for a full-velocity hit (it grows with velocity²).
const CLICK_LEVEL: f32 = 0.9;

/// Output high-pass.
const HP_HZ: f32 = 250.0;
const HP_Q: f32 = 0.707;

/// Gain into the soft clip. Fixed: velocity reaches the clip through the
/// strike strength, so accents saturate harder.
const DRIVE: f32 = 1.5;
/// Output scaling so a full hit at `level = 1` peaks near −10 dBFS.
const CALIBRATION: f32 = 0.3;

/// Per-resonator energy (amplitude²) below which it counts as silent
/// (−100 dB).
const IDLE_ENERGY: f32 = 1e-10;
/// Click and output level below which the voice may go idle (−100 dB).
const IDLE_LEVEL: f32 = 1e-5;

/// A damped resonator: a complex phasor that rotates by the ping frequency
/// and shrinks by the decay ratio every sample. Its imaginary part is the
/// output, so a strike (adding to the real part) starts a sine at zero
/// phase without moving the current output.
#[derive(Clone, Debug, Default)]
struct Resonator {
    re: f32,
    im: f32,
    /// `r · cos(ω)`.
    c: f32,
    /// `r · sin(ω)`.
    s: f32,
}

impl Resonator {
    /// Sets frequency and ring time. Keeps the state, so a ringing resonator
    /// glides to the new pitch.
    fn tune(&mut self, freq_hz: f32, t60_seconds: f32, sample_rate: f32) {
        let r = math::decay_coefficient(t60_seconds, sample_rate);
        let w = (freq_hz / sample_rate).clamp(0.0, 0.45);
        self.c = r * math::cos_turns(w);
        self.s = r * math::sin_turns(w);
    }

    /// Adds a strike of the given strength.
    fn strike(&mut self, amount: f32) {
        self.re += amount;
    }

    /// Current amplitude squared.
    fn energy(&self) -> f32 {
        self.re * self.re + self.im * self.im
    }

    fn reset(&mut self) {
        self.re = 0.0;
        self.im = 0.0;
    }

    /// Returns the current output, then advances one sample.
    #[inline]
    fn tick(&mut self) -> f32 {
        let out = self.im;
        let re = self.c * self.re - self.s * self.im;
        let im = self.s * self.re + self.c * self.im;
        self.re = re;
        self.im = im;
        out
    }
}

/// The rimshot voice. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct Rim {
    sample_rate: f32,
    tune: f32,
    decay: f32,
    tone: f32,
    level: f32,

    // Derived per sample rate.
    click_coef: f32,
    drive_norm: f32,

    // State.
    active: bool,
    low: Resonator,
    high: Resonator,
    click_env: f32,
    click_bp: Svf,
    hp: Svf,
}

/// Clamps a control to `0..=1`; non-finite values become `fallback`.
fn unit(x: f32, fallback: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        fallback
    }
}

impl Rim {
    /// Creates an idle voice for the given sample rate.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        let defaults = VoiceParams::default();
        let mut rim = Self {
            sample_rate,
            tune: defaults.tune,
            decay: defaults.decay,
            tone: defaults.tone,
            level: defaults.level,
            click_coef: 0.0,
            drive_norm: 1.0 / math::soft_clip(DRIVE),
            active: false,
            low: Resonator::default(),
            high: Resonator::default(),
            click_env: 0.0,
            click_bp: Svf::default(),
            hp: Svf::default(),
        };
        rim.set_sample_rate(sample_rate);
        rim
    }

    /// Frequency multiplier the current `tune` resolves to (1 at 0.5).
    fn tune_factor(&self) -> f32 {
        math::exp_range(self.tune, TUNE_LOW_FACTOR, TUNE_LN_RATIO)
    }

    /// Low and high ping frequencies (Hz) for the current `tune`.
    #[must_use]
    pub fn ping_frequencies_hz(&self) -> (f32, f32) {
        let f = self.tune_factor();
        (LOW_PING_HZ * f, HIGH_PING_HZ * f)
    }

    /// Ring time of the low ping (seconds to −60 dB) for the current `decay`.
    #[must_use]
    pub fn decay_seconds(&self) -> f32 {
        math::exp_range(self.decay, DECAY_LOW_S, DECAY_LN_RATIO)
    }

    fn reset_state(&mut self) {
        self.active = false;
        self.low.reset();
        self.high.reset();
        self.click_env = 0.0;
        self.click_bp.reset();
        self.hp.reset();
    }
}

impl Voice for Rim {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = if sample_rate.is_finite() {
            sample_rate.max(1.0)
        } else {
            48_000.0
        };
        self.click_coef = math::tau_coefficient(CLICK_TAU_S, self.sample_rate);
        self.hp.set(HP_HZ, HP_Q, self.sample_rate);
        self.reset_state();
    }

    fn apply_params(&mut self, params: &VoiceParams) {
        self.tune = unit(params.tune, 0.5);
        self.decay = unit(params.decay, 0.5);
        self.tone = unit(params.tone, 0.5);
        self.level = unit(params.level, 0.0);
    }

    fn trigger(&mut self, velocity: f32) {
        let velocity = unit(velocity, 0.0);
        if velocity <= 0.0 {
            return;
        }
        let sr = self.sample_rate;
        let factor = self.tune_factor();
        let t60 = self.decay_seconds();
        self.low.tune(LOW_PING_HZ * factor, t60, sr);
        self.high
            .tune(HIGH_PING_HZ * factor, t60 * HIGH_DECAY_RATIO, sr);
        self.click_bp.set(CLICK_HZ * factor, CLICK_Q, sr);

        // Strike strength: a gentle curve so the accent stays audible after
        // the soft clip has compressed the loudest hits.
        let strike = velocity * (0.5 + 0.5 * velocity);
        let low_gain = LOW_GAIN_AT_0 + LOW_GAIN_SPAN * self.tone;
        let high_gain = HIGH_GAIN_AT_0 + HIGH_GAIN_SPAN * self.tone;
        self.low.strike(strike * low_gain);
        self.high.strike(strike * high_gain);
        self.click_env += CLICK_LEVEL * velocity * velocity;
        self.active = true;
    }

    #[inline]
    fn process(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }

        let low = self.low.tick();
        let high = self.high.tick();
        let click = self.click_bp.process(self.click_env).band * self.click_bp.k();
        self.click_env *= self.click_coef;

        let y = self.hp.process(low + high + click).high;
        let shaped = math::soft_clip(y * DRIVE) * self.drive_norm;

        if self.low.energy() < IDLE_ENERGY
            && self.high.energy() < IDLE_ENERGY
            && self.click_env < IDLE_LEVEL
            && y.abs() < IDLE_LEVEL
        {
            self.reset_state();
        }

        shaped * self.level * CALIBRATION
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

    fn render(rim: &mut Rim, n: usize) -> Vec<f32> {
        (0..n).map(|_| rim.process()).collect()
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    fn energy(samples: &[f32]) -> f64 {
        samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum()
    }

    fn with_params(sr: f32, tune: f32, decay: f32, tone: f32, level: f32) -> Rim {
        let mut rim = Rim::new(sr);
        rim.apply_params(&VoiceParams {
            tune,
            decay,
            tone,
            snappy: 0.5,
            level,
        });
        rim
    }

    fn hit(rim: &mut Rim, velocity: f32, n: usize) -> Vec<f32> {
        rim.trigger(velocity);
        render(rim, n)
    }

    /// Magnitude of the Hann-windowed signal at `hz` (single DFT bin).
    fn magnitude_at(samples: &[f32], sr: f32, hz: f32) -> f64 {
        let n = samples.len();
        let w = std::f64::consts::TAU * f64::from(hz) / f64::from(sr);
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &x) in samples.iter().enumerate() {
            let hann = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
            let p = w * i as f64;
            re += f64::from(x) * hann * p.cos();
            im -= f64::from(x) * hann * p.sin();
        }
        re.hypot(im)
    }

    /// Frequency of the strongest component between `lo` and `hi` Hz.
    fn dominant_hz(samples: &[f32], sr: f32, lo: f32, hi: f32) -> f32 {
        let mut best = (lo, 0.0f64);
        let mut hz = lo;
        while hz <= hi {
            let m = magnitude_at(samples, sr, hz);
            if m > best.1 {
                best = (hz, m);
            }
            hz += 2.0;
        }
        best.0
    }

    /// Magnitude-weighted spectral centroid (Goertzel per bin).
    fn centroid_hz(samples: &[f32], sr: f32) -> f64 {
        let n = samples.len();
        let windowed: Vec<f64> = samples
            .iter()
            .enumerate()
            .map(|(i, &x)| {
                f64::from(x) * (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos())
            })
            .collect();
        let (mut num, mut den) = (0.0f64, 0.0f64);
        for k in 1..n / 2 {
            let w = std::f64::consts::TAU * k as f64 / n as f64;
            let coef = 2.0 * w.cos();
            let (mut s1, mut s2) = (0.0f64, 0.0f64);
            for &x in &windowed {
                let s = x + coef * s1 - s2;
                s2 = s1;
                s1 = s;
            }
            let mag = (s1 * s1 + s2 * s2 - coef * s1 * s2).max(0.0).sqrt();
            num += mag * k as f64 * f64::from(sr) / n as f64;
            den += mag;
        }
        num / den
    }

    #[test]
    fn idle_voice_is_silent() {
        let mut rim = Rim::new(SR);
        assert!(!rim.is_active());
        assert!(render(&mut rim, 1_000).iter().all(|&s| s == 0.0));
        // A zero-velocity trigger does not start a hit.
        rim.trigger(0.0);
        assert!(!rim.is_active());
        assert!(render(&mut rim, 100).iter().all(|&s| s == 0.0));
    }

    #[test]
    fn full_hit_peaks_near_minus_10_dbfs() {
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            let mut rim = Rim::new(sr);
            let p = peak(&hit(&mut rim, 1.0, sr as usize / 5));
            let db = 20.0 * p.log10();
            assert!((-11.5..=-8.5).contains(&db), "{sr} Hz: peak {p} = {db} dBFS");
        }
    }

    #[test]
    fn output_is_finite_and_bounded_for_extreme_controls() {
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            for bits in 0..16u32 {
                let pick = |b: u32| if bits & (1 << b) != 0 { 1.0 } else { 0.0 };
                let mut rim = with_params(sr, pick(0), pick(1), pick(2), pick(3));
                for velocity in [0.1, 0.7, 1.0] {
                    rim.trigger(velocity);
                    for s in render(&mut rim, 256) {
                        assert!(s.is_finite() && s.abs() <= 1.0, "{s}");
                    }
                    // Retrigger on top of the ring, then let it die out.
                    rim.trigger(1.0);
                    rim.trigger(1.0);
                    for s in render(&mut rim, sr as usize / 4) {
                        assert!(s.is_finite() && s.abs() <= 1.0, "{s}");
                    }
                    assert!(!rim.is_active(), "{sr} Hz, controls {bits:04b}");
                }
            }
        }
    }

    #[test]
    fn decays_to_silence_goes_idle_and_returns_exact_zero() {
        let mut rim = with_params(SR, 0.0, 1.0, 0.0, 1.0); // longest ring
        let out = hit(&mut rim, 1.0, 9_600);
        assert!(!rim.is_active());
        let tail = peak(&out[7_200..]);
        assert!(tail < 1e-5, "tail {tail}");
        assert!(render(&mut rim, 4_800).iter().all(|&s| s == 0.0));
        // It goes idle within a sensible time (−100 dB of a 55 ms t60).
        let last_sound = out.iter().rposition(|&s| s != 0.0).unwrap();
        assert!(last_sound < 6_000, "rang for {last_sound} samples");
    }

    #[test]
    fn pings_sit_at_the_tuned_frequencies() {
        let mut rim = with_params(SR, 0.5, 1.0, 0.5, 1.0);
        let out = hit(&mut rim, 0.7, 4_096);
        let low = dominant_hz(&out, SR, 300.0, 900.0);
        let high = dominant_hz(&out, SR, 1_200.0, 2_400.0);
        assert!((low - 470.0).abs() < 20.0, "low ping {low} Hz");
        assert!((high - 1_660.0).abs() < 40.0, "high ping {high} Hz");
        assert_eq!(rim.ping_frequencies_hz(), (470.0, 1_660.0));
    }

    #[test]
    fn tune_raises_both_pings() {
        let pings = |tune: f32| {
            let mut rim = with_params(SR, tune, 1.0, 0.5, 1.0);
            let out = hit(&mut rim, 0.7, 4_096);
            (
                dominant_hz(&out, SR, 250.0, 1_000.0),
                dominant_hz(&out, SR, 1_050.0, 3_000.0),
            )
        };
        let (l0, h0) = pings(0.0);
        let (l5, h5) = pings(0.5);
        let (l1, h1) = pings(1.0);
        assert!(l0 < l5 * 0.8 && l5 < l1 * 0.8, "{l0} {l5} {l1}");
        assert!(h0 < h5 * 0.8 && h5 < h1 * 0.8, "{h0} {h5} {h1}");
    }

    #[test]
    fn decay_lengthens_the_tail() {
        let tail = |decay: f32| {
            let mut rim = with_params(SR, 0.5, decay, 0.5, 1.0);
            let out = hit(&mut rim, 1.0, 4_800);
            energy(&out[960..2_400]) // 20–50 ms
        };
        let (short, mid, long) = (tail(0.0), tail(0.5), tail(1.0));
        assert!(mid > short * 2.0 && long > mid * 2.0, "{short} {mid} {long}");
        // Subtle: the first 5 ms barely change.
        let head = |decay: f32| {
            let mut rim = with_params(SR, 0.5, decay, 0.5, 1.0);
            peak(&hit(&mut rim, 1.0, 240))
        };
        assert!((head(1.0) / head(0.0) - 1.0).abs() < 0.2);
    }

    #[test]
    fn tone_moves_the_spectral_centroid_up() {
        let centroid = |tone: f32| {
            let mut rim = with_params(SR, 0.5, 0.5, tone, 1.0);
            centroid_hz(&hit(&mut rim, 0.7, 2_048), SR)
        };
        let (c0, c5, c1) = (centroid(0.0), centroid(0.5), centroid(1.0));
        assert!(c0 < c5 && c5 < c1, "{c0} {c5} {c1}");
        assert!(c1 > c0 * 1.4, "{c0} -> {c1}");
    }

    #[test]
    fn velocity_raises_level_and_brightness() {
        let run = |velocity: f32| {
            let mut rim = Rim::new(SR);
            let out = hit(&mut rim, velocity, 2_048);
            (peak(&out), centroid_hz(&out, SR))
        };
        let (p_acc, c_acc) = run(1.0);
        let (p_norm, c_norm) = run(0.7);
        let (p_soft, _) = run(0.1);
        let accent_db = 20.0 * (p_acc / p_norm).log10();
        assert!((2.0..=6.0).contains(&accent_db), "accent adds {accent_db} dB");
        assert!(p_soft < p_norm * 0.2, "{p_soft} vs {p_norm}");
        assert!(c_acc > c_norm * 1.03, "centroid {c_norm} -> {c_acc}");
    }

    #[test]
    fn level_scales_output_immediately() {
        let mut a = Rim::new(SR);
        let mut b = with_params(SR, 0.5, 0.5, 0.5, 0.5);
        a.trigger(1.0);
        b.trigger(1.0);
        for _ in 0..500 {
            let (x, y) = (a.process(), b.process());
            assert!((x * 0.5 - y).abs() < 1e-7);
        }
        b.apply_params(&VoiceParams {
            level: 0.0,
            ..VoiceParams::default()
        });
        assert!(render(&mut b, 100).iter().all(|&s| s == 0.0));
    }

    #[test]
    fn output_has_no_dc_and_no_low_end() {
        let mut rim = with_params(SR, 0.0, 1.0, 0.0, 1.0);
        let out = hit(&mut rim, 1.0, 8_192);
        let mean = out.iter().map(|&s| f64::from(s)).sum::<f64>() / out.len() as f64;
        assert!(mean.abs() < 1e-4, "mean {mean}");
        let sub = magnitude_at(&out, SR, 80.0);
        let body = magnitude_at(&out, SR, 332.0);
        assert!(sub < body * 0.05, "80 Hz {sub} vs ping {body}");
    }

    #[test]
    fn renders_are_deterministic() {
        let run = || {
            let mut rim = with_params(SR, 0.3, 0.8, 0.6, 0.9);
            let mut out = hit(&mut rim, 1.0, 1_000);
            out.extend(hit(&mut rim, 0.42, 300));
            out.extend(hit(&mut rim, 0.7, 4_000));
            out
        };
        let (a, b) = (run(), run());
        assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
    }

    #[test]
    fn retrigger_mid_ring_has_no_discontinuity() {
        let max_step = |s: &[f32]| s.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            let at = (sr * 0.006) as usize; // 6 ms in: the ring is still loud
            let mut fresh = Rim::new(sr);
            let mut single = vec![0.0];
            single.extend(hit(&mut fresh, 1.0, 2_000));
            let fresh_step = max_step(&single);

            let mut rim = Rim::new(sr);
            let first = hit(&mut rim, 1.0, at);
            let ring_step = max_step(&first[first.len() / 2..]);
            let ring_level = peak(&first[first.len() - 100..]);
            assert!(ring_level > 0.02, "ring has died ({ring_level})");
            let mut joined = vec![first[at - 1]];
            joined.extend(hit(&mut rim, 1.0, 2_000));
            let retrig_step = max_step(&joined);
            // Striking a ringing resonator adds to it; the jump at the
            // retrigger is no worse than a fresh attack on top of the ring.
            assert!(
                retrig_step <= fresh_step + ring_step,
                "{sr} Hz: {retrig_step} vs fresh {fresh_step} + ring {ring_step}"
            );
            // The first retriggered sample continues the ring.
            assert!((joined[1] - joined[0]).abs() <= fresh_step);
        }
    }

    #[test]
    fn ten_seconds_render_quickly() {
        let mut rim = Rim::new(SR);
        let start = std::time::Instant::now();
        let mut acc = 0.0f32;
        for i in 0..480_000 {
            if i % 6_000 == 0 {
                rim.trigger(if i % 12_000 == 0 { 1.0 } else { 0.7 });
            }
            acc += rim.process();
        }
        let elapsed = start.elapsed();
        assert!(acc.is_finite());
        assert!(elapsed.as_millis() < 100, "10 s took {elapsed:?}");
    }
}
