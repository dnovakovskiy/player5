//! TR-inspired hi-hats, closed and open.
//!
//! The classic analogue hat has no noise source at all. Six free-running
//! square-wave oscillators at unrelated frequencies are summed into a dense,
//! inharmonic "metal" signal whose odd harmonics interleave all the way up
//! the spectrum. A band-pass filter picks out the sizzle around 7–10 kHz, a
//! VCA shapes it with a short (closed) or long (open) decay, and a high-pass
//! after the VCA removes everything the low oscillator fundamentals and the
//! envelope itself would otherwise leak into the low end.
//!
//! This model follows that signal path:
//!
//! * **metal** – six [`Square`] oscillators (PolyBLEP, 50 % duty) at
//!   [`METAL_FREQS_HZ`] times the tune ratio. They are never reset, so like
//!   the hardware every hit catches the cluster at a slightly different
//!   phase; renders stay deterministic because the phases only depend on
//!   what was played before.
//! * **band-pass** – a 2-pole [`Svf`] band-pass, unity gain at its centre.
//!   `tone` moves the centre from 6.5 to 11 kHz; velocity pushes it a little
//!   higher, so accents are brighter as well as louder.
//! * **VCA** – an exponential decay with a ~0.1 ms attack smoother, which
//!   keeps retriggers and chokes free of steps.
//! * **high-pass** – a 4-pole Butterworth high-pass (two [`Svf`] stages)
//!   at 6–7.8 kHz that tracks `tone` at half the rate. Everything below
//!   2 kHz ends up more than 30 dB under the main band (about 45 dB in
//!   practice).
//! * **low-pass** – a 2-pole Butterworth at 16 kHz that removes the
//!   ultrasonic harmonics a 96 kHz render would otherwise keep, so every
//!   sample rate sounds the same.
//!
//! Controls are normalised `0..=1`: `tune` (the whole cluster ±25 %),
//! `decay` (60 dB fall time: closed 40–120 ms, open 0.25–1.5 s), `tone` and
//! `level`. `snappy` is ignored. Pitch, decay and tone take effect on the
//! next trigger; `level` applies immediately.
//!
//! A full-velocity hit at `level = 1.0` peaks close to −14 dBFS for both
//! hats, well under the kick. Being metal, no two hits are identical: at
//! 48 kHz single-hit peaks spread about ±3 dB around that, and a 96 kHz
//! render reads 1–1.5 dB hotter because its samples land closer to the
//! true peaks of a signal that lives at 7–12 kHz. The kit calls
//! [`OpenHat`]'s [`Voice::choke`] whenever the closed hat triggers; the open
//! hat then fades out over about 5 ms.

use crate::blocks::{Square, Svf};
use crate::math;
use crate::voice::Voice;
use crate::VoiceParams;

/// Frequencies (Hz) of the six square oscillators at `tune = 0.5`.
pub const METAL_FREQS_HZ: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];
const OSC_COUNT_INV: f32 = 1.0 / 6.0;

/// Tune ratio at `tune = 0`; `tune = 1` gives 1.25, `tune = 0.5` gives 1.
const TUNE_LOW: f32 = 0.8;
/// ln(1.25 / 0.8).
const TUNE_LN_RATIO: f32 = 0.446_287_103;

/// Band-pass centre at `tone = 0` (and full velocity); `tone = 1` is 11 kHz.
const BAND_LOW_HZ: f32 = 6_500.0;
/// ln(11 000 / 6 500).
const BAND_LN_RATIO: f32 = 0.526_093_096;
/// High-pass corner at `tone = 0`; `tone = 1` is about 7.8 kHz.
const HIGHPASS_LOW_HZ: f32 = 6_000.0;
/// Half of [`BAND_LN_RATIO`]: the high-pass moves half as many octaves.
const HIGHPASS_LN_RATIO: f32 = 0.263_046_548;
/// Stage Qs of a 4th-order Butterworth high-pass.
const HIGHPASS_Q: [f32; 2] = [0.541_196_100, 1.306_562_965];
/// Corner of the 2-pole Butterworth low-pass that rolls off the
/// ultrasonic harmonics, so the hats sound the same at every sample rate.
const LOWPASS_HZ: f32 = 16_000.0;
/// Q of a 2nd-order Butterworth, 1/√2.
const BUTTERWORTH_Q: f32 = core::f32::consts::FRAC_1_SQRT_2;

/// ln of how far a full-velocity hit raises the band centre over a
/// zero-velocity one (×1.4). An unaccented hit (0.7) sits about 10 % lower
/// than a full accent, which is where `tone` is specified.
const VELOCITY_BRIGHT_LN: f32 = 0.336_472_237;

/// Time constant of the VCA attack smoother.
const ATTACK_TAU_S: f32 = 0.000_1;
/// 60 dB fall time once choked.
const CHOKE_T60_S: f32 = 0.005;
/// Envelope level below which the voice goes idle (−100 dB).
const IDLE_THRESHOLD: f32 = 1e-5;

/// What distinguishes the closed hat from the open one.
#[derive(Clone, Copy, Debug)]
struct Shape {
    /// 60 dB fall time at `decay = 0`.
    decay_low_s: f32,
    /// ln(longest / shortest fall time).
    decay_ln_ratio: f32,
    /// Band-pass quality factor.
    band_q: f32,
    /// Starting phases (turns) of the oscillators, distinct per hat so the
    /// two never start in lockstep.
    phases: [f32; 6],
    /// Output scaling so a full hit at `level = 1` peaks near −14 dBFS.
    calibration: f32,
}

const CLOSED: Shape = Shape {
    decay_low_s: 0.04,
    // ln(0.12 / 0.04)
    decay_ln_ratio: 1.098_612_289,
    band_q: 1.0,
    phases: [0.0, 0.37, 0.71, 0.13, 0.52, 0.89],
    calibration: 1.762,
};

const OPEN: Shape = Shape {
    decay_low_s: 0.25,
    // ln(1.5 / 0.25)
    decay_ln_ratio: 1.791_759_469,
    band_q: 1.0,
    phases: [0.25, 0.61, 0.08, 0.83, 0.44, 0.97],
    calibration: 1.404,
};

/// Clamps a control to `0..=1`; non-finite values become 0.
fn unit(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The signal path both hats share.
#[derive(Clone, Debug)]
struct HatCore {
    shape: Shape,
    sample_rate: f32,
    params: VoiceParams,

    // Derived per sample rate.
    attack_coef: f32,
    choke_coef: f32,

    // Derived per trigger.
    env_coef: f32,

    // State.
    active: bool,
    oscs: [Square; 6],
    band: Svf,
    band_k: f32,
    highpass: [Svf; 2],
    lowpass: Svf,
    /// Envelope target: jumps to the velocity on trigger, then decays.
    env: f32,
    /// Smoothed VCA gain following `env`.
    gain: f32,
}

impl HatCore {
    fn new(shape: Shape, sample_rate: f32) -> Self {
        let mut core = Self {
            shape,
            sample_rate,
            params: VoiceParams::default(),
            attack_coef: 1.0,
            choke_coef: 0.0,
            env_coef: 0.0,
            active: false,
            oscs: Default::default(),
            band: Svf::default(),
            band_k: 1.0,
            highpass: [Svf::default(), Svf::default()],
            lowpass: Svf::default(),
            env: 0.0,
            gain: 0.0,
        };
        core.set_sample_rate(sample_rate);
        core
    }

    fn tune_ratio(&self) -> f32 {
        math::exp_range(self.params.tune, TUNE_LOW, TUNE_LN_RATIO)
    }

    fn decay_seconds(&self) -> f32 {
        math::exp_range(
            self.params.decay,
            self.shape.decay_low_s,
            self.shape.decay_ln_ratio,
        )
    }

    fn band_center_hz(&self, velocity: f32) -> f32 {
        math::exp_range(self.params.tone, BAND_LOW_HZ, BAND_LN_RATIO)
            * math::exp(VELOCITY_BRIGHT_LN * (unit(velocity) - 1.0))
    }

    fn highpass_hz(&self) -> f32 {
        math::exp_range(self.params.tone, HIGHPASS_LOW_HZ, HIGHPASS_LN_RATIO)
    }

    /// Silences the voice and clears the filters. The oscillators keep
    /// their phases.
    fn reset_state(&mut self) {
        self.active = false;
        self.env = 0.0;
        self.gain = 0.0;
        self.band.reset();
        for hp in &mut self.highpass {
            hp.reset();
        }
        self.lowpass.reset();
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.attack_coef = 1.0 - math::tau_coefficient(ATTACK_TAU_S, self.sample_rate);
        self.choke_coef = math::decay_coefficient(CHOKE_T60_S, self.sample_rate);
        self.lowpass
            .set(LOWPASS_HZ, BUTTERWORTH_Q, self.sample_rate);
        for ((osc, &hz), &phase) in self
            .oscs
            .iter_mut()
            .zip(&METAL_FREQS_HZ)
            .zip(&self.shape.phases)
        {
            osc.set_frequency(hz, self.sample_rate);
            osc.reset(phase);
        }
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
    }

    fn trigger(&mut self, velocity: f32) {
        let velocity = unit(velocity);
        if velocity <= 0.0 {
            // A silent hit changes nothing; a ringing hat keeps ringing.
            return;
        }
        let sr = self.sample_rate;
        let ratio = self.tune_ratio();
        for (osc, &hz) in self.oscs.iter_mut().zip(&METAL_FREQS_HZ) {
            osc.set_frequency(hz * ratio, sr);
        }
        self.band
            .set(self.band_center_hz(velocity), self.shape.band_q, sr);
        self.band_k = self.band.k();
        let hp_hz = self.highpass_hz();
        for (hp, &q) in self.highpass.iter_mut().zip(&HIGHPASS_Q) {
            hp.set(hp_hz, q, sr);
        }
        self.env_coef = math::decay_coefficient(self.decay_seconds(), sr);

        // Soft retrigger: oscillators, filters and the VCA gain carry on;
        // only the envelope target jumps, and the attack smoother glides
        // the gain up to it.
        self.env = velocity;
        self.active = true;
    }

    fn choke(&mut self) {
        if self.active {
            self.env_coef = self.choke_coef;
        }
    }

    #[inline]
    fn process(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }

        let mut metal = 0.0f32;
        for osc in &mut self.oscs {
            metal += osc.tick();
        }
        let band = self.band.process(metal * OSC_COUNT_INV).band * self.band_k;

        self.gain += self.attack_coef * (self.env - self.gain);
        self.env *= self.env_coef;
        let vca = band * self.gain;

        let [hp1, hp2] = &mut self.highpass;
        let high = hp2.process(hp1.process(vca).high).high;
        let out = self.lowpass.process(high).low;

        if self.env < IDLE_THRESHOLD && self.gain < IDLE_THRESHOLD {
            self.reset_state();
        }

        out * self.params.level * self.shape.calibration
    }
}

/// Closed hi-hat. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct ClosedHat {
    core: HatCore,
}

impl ClosedHat {
    /// Creates an idle voice for the given sample rate.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        Self {
            core: HatCore::new(CLOSED, sample_rate),
        }
    }

    /// Current controls.
    #[must_use]
    pub fn params(&self) -> VoiceParams {
        self.core.params
    }

    /// Fall time (seconds to −60 dB) the current `decay` resolves to.
    #[must_use]
    pub fn decay_seconds(&self) -> f32 {
        self.core.decay_seconds()
    }

    /// Band-pass centre (Hz) a hit at `velocity` would use.
    #[must_use]
    pub fn band_center_hz(&self, velocity: f32) -> f32 {
        self.core.band_center_hz(velocity)
    }

    /// Factor the current `tune` applies to [`METAL_FREQS_HZ`].
    #[must_use]
    pub fn tune_ratio(&self) -> f32 {
        self.core.tune_ratio()
    }
}

impl Voice for ClosedHat {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.core.set_sample_rate(sample_rate);
    }

    fn apply_params(&mut self, params: &VoiceParams) {
        self.core.apply_params(params);
    }

    fn trigger(&mut self, velocity: f32) {
        self.core.trigger(velocity);
    }

    #[inline]
    fn process(&mut self) -> f32 {
        self.core.process()
    }

    #[inline]
    fn is_active(&self) -> bool {
        self.core.active
    }

    /// Fades a ringing hat out over about 5 ms, like the open hat. The kit
    /// never chokes the closed hat itself.
    fn choke(&mut self) {
        self.core.choke();
    }
}

/// Open hi-hat. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct OpenHat {
    core: HatCore,
}

impl OpenHat {
    /// Creates an idle voice for the given sample rate.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        Self {
            core: HatCore::new(OPEN, sample_rate),
        }
    }

    /// Current controls.
    #[must_use]
    pub fn params(&self) -> VoiceParams {
        self.core.params
    }

    /// Fall time (seconds to −60 dB) the current `decay` resolves to.
    #[must_use]
    pub fn decay_seconds(&self) -> f32 {
        self.core.decay_seconds()
    }

    /// Band-pass centre (Hz) a hit at `velocity` would use.
    #[must_use]
    pub fn band_center_hz(&self, velocity: f32) -> f32 {
        self.core.band_center_hz(velocity)
    }

    /// Factor the current `tune` applies to [`METAL_FREQS_HZ`].
    #[must_use]
    pub fn tune_ratio(&self) -> f32 {
        self.core.tune_ratio()
    }
}

impl Voice for OpenHat {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.core.set_sample_rate(sample_rate);
    }

    fn apply_params(&mut self, params: &VoiceParams) {
        self.core.apply_params(params);
    }

    fn trigger(&mut self, velocity: f32) {
        self.core.trigger(velocity);
    }

    #[inline]
    fn process(&mut self) -> f32 {
        self.core.process()
    }

    #[inline]
    fn is_active(&self) -> bool {
        self.core.active
    }

    /// Fades a ringing hat out over about 5 ms (60 dB), without a click.
    fn choke(&mut self) {
        self.core.choke();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    fn db(x: f64) -> f64 {
        10.0 * x.max(1e-30).log10()
    }

    fn hat(open: bool, sr: f32) -> Box<dyn Voice> {
        if open {
            Box::new(OpenHat::new(sr))
        } else {
            Box::new(ClosedHat::new(sr))
        }
    }

    fn render(voice: &mut dyn Voice, n: usize) -> Vec<f32> {
        (0..n).map(|_| voice.process()).collect()
    }

    /// A fresh voice with `params`, hit once at `velocity`, rendered for `n`
    /// samples.
    fn hit(open: bool, sr: f32, params: VoiceParams, velocity: f32, n: usize) -> Vec<f32> {
        let mut v = hat(open, sr);
        v.apply_params(&params);
        v.trigger(velocity);
        render(v.as_mut(), n)
    }

    fn fft(re: &mut [f64], im: &mut [f64]) {
        let n = re.len();
        let mut j = 0;
        for i in 1..n {
            let mut bit = n >> 1;
            while j & bit != 0 {
                j ^= bit;
                bit >>= 1;
            }
            j |= bit;
            if i < j {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let ang = -std::f64::consts::TAU / len as f64;
            for start in (0..n).step_by(len) {
                for k in 0..len / 2 {
                    let (wi, wr) = (ang * k as f64).sin_cos();
                    let (a, b) = (start + k, start + k + len / 2);
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len <<= 1;
        }
    }

    /// Summed power spectrum over Hann-windowed frames: `(hz, power)` per
    /// bin up to Nyquist.
    fn spectrum(x: &[f32], sr: f32, size: usize) -> Vec<(f64, f64)> {
        let mut acc = vec![0.0f64; size / 2];
        let mut re = vec![0.0f64; size];
        let mut im = vec![0.0f64; size];
        let mut start = 0;
        while start < x.len() {
            for i in 0..size {
                let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / size as f64).cos();
                re[i] = x.get(start + i).map_or(0.0, |&s| f64::from(s)) * w;
                im[i] = 0.0;
            }
            fft(&mut re, &mut im);
            for (bin, a) in acc.iter_mut().enumerate() {
                *a += re[bin] * re[bin] + im[bin] * im[bin];
            }
            start += size / 2;
        }
        let bin_hz = f64::from(sr) / size as f64;
        acc.into_iter()
            .enumerate()
            .map(|(bin, p)| (bin as f64 * bin_hz, p))
            .collect()
    }

    fn band_power(spec: &[(f64, f64)], lo: f64, hi: f64) -> f64 {
        spec.iter()
            .filter(|(hz, _)| *hz >= lo && *hz < hi)
            .map(|(_, p)| p)
            .sum()
    }

    fn centroid(spec: &[(f64, f64)]) -> f64 {
        let total: f64 = spec.iter().map(|(_, p)| p).sum();
        spec.iter().map(|(hz, p)| hz * p).sum::<f64>() / total
    }

    fn params(tune: f32, decay: f32, tone: f32, level: f32) -> VoiceParams {
        VoiceParams {
            tune,
            decay,
            tone,
            snappy: 0.5,
            level,
        }
    }

    /// Largest sample-to-sample step.
    fn max_step(samples: &[f32]) -> f32 {
        samples
            .windows(2)
            .fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()))
    }

    fn rms_db(samples: &[f32]) -> f64 {
        let p = samples.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>() / samples.len() as f64;
        db(p)
    }

    /// Peaks (dBFS) of `hits` consecutive full-velocity hits, each rendered
    /// until the voice is idle, sorted.
    fn hit_peaks_db(open: bool, sr: f32, hits: usize) -> Vec<f32> {
        let mut v = hat(open, sr);
        let mut peaks: Vec<f32> = (0..hits)
            .map(|_| {
                v.trigger(1.0);
                let mut p = 0.0f32;
                while v.is_active() {
                    p = p.max(v.process().abs());
                }
                20.0 * p.log10()
            })
            .collect();
        peaks.sort_by(f32::total_cmp);
        peaks
    }

    #[test]
    fn idle_voices_are_silent() {
        for open in [false, true] {
            let mut v = hat(open, SR);
            assert!(!v.is_active());
            assert!(render(v.as_mut(), 1_000).iter().all(|&s| s == 0.0));
            // A silent hit and a choke leave it idle.
            v.trigger(0.0);
            v.choke();
            assert!(!v.is_active());
            assert!(render(v.as_mut(), 1_000).iter().all(|&s| s == 0.0));
        }
    }

    #[test]
    fn full_hit_peaks_near_minus_14_dbfs() {
        let window = -15.5..=-12.5;
        for open in [false, true] {
            // The first hit from a fresh voice is fully deterministic.
            let first = 20.0 * peak(&hit(open, SR, VoiceParams::default(), 1.0, 24_000)).log10();
            assert!(
                window.contains(&first),
                "open={open}: first hit {first} dBFS"
            );
            // Free-running oscillators make every hit a little different;
            // the typical hit sits in the window at every rate, and none is
            // far outside it.
            for sr in crate::SUPPORTED_SAMPLE_RATES {
                let peaks = hit_peaks_db(open, sr, 32);
                let median = peaks[16];
                assert!(
                    window.contains(&median),
                    "open={open} sr={sr}: median peak {median} dBFS"
                );
                assert!(
                    peaks[0] > -19.5 && peaks[31] < -8.5,
                    "open={open} sr={sr}: peaks {peaks:?}"
                );
            }
        }
    }

    #[test]
    fn closed_and_open_sit_at_the_same_level() {
        let closed = hit_peaks_db(false, SR, 32)[16];
        let open = hit_peaks_db(true, SR, 32)[16];
        assert!((closed - open).abs() < 1.0, "closed {closed}, open {open}");
    }

    #[test]
    fn output_is_finite_and_bounded() {
        for open in [false, true] {
            for sr in crate::SUPPORTED_SAMPLE_RATES {
                let mut v = hat(open, sr);
                for bits in 0..16u8 {
                    let corner = |bit: u8| f32::from((bits >> bit) & 1);
                    v.apply_params(&params(corner(0), corner(1), corner(2), corner(3)));
                    for velocity in [0.1, 0.7, 1.0] {
                        v.trigger(velocity);
                        for s in render(v.as_mut(), (sr * 0.3) as usize) {
                            assert!(
                                s.is_finite() && s.abs() <= 1.0,
                                "open={open} sr={sr} params {bits:04b} v={velocity}: {s}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn non_finite_inputs_are_harmless() {
        for open in [false, true] {
            let mut v = hat(open, SR);
            v.apply_params(&params(
                f32::NAN,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NAN,
            ));
            v.trigger(f32::NAN);
            assert!(!v.is_active());
            v.trigger(2.0);
            // A NaN level reads as 0: silent but well-formed.
            assert!(render(v.as_mut(), 4_800).iter().all(|&s| s == 0.0));
            v.apply_params(&VoiceParams::default());
            v.trigger(1.0);
            assert!(render(v.as_mut(), 4_800)
                .iter()
                .all(|s| s.is_finite() && s.abs() <= 1.0));
        }
    }

    #[test]
    fn decays_to_silence_and_goes_idle() {
        // -100 dB comes at 5/3 of the 60 dB fall time, plus the attack.
        for (open, decay, limit_s) in [(false, 0.0, 0.08), (false, 1.0, 0.22), (true, 1.0, 2.6)] {
            let n = (SR * (limit_s + 0.5)) as usize;
            let mut v = hat(open, SR);
            v.apply_params(&params(0.5, decay, 0.5, 1.0));
            v.trigger(1.0);
            let out = render(v.as_mut(), n);
            assert!(!v.is_active(), "open={open} decay={decay}");
            let idle_at = out.iter().rposition(|&s| s != 0.0).unwrap() + 1;
            assert!(
                idle_at as f32 <= SR * limit_s,
                "open={open} decay={decay}: idle after {idle_at} samples"
            );
            assert!(out[idle_at..].iter().all(|&s| s == 0.0));
            // The last 10 ms before going idle are ~100 dB down.
            let tail = peak(&out[idle_at - 480..idle_at]);
            assert!(tail < peak(&out) * 1e-4, "open={open}: tail {tail}");
        }
    }

    #[test]
    fn decay_maps_to_the_documented_ranges() {
        let closed = ClosedHat::new(SR);
        let open = OpenHat::new(SR);
        assert!((closed.decay_seconds() - 0.069_28).abs() < 1e-3);
        assert!((open.decay_seconds() - 0.612_4).abs() < 1e-3);
        for (open, decay, t60) in [
            (false, 0.0, 0.04),
            (false, 1.0, 0.12),
            (true, 0.0, 0.25),
            (true, 1.0, 1.5),
        ] {
            let mut v = hat(open, SR);
            v.apply_params(&params(0.5, decay, 0.5, 1.0));
            v.trigger(1.0);
            let out = render(v.as_mut(), (SR * 2.0) as usize);
            // Two short windows a third of t60 apart: 20 dB if the
            // envelope is right.
            let at = |t: f32| {
                let start = (SR * t) as usize;
                rms_db(&out[start..start + 240])
            };
            let fall = at(t60 * 0.1) - at(t60 * 0.1 + t60 / 3.0);
            assert!(
                (fall - 20.0).abs() < 4.0,
                "open={open} decay={decay}: fell {fall} dB"
            );
        }
    }

    #[test]
    fn longer_decay_rings_longer() {
        for (open, from_s, to_s) in [(false, 0.03, 0.06), (true, 0.2, 0.4)] {
            let tail = |decay: f32| {
                let out = hit(
                    open,
                    SR,
                    params(0.5, decay, 0.5, 1.0),
                    1.0,
                    (SR * to_s) as usize,
                );
                rms_db(&out[(SR * from_s) as usize..])
            };
            let (short, mid, long) = (tail(0.0), tail(0.5), tail(1.0));
            assert!(
                mid > short + 6.0 && long > mid + 6.0,
                "open={open}: {short} {mid} {long} dB"
            );
        }
    }

    #[test]
    fn tone_raises_spectral_centroid() {
        for open in [false, true] {
            let c = |tone: f32| {
                let out = hit(open, SR, params(0.5, 0.5, tone, 1.0), 1.0, 12_000);
                centroid(&spectrum(&out, SR, 2_048))
            };
            let (dark, mid, bright) = (c(0.0), c(0.5), c(1.0));
            assert!(
                mid > dark * 1.08 && bright > mid * 1.08,
                "open={open}: {dark} {mid} {bright} Hz"
            );
        }
    }

    /// Fraction of the 5–14 kHz power that sits on the odd harmonics the
    /// metal cluster has at `ratio`.
    fn power_on_lines(spec: &[(f64, f64)], ratio: f64) -> f64 {
        let bin_hz = spec[1].0;
        let mut on_lines = 0.0;
        for &f in &METAL_FREQS_HZ {
            let f = f64::from(f) * ratio;
            let mut n = 1.0;
            while n * f < 14_000.0 {
                if n * f > 5_000.0 {
                    let bin = (n * f / bin_hz).round() as usize;
                    on_lines += spec[bin - 1..=bin + 1].iter().map(|(_, p)| p).sum::<f64>();
                }
                n += 2.0;
            }
        }
        on_lines / band_power(spec, 5_000.0, 14_000.0)
    }

    #[test]
    fn tune_shifts_the_metal_cluster() {
        // A long open hat at three settings: its spectral lines sit on the
        // odd harmonics of its own tune ratio, not on the other ratios'.
        for (tune, ratio) in [(0.0, 0.8), (0.5, 1.0), (1.0, 1.25)] {
            let mut v = OpenHat::new(SR);
            v.apply_params(&params(tune, 1.0, 0.5, 1.0));
            assert!((f64::from(v.tune_ratio()) - ratio).abs() < 1e-5);
            v.trigger(1.0);
            let out = render(&mut v, 32_768);
            let spec = spectrum(&out, SR, 32_768);
            let own = power_on_lines(&spec, ratio);
            for other in [0.8, 1.0, 1.25] {
                if other != ratio {
                    let theirs = power_on_lines(&spec, other);
                    assert!(
                        own > 0.6 && own > theirs * 3.0,
                        "tune {tune}: {own} on own lines, {theirs} on {other}'s"
                    );
                }
            }
        }
    }

    #[test]
    fn velocity_raises_level_and_brightness() {
        for open in [false, true] {
            let loud = hit(open, SR, VoiceParams::default(), 1.0, 12_000);
            let soft = hit(open, SR, VoiceParams::default(), 0.7, 12_000);
            let ghost = hit(open, SR, VoiceParams::default(), 0.3, 12_000);
            let (pl, ps, pg) = (peak(&loud), peak(&soft), peak(&ghost));
            assert!(
                pl > ps * 1.25 && ps > pg * 1.8,
                "open={open}: {pl} {ps} {pg}"
            );
            let (cl, cs, cg) = (
                centroid(&spectrum(&loud, SR, 2_048)),
                centroid(&spectrum(&soft, SR, 2_048)),
                centroid(&spectrum(&ghost, SR, 2_048)),
            );
            assert!(
                cl > cs * 1.02 && cs > cg * 1.03,
                "open={open}: centroids {cl} {cs} {cg}"
            );
        }
    }

    #[test]
    fn level_scales_output_immediately() {
        for open in [false, true] {
            let full = hit(open, SR, VoiceParams::default(), 1.0, 2_400);
            let half = hit(open, SR, params(0.5, 0.5, 0.5, 0.5), 1.0, 2_400);
            for (a, b) in full.iter().zip(&half) {
                assert!((a * 0.5 - b).abs() <= a.abs() * 1e-6);
            }
            // Mid-ring, without a retrigger.
            let mut v = hat(open, SR);
            v.trigger(1.0);
            render(v.as_mut(), 240);
            v.apply_params(&params(0.5, 0.5, 0.5, 0.0));
            assert_eq!(v.process(), 0.0);
        }
    }

    #[test]
    fn snappy_is_ignored() {
        for open in [false, true] {
            let a = hit(open, SR, VoiceParams::default(), 1.0, 4_800);
            let b = hit(open, SR, params(0.5, 0.5, 0.5, 1.0), 1.0, 4_800);
            let snappy = VoiceParams {
                snappy: 1.0,
                ..VoiceParams::default()
            };
            let c = hit(open, SR, snappy, 1.0, 4_800);
            assert_eq!(a, b);
            assert_eq!(a, c);
        }
    }

    #[test]
    fn low_end_stays_30_db_under_the_main_band() {
        for open in [false, true] {
            for (tune, tone) in [(0.0, 0.0), (0.5, 0.5), (1.0, 1.0), (0.0, 1.0)] {
                for velocity in [0.3, 1.0] {
                    let out = hit(open, SR, params(tune, 0.5, tone, 1.0), velocity, 24_000);
                    let spec = spectrum(&out, SR, 4_096);
                    let low = band_power(&spec, 0.0, 2_000.0);
                    let peak_octave = [2_000.0, 4_000.0, 8_000.0, 16_000.0]
                        .iter()
                        .map(|&lo| band_power(&spec, lo, lo * 2.0))
                        .fold(0.0, f64::max);
                    let margin = db(peak_octave) - db(low);
                    assert!(
                        margin > 30.0,
                        "open={open} tune={tune} tone={tone} v={velocity}: {margin} dB"
                    );
                    // No DC either.
                    let mean = out.iter().map(|&s| f64::from(s)).sum::<f64>() / out.len() as f64;
                    assert!(mean.abs() < 1e-5, "mean {mean}");
                }
            }
        }
    }

    #[test]
    fn renders_are_deterministic() {
        let run = |open: bool| {
            let mut v = hat(open, SR);
            let mut out = Vec::new();
            for (i, velocity) in [1.0, 0.7, 0.7, 1.0, 0.3].into_iter().enumerate() {
                let x = i as f32 * 0.25;
                v.apply_params(&params(x, 0.5, 1.0 - x, 1.0));
                v.trigger(velocity);
                out.extend(render(v.as_mut(), 3_000));
                if i == 3 {
                    v.choke();
                }
            }
            out.extend(render(v.as_mut(), 48_000));
            out
        };
        for open in [false, true] {
            let (a, b) = (run(open), run(open));
            assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
        }
    }

    /// The part of `x` below about 1 kHz (two 4-pole Butterworth low-passes).
    /// A click is broadband; a hat has next to nothing down there, so a
    /// click shows up here at once.
    fn low_band(x: &[f32]) -> Vec<f32> {
        let mut stages = [0, 1, 0, 1].map(|i| Svf::new(1_000.0, HIGHPASS_Q[i], SR));
        x.iter()
            .map(|&s| stages.iter_mut().fold(s, |acc, f| f.process(acc).low))
            .collect()
    }

    #[test]
    fn retrigger_mid_ring_has_no_spike() {
        for open in [false, true] {
            let fresh = hit(open, SR, params(0.5, 1.0, 0.5, 1.0), 1.0, 960);
            for ring_ms in [1, 5, 12, 30] {
                let mut v = hat(open, SR);
                v.apply_params(&params(0.5, 1.0, 0.5, 1.0));
                v.trigger(1.0);
                let mut out = render(v.as_mut(), ring_ms * 48);
                v.trigger(1.0);
                out.extend(render(v.as_mut(), 960));
                let around = &out[out.len() - 1_008..];
                let low = peak(&low_band(&out)[out.len() - 1_008..]);
                assert!(
                    peak(around) < peak(&fresh) * 1.5 && low < peak(&fresh) * 1e-3,
                    "open={open} ring {ring_ms} ms: peak {} vs {}, low band {low}",
                    peak(around),
                    peak(&fresh),
                );
            }
        }
    }

    #[test]
    fn closed_hat_chokes_the_open_hat_through_the_kit() {
        use crate::{slot, Kit};

        // Let the open hat ring for 100 ms, then hit the closed hat.
        let ring = 4_800;
        let mut kit = Kit::new(SR);
        kit.trigger(slot::OPEN_HAT, 1.0);
        let before: Vec<f32> = (0..ring).map(|_| kit.process()).collect();
        let ringing = &before[ring - 480..];
        let level = peak(ringing);
        assert!(level > 0.03, "open hat should ring: {level}");
        kit.trigger(slot::CLOSED_HAT, 1.0);

        // Listen to the open hat alone: a ~5 ms fade, neither a cut nor a
        // click.
        let mut open = kit.open_hat.clone();
        let fade = render(&mut open, 960);
        assert!(peak(&fade[..24]) > level * 0.3, "faded too abruptly");
        assert!(max_step(&fade) <= max_step(ringing) * 1.1, "click on choke");
        let mut joined = before.clone();
        joined.extend_from_slice(&fade);
        let low = peak(&low_band(&joined)[ring - 48..]);
        assert!(low < level * 1e-3, "click on choke: low band {low}");
        let after_6ms = peak(&fade[288..]);
        assert!(after_6ms < level * 1e-3, "still {after_6ms} after 6 ms");
        assert!(!open.is_active(), "open hat still active after 20 ms");
        assert!(fade[600..].iter().all(|&s| s == 0.0));

        // The kit's mix is then exactly a lone closed hat (times the kit's
        // fixed headroom trim).
        let mix: Vec<f32> = (0..9_600).map(|_| kit.process()).collect();
        let mut lone = ClosedHat::new(SR);
        lone.trigger(1.0);
        let lone = render(&mut lone, 9_600);
        assert!(mix[960..]
            .iter()
            .zip(&lone[960..])
            .all(|(a, b)| a.to_bits() == (b * crate::KIT_HEADROOM).to_bits()));

        // An open-hat hit after the choke rings out with its full decay.
        kit.trigger(slot::OPEN_HAT, 1.0);
        let reopened: Vec<f32> = (0..14_400).map(|_| kit.process()).collect();
        let fresh = hit(true, SR, VoiceParams::default(), 1.0, 14_400);
        let late = |x: &[f32]| rms_db(&x[9_600..]);
        let trim_db = 20.0 * f64::from(crate::KIT_HEADROOM).log10();
        assert!(
            (late(&reopened) - (late(&fresh) + trim_db)).abs() < 2.0,
            "{} vs {} dB",
            late(&reopened),
            late(&fresh)
        );
    }

    #[test]
    fn fast_enough_for_real_time() {
        // Ten seconds of busy hats at 48 kHz, both voices sounding.
        let mut closed = ClosedHat::new(SR);
        let mut open = OpenHat::new(SR);
        let start = std::time::Instant::now();
        let mut acc = 0.0f32;
        for step in 0..80 {
            if step % 4 == 2 {
                open.trigger(1.0);
            } else {
                open.choke();
                closed.trigger(0.7);
            }
            for _ in 0..6_000 {
                acc += closed.process() + open.process();
            }
        }
        let elapsed = start.elapsed();
        assert!(acc.is_finite());
        assert!(elapsed.as_millis() < 100, "10 s of hats took {elapsed:?}");
    }
}
