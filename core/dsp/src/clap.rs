//! TR-inspired hand clap.
//!
//! The classic analogue clap is a single white-noise source feeding two
//! envelope-controlled paths. A fast sawtooth generator re-triggers one
//! envelope three or four times in quick succession (the "flutter" of
//! several hands not quite together), and a second, longer envelope opens
//! after the last burst to give the diffuse, room-like tail. This model
//! keeps that structure:
//!
//! * **bursts** – white noise through a band-pass (moderate Q, centre
//!   0.85–1.9 kHz set by `tone`), shaped by four sharp-attack exponential
//!   bursts (a sawtooth through an exponential amplifier) about 10 ms apart,
//!   each falling some 25 dB before the next one starts;
//! * **tail** – the same noise through a lower, wider band-pass and a gentle
//!   low-pass, so it sounds a little darker and more diffuse than the
//!   bursts. It blooms with the last burst and decays exponentially; `decay`
//!   sets its length (80–400 ms to −60 dB);
//! * **output** – both paths summed and high-passed at 160 Hz to keep
//!   rumble and envelope thump out of the low end.
//!
//! Each filtered noise path is scaled to a fixed RMS (so `tone` changes the
//! colour, not the level, and the clap sounds the same at 44.1, 48 and
//! 96 kHz) and soft-clipped, which rounds off the tallest noise peaks and
//! keeps the hit-to-hit peak level steady (within about ±1 dB).
//!
//! Controls (all `0..=1`; `tune` and `snappy` are ignored):
//!
//! * `tone` – band-pass centre for bursts and tail, 0.85–1.9 kHz
//!   (≈ 1.27 kHz at the centre of travel);
//! * `decay` – tail length, 0.08–0.4 s to −60 dB;
//! * `level` – output level. It applies immediately, smoothed over a few
//!   milliseconds so moving it mid-hit never clicks.
//!
//! `tone` and `decay` take effect on the next hit. A hit with velocity 0 is
//! ignored.
//!
//! Accent (velocity): louder (gain follows velocity) and slightly brighter
//! (the band-pass centre rises by about 7 % from an unaccented 0.7 to a full
//! 1.0). A full-velocity hit at default controls and `level = 1.0` peaks
//! close to −10 dBFS, leaving room for the kick in a full kit.
//!
//! Retriggering while the clap still sounds (flams, rolls) does not click:
//! the noise filters keep running, the burst schedule restarts, and both
//! amplitude envelopes are slewed (about 0.1 ms for the bursts, 1 ms for the
//! tail), so the waveform never steps. A new tail charges up from whatever
//! the old one has left, like an analogue envelope capacitor. A flammed
//! clap therefore plays the grace note's first bursts and then the main
//! hit's full flutter: a wider, more ensemble-like clap.

use crate::blocks::{Noise, OnePole, Svf};
use crate::math;
use crate::voice::Voice;
use crate::VoiceParams;

/// Number of bursts per hit (the last one opens the tail).
const BURSTS: usize = 4;
/// Burst onsets after the trigger (seconds) and their levels relative to
/// the hit. Slightly uneven spacing keeps the flutter from sounding like a
/// buzz.
const BURST_SCHEDULE: [(f32, f32); BURSTS] =
    [(0.0, 0.9), (0.010, 1.0), (0.021, 0.92), (0.031, 0.85)];
/// Time constant of each burst's decay: about −25 dB by the next burst.
const BURST_TAU_S: f32 = 0.0035;
/// Attack slew of the burst envelope (sharp, but not a step).
const BURST_ATTACK_TAU_S: f32 = 0.000_12;

/// Tail level relative to the hit, reached shortly after the last burst.
const TAIL_LEVEL: f32 = 0.62;
/// Attack slew of the tail envelope: it blooms rather than snaps.
const TAIL_ATTACK_TAU_S: f32 = 0.0012;
/// Tail (−60 dB): 0.08 s at `decay = 0`, 0.4 s at `decay = 1`.
const TAIL_T60_LOW_S: f32 = 0.08;
/// ln(0.4 / 0.08).
const TAIL_T60_LN_RATIO: f32 = 1.609_437_912;

/// Band-pass centre: 850 Hz at `tone = 0`, 1.9 kHz at `tone = 1`, before
/// the velocity brightening below.
const CENTRE_LOW_HZ: f32 = 850.0;
/// ln(1900 / 850).
const CENTRE_LN_RATIO: f32 = 0.804_372_816;
/// Centre multiplier `BASE + VELOCITY · v`: 1.0 unaccented (0.7), 1.075 at
/// a full accent.
const BRIGHT_BASE: f32 = 0.825;
const BRIGHT_VELOCITY: f32 = 0.25;
/// Burst band-pass quality.
const BURST_Q: f32 = 1.4;
/// The tail band-pass sits lower and is wider than the burst band-pass.
const TAIL_CENTRE_RATIO: f32 = 0.78;
const TAIL_Q: f32 = 0.9;
/// Tail low-pass cutoff as a multiple of the burst centre.
const TAIL_LP_RATIO: f32 = 3.0;
/// Keeps every filter well below Nyquist at every sample rate.
const MAX_NYQUIST_FRACTION: f32 = 0.42;
/// Rumble high-pass on the summed output.
const HP_HZ: f32 = 160.0;
const HP_Q: f32 = 0.707;

/// The band-passed noise is scaled to this RMS before its soft clip. The
/// clip shaves about 1 dB off typical (1σ) samples and about 4 dB off the
/// tall (2σ) peaks, which steadies the peak level from hit to hit and adds
/// a little analogue grit.
const NOISE_DRIVE_RMS: f32 = 0.7;
/// Noise-equivalent bandwidth of a unity-peak two-pole band-pass, per hertz
/// of −3 dB bandwidth: `π / 2`.
const BANDPASS_ENBW: f32 = core::f32::consts::FRAC_PI_2;
/// Seed of the noise generator (reset with the sample rate).
const NOISE_SEED: u32 = 0x6C8E_9CF5;

/// Time constant of the `level` smoother.
const LEVEL_TAU_S: f32 = 0.005;
/// Sample rates are clamped to `1..=MAX_SAMPLE_RATE` so every derived
/// coefficient stays finite.
const MAX_SAMPLE_RATE: f32 = 1.0e6;

/// Output scaling so a full hit at default controls peaks near −10 dBFS.
const CALIBRATION: f32 = 0.35;
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

/// Clamps a control to `0..=1`, mapping non-finite values to 0.
#[inline]
fn unit(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Gain that brings uniform white noise in `[-1, 1)` (variance 1/3, spread
/// evenly up to Nyquist) through a unity-peak band-pass with −3 dB
/// bandwidth `bandwidth_hz` to [`NOISE_DRIVE_RMS`].
#[inline]
fn noise_scale(bandwidth_hz: f32, sample_rate: f32) -> f32 {
    let enbw = (BANDPASS_ENBW * bandwidth_hz).max(10.0);
    let rms = (enbw * 2.0 / (3.0 * sample_rate)).sqrt();
    NOISE_DRIVE_RMS / rms
}

/// The hand clap voice. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct Clap {
    sample_rate: f32,
    params: VoiceParams,

    // Derived per sample rate.
    burst_onsets: [(u32, f32); BURSTS],
    burst_coef: f32,
    burst_slew: f32,
    tail_slew: f32,
    level_coef: f32,
    hp: Svf,

    // Derived per trigger.
    velocity: f32,
    tail_coef: f32,
    burst_scale: f32,
    tail_scale: f32,
    burst_bp: Svf,
    tail_bp: Svf,
    tail_lp: OnePole,

    // State.
    active: bool,
    noise: Noise,
    elapsed: u32,
    next_burst: usize,
    burst_env: f32,
    burst_amp: f32,
    tail_env: f32,
    tail_amp: f32,
    level_now: f32,
}

impl Clap {
    /// Creates an idle voice for the given sample rate.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        let mut clap = Self {
            sample_rate,
            params: VoiceParams::default(),
            burst_onsets: [(0, 0.0); BURSTS],
            burst_coef: 0.0,
            burst_slew: 0.0,
            tail_slew: 0.0,
            level_coef: 0.0,
            hp: Svf::default(),
            velocity: 0.0,
            tail_coef: 0.0,
            burst_scale: 0.0,
            tail_scale: 0.0,
            burst_bp: Svf::default(),
            tail_bp: Svf::default(),
            tail_lp: OnePole::default(),
            active: false,
            noise: Noise::new(NOISE_SEED),
            elapsed: 0,
            next_burst: BURSTS,
            burst_env: 0.0,
            burst_amp: 0.0,
            tail_env: 0.0,
            tail_amp: 0.0,
            level_now: 0.0,
        };
        clap.set_sample_rate(sample_rate);
        clap
    }

    /// Current controls.
    #[must_use]
    pub fn params(&self) -> VoiceParams {
        self.params
    }

    /// Burst band-pass centre (Hz) the current `tone` resolves to for an
    /// unaccented (velocity 0.7) hit.
    #[must_use]
    pub fn centre_frequency_hz(&self) -> f32 {
        math::exp_range(self.params.tone, CENTRE_LOW_HZ, CENTRE_LN_RATIO)
    }

    /// Tail length (seconds to −60 dB) the current `decay` resolves to.
    #[must_use]
    pub fn tail_decay_seconds(&self) -> f32 {
        math::exp_range(self.params.decay, TAIL_T60_LOW_S, TAIL_T60_LN_RATIO)
    }

    fn go_idle(&mut self) {
        self.active = false;
        self.elapsed = 0;
        self.next_burst = BURSTS;
        self.burst_env = 0.0;
        self.burst_amp = 0.0;
        self.tail_env = 0.0;
        self.tail_amp = 0.0;
        self.burst_bp.reset();
        self.tail_bp.reset();
        self.tail_lp.reset();
        self.hp.reset();
    }
}

impl Voice for Clap {
    fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = if sample_rate.is_finite() {
            sample_rate.clamp(1.0, MAX_SAMPLE_RATE)
        } else {
            48_000.0
        };
        let sr = self.sample_rate;
        for (slot, &(onset_s, level)) in self.burst_onsets.iter_mut().zip(&BURST_SCHEDULE) {
            // Round to the nearest sample; the cast saturates, never panics.
            *slot = ((onset_s * sr + 0.5) as u32, level);
        }
        self.burst_coef = math::tau_coefficient(BURST_TAU_S, sr);
        self.burst_slew = 1.0 - math::tau_coefficient(BURST_ATTACK_TAU_S, sr);
        self.tail_slew = 1.0 - math::tau_coefficient(TAIL_ATTACK_TAU_S, sr);
        self.level_coef = 1.0 - math::tau_coefficient(LEVEL_TAU_S, sr);
        self.hp.set(HP_HZ, HP_Q, sr);
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
        let max_hz = MAX_NYQUIST_FRACTION * sr;

        if !self.active {
            self.level_now = self.params.level;
        }

        // Filters: coefficients change, state carries on (the noise never
        // stops while the voice is active), so a retrigger cannot click.
        let bright = BRIGHT_BASE + BRIGHT_VELOCITY * velocity;
        let centre = (self.centre_frequency_hz() * bright).min(max_hz);
        let tail_centre = centre * TAIL_CENTRE_RATIO;
        self.burst_bp.set(centre, BURST_Q, sr);
        self.tail_bp.set(tail_centre, TAIL_Q, sr);
        self.tail_lp
            .set_cutoff((centre * TAIL_LP_RATIO).min(max_hz), sr);
        self.burst_scale = self.burst_bp.k() * noise_scale(centre / BURST_Q, sr);
        self.tail_scale = self.tail_bp.k() * noise_scale(tail_centre / TAIL_Q, sr);

        // Envelopes: restart the burst schedule; whatever is still sounding
        // decays under the new hit.
        self.tail_coef = math::decay_coefficient(self.tail_decay_seconds(), sr);
        self.velocity = velocity;
        self.elapsed = 0;
        self.next_burst = 0;
        self.active = true;
    }

    #[inline]
    fn process(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }

        // Burst schedule: each onset re-arms the burst envelope; the last
        // one also opens the tail.
        if let Some(&(onset, level)) = self.burst_onsets.get(self.next_burst) {
            if self.elapsed >= onset {
                self.burst_env = level * self.velocity;
                self.next_burst += 1;
                if self.next_burst == BURSTS {
                    self.tail_env = self.tail_env.max(TAIL_LEVEL * self.velocity);
                }
            }
            self.elapsed = self.elapsed.saturating_add(1);
        }

        // One noise source, two band-passes, each normalised and rounded.
        let white = self.noise.tick();
        let burst_noise = math::soft_clip(self.burst_bp.process(white).band * self.burst_scale);
        let tail_noise = self.tail_lp.lowpass(math::soft_clip(
            self.tail_bp.process(white).band * self.tail_scale,
        ));

        // Slewed amplitude envelopes (the amplifiers' response).
        self.burst_amp += self.burst_slew * (self.burst_env - self.burst_amp);
        self.tail_amp += self.tail_slew * (self.tail_env - self.tail_amp);
        let mix = burst_noise * self.burst_amp + tail_noise * self.tail_amp;
        let out = self.hp.process(mix).high;

        // Level smoother; snaps onto the target once within -120 dB of it,
        // so a fade towards zero never goes denormal.
        let level_gap = self.params.level - self.level_now;
        self.level_now = if level_gap.abs() < FLUSH_THRESHOLD {
            self.params.level
        } else {
            self.level_now + self.level_coef * level_gap
        };
        let y = out * self.level_now * CALIBRATION;

        // Advance envelopes.
        self.burst_env = flush(self.burst_env * self.burst_coef);
        self.tail_env = flush(self.tail_env * self.tail_coef);
        self.burst_amp = flush(self.burst_amp);
        self.tail_amp = flush(self.tail_amp);
        if self.next_burst >= BURSTS
            && self.burst_env < IDLE_THRESHOLD
            && self.burst_amp < IDLE_THRESHOLD
            && self.tail_env < IDLE_THRESHOLD
            && self.tail_amp < IDLE_THRESHOLD
        {
            self.go_idle();
        }

        // Never reached at any setting (peaks stay near −10 dBFS); a hard
        // guarantee of the output contract.
        y.clamp(-1.0, 1.0)
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

    fn clap_with(sample_rate: f32, tone: f32, decay: f32, level: f32) -> Clap {
        let mut clap = Clap::new(sample_rate);
        clap.apply_params(&VoiceParams {
            tone,
            decay,
            level,
            ..VoiceParams::default()
        });
        clap
    }

    fn render(clap: &mut Clap, n: usize) -> Vec<f32> {
        (0..n).map(|_| clap.process()).collect()
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

    /// Power spectrum `(hz, power)` of `samples` from a Hann-windowed DFT
    /// evaluated every 25 Hz up to 12 kHz (test-only, so `std` math is fine).
    fn spectrum(samples: &[f32], sample_rate: f32) -> Vec<(f64, f64)> {
        let n = samples.len();
        let windowed: Vec<f64> = samples
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                let x = i as f64 / n as f64;
                f64::from(s) * (0.5 - 0.5 * (std::f64::consts::TAU * x).cos())
            })
            .collect();
        (1..=480)
            .map(|bin| {
                let f = f64::from(bin) * 25.0;
                let step = std::f64::consts::TAU * f / f64::from(sample_rate);
                let (mut re, mut im) = (0.0f64, 0.0f64);
                for (i, &v) in windowed.iter().enumerate() {
                    let ph = step * i as f64;
                    re += v * ph.cos();
                    im -= v * ph.sin();
                }
                (f, re * re + im * im)
            })
            .collect()
    }

    /// Power-weighted mean frequency (Hz) up to 12 kHz.
    fn centroid_hz(samples: &[f32], sample_rate: f32) -> f64 {
        let spec = spectrum(samples, sample_rate);
        let num: f64 = spec.iter().map(|&(f, p)| f * p).sum();
        let den: f64 = spec.iter().map(|&(_, p)| p).sum();
        num / den
    }

    /// RMS per 1 ms frame.
    fn frame_rms(samples: &[f32], sample_rate: f32) -> Vec<f32> {
        let frame = (sample_rate / 1_000.0) as usize;
        samples
            .chunks(frame)
            .map(|c| (energy(c) / c.len() as f64).sqrt() as f32)
            .collect()
    }

    /// Largest sample-to-sample step.
    fn max_step(samples: &[f32]) -> f32 {
        samples
            .windows(2)
            .fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()))
    }

    /// Frame indices of the envelope maxima that stand out: each one is
    /// followed by a fall of at least 6 dB before the envelope rises again.
    fn distinct_maxima(env: &[f32]) -> Vec<usize> {
        let mut found = Vec::new();
        let (mut high, mut high_at, mut low) = (0.0f32, 0usize, f32::MAX);
        let mut rising = true;
        for (i, &e) in env.iter().enumerate() {
            if rising {
                if e > high {
                    high = e;
                    high_at = i;
                }
                if e < high * 0.5 {
                    found.push(high_at);
                    rising = false;
                    low = e;
                }
            } else {
                low = low.min(e);
                if e > low * 2.0 {
                    rising = true;
                    high = e;
                    high_at = i;
                }
            }
        }
        found
    }

    fn full_hit(clap: &mut Clap, velocity: f32, n: usize) -> Vec<f32> {
        clap.trigger(velocity);
        render(clap, n)
    }

    #[test]
    fn idle_voice_is_silent() {
        let mut clap = Clap::new(SR);
        assert!(!clap.is_active());
        assert!(render(&mut clap, 1_000).iter().all(|&s| s == 0.0));
    }

    #[test]
    fn zero_velocity_is_ignored() {
        let mut clap = Clap::new(SR);
        clap.trigger(0.0);
        assert!(!clap.is_active());
        assert_eq!(clap.process(), 0.0);
    }

    #[test]
    fn control_mappings() {
        let mut clap = Clap::new(SR);
        for (tone, hz) in [(0.0, 850.0), (0.5, 1_270.8), (1.0, 1_900.0)] {
            clap.apply_params(&VoiceParams {
                tone,
                ..VoiceParams::default()
            });
            let got = clap.centre_frequency_hz();
            assert!((got - hz).abs() < hz * 0.005, "tone {tone}: {got} Hz");
        }
        for (decay, s) in [(0.0, 0.08), (0.5, 0.178_9), (1.0, 0.4)] {
            clap.apply_params(&VoiceParams {
                decay,
                ..VoiceParams::default()
            });
            let got = clap.tail_decay_seconds();
            assert!((got - s).abs() < s * 0.005, "decay {decay}: {got} s");
        }
    }

    #[test]
    fn full_hit_peaks_near_minus_10_dbfs() {
        // Every hit of a run (the noise sequence carries across hits), at
        // every supported rate.
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            let mut clap = clap_with(sr, 0.5, 0.5, 1.0);
            for hit in 0..8 {
                let p = db(peak(&full_hit(&mut clap, 1.0, (sr * 0.6) as usize)));
                assert!((-11.5..=-8.5).contains(&p), "{sr} Hz, hit {hit}: {p} dBFS");
            }
        }
    }

    #[test]
    fn output_is_finite_and_bounded() {
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            // Every control at 0 and at 1, in every combination.
            for bits in 0..32u32 {
                let pick = |b: u32| if bits & (1 << b) != 0 { 1.0 } else { 0.0 };
                let params = VoiceParams {
                    tune: pick(0),
                    decay: pick(1),
                    tone: pick(2),
                    snappy: pick(3),
                    level: pick(4),
                };
                for velocity in [0.1, 0.7, 1.0] {
                    let mut clap = Clap::new(sr);
                    clap.apply_params(&params);
                    clap.trigger(velocity);
                    let mut n = 0usize;
                    while clap.is_active() {
                        let s = clap.process();
                        assert!(s.is_finite() && s.abs() <= 1.0, "{params:?} v {velocity}");
                        n += 1;
                        assert!(n < sr as usize, "{params:?} v {velocity}: never idle");
                    }
                }
            }
        }
    }

    #[test]
    fn rolls_at_full_settings_keep_headroom() {
        // A 32nd-note roll of full accents at the loudest, longest settings
        // stays far below full scale: the output clamp is only a guarantee,
        // never part of the sound.
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            for tone in [0.0, 1.0] {
                let mut clap = clap_with(sr, tone, 1.0, 1.0);
                let gap = (sr * 0.0625) as usize;
                let mut out = Vec::new();
                for _ in 0..32 {
                    out.extend(full_hit(&mut clap, 1.0, gap));
                }
                let p = db(peak(&out));
                assert!(p < -6.0, "{sr} Hz tone {tone}: {p} dBFS");
            }
        }
    }

    #[test]
    fn decays_to_silence_and_goes_idle() {
        let mut idle_after = Vec::new();
        for decay in [0.0, 0.5, 1.0] {
            let mut clap = clap_with(SR, 0.5, decay, 1.0);
            let out = full_hit(&mut clap, 1.0, 48_000);
            assert!(!clap.is_active(), "decay {decay}");
            let last = out.iter().rposition(|&s| s != 0.0).unwrap();
            // Below -100 dBFS by the time it stops, and exactly 0 after.
            assert!(out[last].abs() < 1e-5, "decay {decay}: {}", out[last]);
            assert!(out[last + 1..].iter().all(|&s| s == 0.0));
            assert!(render(&mut clap, 1_000).iter().all(|&s| s == 0.0));
            idle_after.push(last);
        }
        assert!(idle_after[0] < idle_after[1] && idle_after[1] < idle_after[2]);
    }

    #[test]
    fn longer_decay_rings_longer() {
        let hit = |decay: f32| {
            let mut clap = clap_with(SR, 0.5, decay, 1.0);
            full_hit(&mut clap, 1.0, 24_000)
        };
        let (short, mid, long) = (hit(0.0), hit(0.5), hit(1.0));
        // Tail energy 100-300 ms after the hit.
        let tail = |out: &[f32]| energy(&out[4_800..14_400]);
        assert!(tail(&long) > tail(&mid) * 4.0);
        assert!(tail(&mid) > tail(&short) * 4.0);
        // The bursts before the tail opens do not depend on `decay`.
        assert_eq!(short[..1_400], long[..1_400]);
    }

    #[test]
    fn bursts_are_visible_in_the_first_40_ms() {
        for sr in crate::SUPPORTED_SAMPLE_RATES {
            for velocity in [0.7, 1.0] {
                let mut clap = clap_with(sr, 0.5, 0.5, 1.0);
                let out = full_hit(&mut clap, velocity, (sr * 0.04) as usize);
                let maxima = distinct_maxima(&frame_rms(&out, sr));
                assert!(maxima.len() >= 3, "{sr} Hz v {velocity}: {maxima:?}");
                // About 10 ms apart (1 ms frames).
                for pair in maxima.windows(2) {
                    let gap = pair[1] - pair[0];
                    assert!((8..=13).contains(&gap), "{sr} Hz: {maxima:?}");
                }
            }
        }
    }

    #[test]
    fn tone_raises_spectral_centroid() {
        let centroid = |tone: f32| {
            let mut clap = clap_with(SR, tone, 0.5, 1.0);
            let out = full_hit(&mut clap, 0.7, 4_800);
            centroid_hz(&out, SR)
        };
        let (dark, mid, bright) = (centroid(0.0), centroid(0.5), centroid(1.0));
        assert!(mid > dark * 1.15, "{dark} -> {mid}");
        assert!(bright > mid * 1.15, "{mid} -> {bright}");
    }

    #[test]
    fn tail_is_darker_than_bursts() {
        let mut clap = clap_with(SR, 0.5, 1.0, 1.0);
        let out = full_hit(&mut clap, 0.7, 12_000);
        let bursts = centroid_hz(&out[..1_440], SR);
        let tail = centroid_hz(&out[2_400..12_000], SR);
        assert!(bursts > tail * 1.15, "bursts {bursts} Hz, tail {tail} Hz");
    }

    #[test]
    fn accent_is_louder_and_brighter() {
        let hit = |velocity: f32| {
            let mut clap = clap_with(SR, 0.5, 0.5, 1.0);
            let out = full_hit(&mut clap, velocity, 9_600);
            (peak(&out), energy(&out), centroid_hz(&out[..4_800], SR))
        };
        let (soft_peak, soft_energy, _) = hit(0.1);
        let (peak_07, energy_07, centroid_07) = hit(0.7);
        let (peak_10, energy_10, centroid_10) = hit(1.0);
        assert!(peak_07 > soft_peak * 4.0);
        assert!(energy_07 > soft_energy * 16.0);
        assert!(peak_10 > peak_07 * 1.25, "{peak_07} -> {peak_10}");
        assert!(energy_10 > energy_07 * 1.6, "{energy_07} -> {energy_10}");
        assert!(
            centroid_10 > centroid_07 * 1.03,
            "{centroid_07} -> {centroid_10} Hz"
        );
    }

    #[test]
    fn tune_and_snappy_are_ignored() {
        let run = |tune: f32, snappy: f32| {
            let mut clap = Clap::new(SR);
            clap.apply_params(&VoiceParams {
                tune,
                snappy,
                ..VoiceParams::default()
            });
            full_hit(&mut clap, 1.0, 9_600)
        };
        assert_eq!(run(0.0, 0.0), run(1.0, 1.0));
    }

    #[test]
    fn level_scales_output_and_applies_smoothly() {
        let mut full = clap_with(SR, 0.5, 0.5, 1.0);
        let mut half = clap_with(SR, 0.5, 0.5, 0.5);
        let a = full_hit(&mut full, 1.0, 9_600);
        let b = full_hit(&mut half, 1.0, 9_600);
        assert!(a.iter().zip(&b).all(|(&x, &y)| (x * 0.5 - y).abs() < 1e-7));

        // Pulling the level down mid-hit fades within a few ms, without a
        // step.
        let mut clap = clap_with(SR, 0.5, 1.0, 1.0);
        let mut out = full_hit(&mut clap, 1.0, 2_400);
        clap.apply_params(&VoiceParams {
            decay: 1.0,
            level: 0.0,
            ..VoiceParams::default()
        });
        out.extend(render(&mut clap, 9_600));
        assert!(max_step(&out[2_350..2_450]) < max_step(&a));
        assert!(peak(&out[3_600..]) < peak(&out[..2_400]) * 0.01);
        // The fade lands exactly on zero (no denormal crawl) while the tail
        // is still running.
        assert!(clap.is_active());
        assert!(out[7_200..].iter().all(|&s| s == 0.0));
    }

    #[test]
    fn renders_are_deterministic() {
        let run = || {
            let mut clap = clap_with(SR, 0.3, 0.7, 0.9);
            let mut out = Vec::new();
            for velocity in [1.0, 0.7, 0.42] {
                out.extend(full_hit(&mut clap, velocity, 7_000));
            }
            out
        };
        let (a, b) = (run(), run());
        assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));

        // A sample-rate change resets the voice to the same starting point.
        let mut clap = clap_with(SR, 0.3, 0.7, 0.9);
        full_hit(&mut clap, 1.0, 3_000);
        clap.set_sample_rate(SR);
        let mut fresh = clap_with(SR, 0.3, 0.7, 0.9);
        assert_eq!(
            full_hit(&mut clap, 1.0, 7_000),
            full_hit(&mut fresh, 1.0, 7_000)
        );
    }

    #[test]
    fn retrigger_mid_ring_does_not_click() {
        // Reference: the steepest step anywhere in a clean full hit.
        let mut clean = clap_with(SR, 0.5, 0.5, 1.0);
        let reference = max_step(&full_hit(&mut clean, 1.0, 24_000));
        // (retrigger after n samples, first velocity, second velocity):
        // mid-burst, a flam's grace note into an accent, in the tail, and an
        // accent cut short by a soft hit.
        for (at, first, second) in [
            (240, 1.0, 1.0),
            (1_150, 0.6, 1.0),
            (2_900, 1.0, 0.7),
            (6_000, 1.0, 1.0),
            (1_700, 1.0, 0.1),
        ] {
            let mut clap = clap_with(SR, 0.5, 0.5, 1.0);
            let mut out = full_hit(&mut clap, first, at);
            out.extend(full_hit(&mut clap, second, 480));
            let around = max_step(&out[at - 48..at + 48]);
            assert!(
                around <= reference * 1.1,
                "retrigger at {at}: step {around} vs {reference}"
            );
        }
    }

    #[test]
    fn no_dc_or_rumble() {
        let mut clap = clap_with(SR, 0.0, 1.0, 1.0);
        let out = full_hit(&mut clap, 1.0, 9_600);
        let n = out.len() as f64;
        let mean = out.iter().map(|&s| f64::from(s)).sum::<f64>() / n;
        let rms = (energy(&out) / n).sqrt();
        assert!(mean.abs() < rms * 0.01, "mean {mean}, rms {rms}");
        let spec = spectrum(&out, SR);
        let total: f64 = spec.iter().map(|&(_, p)| p).sum();
        let low: f64 = spec
            .iter()
            .filter(|&&(f, _)| f <= 100.0)
            .map(|&(_, p)| p)
            .sum();
        assert!(
            low < total * 1e-3,
            "{:.2e} of the power below 100 Hz",
            low / total
        );
    }

    #[test]
    fn renders_ten_seconds_quickly() {
        let mut clap = clap_with(SR, 0.5, 1.0, 1.0);
        let start = std::time::Instant::now();
        let mut acc = 0.0f32;
        for i in 0..480_000 {
            if i % 6_000 == 0 {
                clap.trigger(1.0);
            }
            acc += std::hint::black_box(clap.process()).abs();
        }
        let elapsed = start.elapsed();
        assert!(acc.is_finite() && acc > 0.0);
        assert!(elapsed.as_millis() < 100, "10 s took {elapsed:?}");
    }
}
