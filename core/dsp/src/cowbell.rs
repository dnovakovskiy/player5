//! TR-inspired cowbell.
//!
//! The classic analogue circuit runs two square-wave oscillators a little
//! under a fifth apart, mixes them and shapes the mix with a band-pass filter
//! and a VCA. The VCA envelope has two parts: a sharp spike that falls away
//! within a few tens of milliseconds and a quieter tail that rings on. The
//! clash of the two squares' odd harmonics in the pass band is the metallic
//! clank; the tail is what makes it a bell.
//!
//! This model keeps those ingredients:
//!
//! * **oscillators** – two PolyBLEP squares ([`Square`]) at ≈ 540 Hz and
//!   ≈ 800 Hz at `tune = 0.5`, summed. A fresh hit starts them with their
//!   rising edges lined up 0.5 ms in, which is both the hardest attack and
//!   the highest crest the pair can reach, so every hit peaks at the same
//!   level; a retrigger keeps them running;
//! * **filter** – a state-variable band-pass around 2.5 kHz (the clank) plus a
//!   share of the same filter's low-pass output, so the two fundamentals stay
//!   audible underneath;
//! * **envelope** – a fast stage (−60 dB in 30 ms) plus a tail (`decay`),
//!   followed by a 0.25 ms slew so that neither a fresh hit nor a retrigger
//!   in the middle of a ringing tail clicks;
//! * **accent** – velocity sets the level and nudges the band-pass up, so
//!   accented hits are brighter as well as louder.
//!
//! Controls (all `0..=1`): `tune` shifts both oscillators by up to 1.3× either
//! way (−23 % to +30 %, equal ratios per unit of travel), `decay` sets the
//! tail (80–500 ms to −60 dB), `tone` moves the band-pass from 1.6 kHz to
//! 4 kHz and `level` scales the output. `snappy` is ignored. A full-velocity
//! hit at the default controls peaks near −12 dBFS.

use crate::blocks::{Square, Svf};
use crate::math;
use crate::voice::Voice;
use crate::VoiceParams;

/// Lower oscillator at `tune = 0.5`.
const LOW_OSC_HZ: f32 = 540.0;
/// Upper oscillator at `tune = 0.5`.
const HIGH_OSC_HZ: f32 = 800.0;
/// Tune factor at `tune = 0`: 1 / 1.3.
const TUNE_LOW_FACTOR: f32 = 0.769_230_769;
/// ln(1.3²): `tune = 1` is 1.3× the centre pitch.
const TUNE_LN_RATIO: f32 = 0.524_728_529;
/// On a fresh hit both squares are phased so that their rising edges coincide
/// this long after the trigger. Coinciding rising edges are where the filtered mix has its
/// highest crest, so a fresh hit opens on the same peak a retrigger at a
/// random phase could reach: every hit, flam or roll peaks at the calibrated
/// level, and the attack is a single hard edge.
const EDGE_ALIGN_S: f32 = 0.000_5;

/// Band-pass centre at `tone = 0`.
const BP_LOW_HZ: f32 = 1_600.0;
/// ln(2.5): `tone = 1` puts the band-pass at 4 kHz (2.53 kHz at 0.5).
const BP_LN_RATIO: f32 = 0.916_290_732;
const BP_Q: f32 = 1.4;
/// Mix of the band-pass (unity peak gain) and the low-pass fundamentals.
const BAND_GAIN: f32 = 1.0;
const FUNDAMENTAL_GAIN: f32 = 0.3;
/// How far velocity moves the band-pass, as `ln` of the frequency ratio per
/// unit of velocity, relative to an unaccented (0.7) hit. A full accent sits
/// 11 % higher.
const VELOCITY_BRIGHTNESS: f32 = 0.35;
const UNACCENTED_VELOCITY: f32 = 0.7;

/// Fast stage: share of the envelope peak and its fall time to −60 dB.
const FAST_SHARE: f32 = 0.65;
const FAST_T60_S: f32 = 0.03;
/// Tail: share of the envelope peak and its fall time at `decay = 0`.
const TAIL_SHARE: f32 = 0.35;
const TAIL_LOW_S: f32 = 0.08;
/// ln(6.25): `decay = 1` rings for 500 ms (200 ms at 0.5).
const TAIL_LN_RATIO: f32 = 1.832_581_464;
/// Time constant of the envelope slew (the attack).
const SLEW_TAU_S: f32 = 0.000_25;

/// Output scaling so a full hit at `level = 1` peaks near −12 dBFS.
const CALIBRATION: f32 = 0.146;
/// Envelope level below which the voice goes idle (−100 dB).
const IDLE_THRESHOLD: f32 = 1e-5;
/// Level below which the fast stage is flushed to exactly zero (−180 dB).
const FLUSH: f32 = 1e-9;

/// The cowbell voice. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct Cowbell {
    sample_rate: f32,
    tune: f32,
    decay: f32,
    tone: f32,
    level: f32,

    // Derived per sample rate.
    fast_coef: f32,
    slew_coef: f32,

    // Derived per trigger.
    tail_coef: f32,

    // State.
    active: bool,
    low_osc: Square,
    high_osc: Square,
    filter: Svf,
    fast: f32,
    tail: f32,
    env: f32,
}

/// Clamps a control to `0..=1`; non-finite values become `fallback`.
fn unit(x: f32, fallback: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        fallback
    }
}

impl Cowbell {
    /// Creates an idle voice for the given sample rate.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        let defaults = VoiceParams::default();
        let mut cowbell = Self {
            sample_rate,
            tune: defaults.tune,
            decay: defaults.decay,
            tone: defaults.tone,
            level: defaults.level,
            fast_coef: 0.0,
            slew_coef: 0.0,
            tail_coef: 0.0,
            active: false,
            low_osc: Square::default(),
            high_osc: Square::default(),
            filter: Svf::default(),
            fast: 0.0,
            tail: 0.0,
            env: 0.0,
        };
        cowbell.set_sample_rate(sample_rate);
        cowbell
    }

    /// Frequencies (Hz) of the two oscillators for the current `tune`.
    #[must_use]
    pub fn oscillator_frequencies_hz(&self) -> (f32, f32) {
        let f = math::exp_range(self.tune, TUNE_LOW_FACTOR, TUNE_LN_RATIO);
        (LOW_OSC_HZ * f, HIGH_OSC_HZ * f)
    }

    /// Tail time (seconds to −60 dB) for the current `decay`.
    #[must_use]
    pub fn decay_seconds(&self) -> f32 {
        math::exp_range(self.decay, TAIL_LOW_S, TAIL_LN_RATIO)
    }

    /// Band-pass centre (Hz) for the current `tone` at an unaccented hit.
    #[must_use]
    pub fn band_hz(&self) -> f32 {
        math::exp_range(self.tone, BP_LOW_HZ, BP_LN_RATIO)
    }

    fn reset_state(&mut self) {
        self.active = false;
        self.filter.reset();
        self.fast = 0.0;
        self.tail = 0.0;
        self.env = 0.0;
    }
}

impl Voice for Cowbell {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = if sample_rate.is_finite() {
            sample_rate.max(1.0)
        } else {
            48_000.0
        };
        self.fast_coef = math::decay_coefficient(FAST_T60_S, self.sample_rate);
        self.slew_coef = math::tau_coefficient(SLEW_TAU_S, self.sample_rate);
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
        let (low_hz, high_hz) = self.oscillator_frequencies_hz();
        if !self.active {
            // Fresh hit: start from a known state so every hit is the same.
            self.reset_state();
            self.low_osc.reset(-EDGE_ALIGN_S * low_hz);
            self.high_osc.reset(-EDGE_ALIGN_S * high_hz);
        }
        self.low_osc.set_frequency(low_hz, sr);
        self.high_osc.set_frequency(high_hz, sr);
        // Brightness follows the louder of the new hit and the ring it lands
        // on, so a soft hit does not dull a loud ring.
        let loudness = if self.active {
            velocity.max(self.env.min(1.0))
        } else {
            velocity
        };
        let brightness = math::exp(VELOCITY_BRIGHTNESS * (loudness - UNACCENTED_VELOCITY));
        self.filter.set(self.band_hz() * brightness, BP_Q, sr);
        self.tail_coef = math::decay_coefficient(self.decay_seconds(), sr);

        // Recharge both envelope stages; a softer hit landing on a louder
        // ring does not cut it short.
        self.fast = self.fast.max(FAST_SHARE * velocity);
        self.tail = self.tail.max(TAIL_SHARE * velocity);
        self.active = true;
    }

    #[inline]
    fn process(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }

        let x = self.low_osc.tick() + self.high_osc.tick();
        let f = self.filter.process(x);
        let y = f.band * self.filter.k() * BAND_GAIN + f.low * FUNDAMENTAL_GAIN;

        let target = self.fast + self.tail;
        self.env = target + self.slew_coef * (self.env - target);
        let out = y * self.env;

        self.fast *= self.fast_coef;
        if self.fast < FLUSH {
            self.fast = 0.0;
        }
        self.tail *= self.tail_coef;
        if self.fast == 0.0 && self.tail < IDLE_THRESHOLD && self.env < IDLE_THRESHOLD {
            self.reset_state();
        }

        out * self.level * CALIBRATION
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

    fn render(cowbell: &mut Cowbell, n: usize) -> Vec<f32> {
        (0..n).map(|_| cowbell.process()).collect()
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    fn energy(samples: &[f32]) -> f64 {
        samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum()
    }

    fn with_params(sr: f32, tune: f32, decay: f32, tone: f32, level: f32) -> Cowbell {
        let mut cowbell = Cowbell::new(sr);
        cowbell.apply_params(&VoiceParams {
            tune,
            decay,
            tone,
            snappy: 0.5,
            level,
        });
        cowbell
    }

    fn hit(cowbell: &mut Cowbell, velocity: f32, n: usize) -> Vec<f32> {
        cowbell.trigger(velocity);
        render(cowbell, n)
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

    /// Frequency (and magnitude) of the strongest component between `lo` and
    /// `hi` Hz, on a 1 Hz grid.
    fn dominant(samples: &[f32], sr: f32, lo: f32, hi: f32) -> (f32, f64) {
        let mut best = (lo, 0.0f64);
        let mut hz = lo;
        while hz <= hi {
            let m = magnitude_at(samples, sr, hz);
            if m > best.1 {
                best = (hz, m);
            }
            hz += 1.0;
        }
        best
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
        let mut cowbell = Cowbell::new(SR);
        assert!(!cowbell.is_active());
        assert!(render(&mut cowbell, 1_000).iter().all(|&s| s == 0.0));
        cowbell.trigger(0.0);
        assert!(!cowbell.is_active());
        assert!(render(&mut cowbell, 100).iter().all(|&s| s == 0.0));
    }

    #[test]
    fn full_hit_peaks_near_minus_12_dbfs() {
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            let mut cowbell = Cowbell::new(sr);
            let p = peak(&hit(&mut cowbell, 1.0, sr as usize / 2));
            let db = 20.0 * p.log10();
            assert!(
                (-13.5..=-10.5).contains(&db),
                "{sr} Hz: peak {p} = {db} dBFS"
            );
        }
    }

    #[test]
    fn rolls_and_flams_peak_at_the_calibrated_level() {
        // A fresh hit opens on the pair's highest crest, so retriggering at
        // whatever phase the oscillators have reached is no louder.
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            let mut cowbell = Cowbell::new(sr);
            let mut roll = 0.0f32;
            for i in 0..(sr as usize * 2) {
                if i % 351 == 0 {
                    cowbell.trigger(1.0);
                }
                roll = roll.max(cowbell.process().abs());
            }
            let db = 20.0 * roll.log10();
            assert!(
                (-13.5..=-10.5).contains(&db),
                "{sr} Hz: roll peaks at {db} dBFS"
            );

            let mut flam = 0.0f32;
            for gap in (384..1_920).step_by(37) {
                let mut cowbell = Cowbell::new(sr);
                cowbell.trigger(0.6);
                flam = flam.max(peak(&render(&mut cowbell, gap)));
                flam = flam.max(peak(&hit(&mut cowbell, 1.0, 4_800)));
            }
            let db = 20.0 * flam.log10();
            assert!(
                (-13.5..=-10.5).contains(&db),
                "{sr} Hz: flams peak at {db} dBFS"
            );
        }
    }

    #[test]
    fn output_is_finite_and_bounded_for_extreme_controls() {
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            for bits in 0..16u32 {
                let pick = |b: u32| if bits & (1 << b) != 0 { 1.0 } else { 0.0 };
                let mut cowbell = with_params(sr, pick(0), pick(1), pick(2), pick(3));
                for velocity in [0.1, 0.7, 1.0] {
                    cowbell.trigger(velocity);
                    for s in render(&mut cowbell, 512) {
                        assert!(s.is_finite() && s.abs() <= 1.0, "{s}");
                    }
                    // Retrigger on top of the ring, then let it die out.
                    cowbell.trigger(1.0);
                    cowbell.trigger(1.0);
                    for s in render(&mut cowbell, sr as usize) {
                        assert!(s.is_finite() && s.abs() <= 1.0, "{s}");
                    }
                    assert!(!cowbell.is_active(), "{sr} Hz, controls {bits:04b}");
                }
            }
        }
    }

    #[test]
    fn decays_to_silence_goes_idle_and_returns_exact_zero() {
        let mut cowbell = with_params(SR, 0.5, 1.0, 0.5, 1.0); // longest tail
        let out = hit(&mut cowbell, 1.0, 48_000);
        assert!(!cowbell.is_active());
        let tail = peak(&out[43_200..]);
        assert!(tail < 1e-5, "tail {tail}");
        assert!(render(&mut cowbell, 4_800).iter().all(|&s| s == 0.0));
        // −100 dB of a 500 ms tail is about 0.83 s.
        let last_sound = out.iter().rposition(|&s| s != 0.0).unwrap();
        assert!(last_sound < 43_200, "rang for {last_sound} samples");

        let mut short = with_params(SR, 0.5, 0.0, 0.5, 1.0);
        let out = hit(&mut short, 1.0, 14_400);
        assert!(!short.is_active());
        let last_sound = out.iter().rposition(|&s| s != 0.0).unwrap();
        assert!(
            last_sound < 9_600,
            "short tail rang for {last_sound} samples"
        );
    }

    #[test]
    fn never_produces_denormals() {
        let mut cowbell = with_params(SR, 0.0, 1.0, 0.0, 1.0);
        cowbell.trigger(1.0);
        for _ in 0..48_000 {
            let s = cowbell.process();
            assert!(!s.is_subnormal());
            for state in [cowbell.fast, cowbell.tail, cowbell.env] {
                assert!(!state.is_subnormal());
            }
        }
    }

    #[test]
    fn oscillators_sit_at_the_tuned_frequencies() {
        for (tune, low, high) in [
            (0.0, 415.4, 615.4),
            (0.5, 540.0, 800.0),
            (1.0, 702.0, 1_040.0),
        ] {
            let mut cowbell = with_params(SR, tune, 1.0, 0.5, 1.0);
            let (lo_hz, hi_hz) = cowbell.oscillator_frequencies_hz();
            assert!((lo_hz - low).abs() < 0.5 && (hi_hz - high).abs() < 0.5);
            let out = hit(&mut cowbell, 0.7, 9_600);
            for expect in [low, high] {
                let (found, mag) = dominant(&out, SR, expect * 0.9, expect * 1.1);
                assert!(
                    (found - expect).abs() < expect * 0.01,
                    "tune {tune}: peak at {found} Hz, expected {expect}"
                );
                // A real spectral peak, not just the edge of the window.
                let edge = magnitude_at(&out, SR, expect * 0.9);
                assert!(mag > edge * 10.0, "tune {tune}: {mag} vs edge {edge}");
            }
        }
    }

    #[test]
    fn tune_raises_the_dominant_frequency() {
        let dominant_hz = |tune: f32| {
            let mut cowbell = with_params(SR, tune, 1.0, 0.5, 1.0);
            dominant(&hit(&mut cowbell, 0.7, 9_600), SR, 300.0, 1_200.0).0
        };
        let (f0, f5, f1) = (dominant_hz(0.0), dominant_hz(0.5), dominant_hz(1.0));
        assert!(f0 < f5 && f5 < f1, "{f0} {f5} {f1}");
        assert!((f1 / f0 - 1.69).abs() < 0.05, "range {f0} -> {f1}");
    }

    #[test]
    fn decay_lengthens_the_tail() {
        let tail = |decay: f32| {
            let mut cowbell = with_params(SR, 0.5, decay, 0.5, 1.0);
            let out = hit(&mut cowbell, 1.0, 24_000);
            energy(&out[7_200..14_400]) // 150–300 ms
        };
        let (short, mid, long) = (tail(0.0), tail(0.5), tail(1.0));
        assert!(
            mid > short * 10.0 && long > mid * 4.0,
            "{short} {mid} {long}"
        );
    }

    #[test]
    fn envelope_drops_fast_then_rings() {
        let mut cowbell = Cowbell::new(SR); // 200 ms tail
        let out = hit(&mut cowbell, 1.0, 24_000);
        // RMS level (dB) between two times in ms.
        let level = |a: usize, b: usize| {
            let seg = &out[a * 48..b * 48];
            10.0 * (energy(seg) / seg.len() as f64).log10()
        };
        // Fall rates (dB per ms) over the first 20 ms and later in the tail.
        let fast_rate = (level(0, 5) - level(15, 20)) / 15.0;
        let tail_rate = (level(50, 60) - level(150, 160)) / 100.0;
        assert!(
            fast_rate > tail_rate * 2.0,
            "{fast_rate} vs {tail_rate} dB/ms"
        );
        // The tail falls 60 dB in 200 ms.
        assert!(
            (tail_rate - 0.3).abs() < 0.03,
            "tail falls {tail_rate} dB/ms"
        );
        // The attack stands well clear of the tail extrapolated back to it.
        let tail_at_start = level(50, 60) + tail_rate * 52.5;
        let spike = level(0, 5) - tail_at_start;
        assert!((5.0..=12.0).contains(&spike), "attack spike {spike} dB");
    }

    #[test]
    fn tone_moves_the_spectral_centroid_up() {
        let centroid = |tone: f32| {
            let mut cowbell = with_params(SR, 0.5, 0.5, tone, 1.0);
            centroid_hz(&hit(&mut cowbell, 0.7, 4_096), SR)
        };
        let (c0, c5, c1) = (centroid(0.0), centroid(0.5), centroid(1.0));
        assert!(c0 < c5 && c5 < c1, "{c0} {c5} {c1}");
        assert!(c1 > c0 * 1.3, "{c0} -> {c1}");
    }

    #[test]
    fn band_pass_shapes_the_clank() {
        // At the default tone the harmonics around 2.5 kHz stand out against
        // the plain square spectrum (where the 5th harmonic of the lower
        // oscillator is 14 dB under its fundamental).
        let mut cowbell = Cowbell::new(SR);
        let out = hit(&mut cowbell, 0.7, 4_096);
        let fundamental = magnitude_at(&out, SR, 540.0);
        let fifth = magnitude_at(&out, SR, 2_700.0);
        assert!(fifth > fundamental * 0.5, "{fifth} vs {fundamental}");
        // The fundamentals are still there.
        let between = magnitude_at(&out, SR, 1_100.0);
        assert!(fundamental > between * 4.0, "{fundamental} vs {between}");
    }

    #[test]
    fn velocity_raises_level_and_brightness() {
        let run = |velocity: f32| {
            let mut cowbell = Cowbell::new(SR);
            let out = hit(&mut cowbell, velocity, 4_096);
            (peak(&out), centroid_hz(&out, SR))
        };
        let (p_acc, c_acc) = run(1.0);
        let (p_norm, c_norm) = run(0.7);
        let (p_soft, _) = run(0.1);
        let accent_db = 20.0 * (p_acc / p_norm).log10();
        assert!(
            (2.0..=6.0).contains(&accent_db),
            "accent adds {accent_db} dB"
        );
        assert!(p_soft < p_norm * 0.2, "{p_soft} vs {p_norm}");
        assert!(c_acc > c_norm * 1.02, "centroid {c_norm} -> {c_acc}");
    }

    #[test]
    fn level_scales_output_immediately() {
        let mut a = Cowbell::new(SR);
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
    fn output_has_no_dc() {
        let mut cowbell = with_params(SR, 0.0, 1.0, 0.0, 1.0);
        let out = hit(&mut cowbell, 1.0, 48_000);
        let mean = out.iter().map(|&s| f64::from(s)).sum::<f64>() / out.len() as f64;
        assert!(mean.abs() < 1e-4, "mean {mean}");
    }

    #[test]
    fn renders_are_deterministic() {
        let run = || {
            let mut cowbell = with_params(SR, 0.3, 0.8, 0.6, 0.9);
            let mut out = hit(&mut cowbell, 1.0, 1_000);
            out.extend(hit(&mut cowbell, 0.42, 300));
            out.extend(hit(&mut cowbell, 0.7, 30_000));
            out.extend(hit(&mut cowbell, 0.7, 1_000));
            out
        };
        let (a, b) = (run(), run());
        assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
    }

    #[test]
    fn retrigger_mid_ring_has_no_discontinuity() {
        let max_step = |s: &[f32]| s.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            let mut fresh = Cowbell::new(sr);
            let mut single = vec![0.0];
            single.extend(hit(&mut fresh, 1.0, sr as usize / 10));
            let fresh_step = max_step(&single);

            for at_ms in [3.0, 30.0, 120.0] {
                let at = (sr * at_ms / 1_000.0) as usize;
                let mut cowbell = Cowbell::new(sr);
                let first = hit(&mut cowbell, 1.0, at);
                assert!(cowbell.is_active());
                let mut joined = vec![first[at - 1]];
                joined.extend(hit(&mut cowbell, 1.0, sr as usize / 10));
                // The oscillators and filter keep running and the envelope
                // slews to its new peak, so a retrigger is no sharper than
                // the hit itself.
                let retrig_step = max_step(&joined);
                assert!(
                    retrig_step <= fresh_step * 1.05,
                    "{sr} Hz at {at_ms} ms: {retrig_step} vs fresh {fresh_step}"
                );
            }

            // A soft hit on a louder ring adds its own small clank but never
            // cuts the ring short or dulls it: the next 10 ms carry at least
            // the energy of the ring left alone.
            for at_ms in [3.0, 30.0] {
                let at = (sr * at_ms / 1_000.0) as usize;
                let window = (sr * 0.01) as usize;
                let mut alone = Cowbell::new(sr);
                let reference = energy(&hit(&mut alone, 1.0, at + window)[at..]);
                let mut loud = Cowbell::new(sr);
                hit(&mut loud, 1.0, at);
                let soft = energy(&hit(&mut loud, 0.1, window));
                let ratio_db = 10.0 * (soft / reference).log10();
                assert!(
                    (-0.5..6.0).contains(&ratio_db),
                    "{sr} Hz at {at_ms} ms: soft retrigger changes the ring by {ratio_db} dB"
                );
            }
        }
    }

    #[test]
    fn ten_seconds_render_quickly() {
        let mut cowbell = Cowbell::new(SR);
        let start = std::time::Instant::now();
        let mut acc = 0.0f32;
        for i in 0..480_000 {
            if i % 6_000 == 0 {
                cowbell.trigger(if i % 12_000 == 0 { 1.0 } else { 0.7 });
            }
            acc += cowbell.process();
        }
        let elapsed = start.elapsed();
        assert!(acc.is_finite());
        assert!(elapsed.as_millis() < 100, "10 s took {elapsed:?}");
    }
}
