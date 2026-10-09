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
//! 96 kHz) and gently soft-clipped, which rounds off the tallest noise peaks
//! and keeps the hit-to-hit peak level steady.
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
//! Retriggering while the clap still sounds (flams, rolls) never clicks: the
//! noise filters keep running, the burst schedule restarts, and both
//! amplitude envelopes are slewed (about 0.1 ms for the bursts, 1 ms for the
//! tail), so the waveform never jumps. A new tail charges up from whatever
//! the old one has left, like an analogue envelope capacitor.

use crate::blocks::{Noise, OnePole, Svf};
use crate::math;
use crate::voice::Voice;
use crate::VoiceParams;

/// Number of bursts per hit (the last one opens the tail).
const BURSTS: usize = 4;
/// Burst onsets after the trigger (seconds) and their levels relative to
/// the hit. Slightly uneven spacing keeps the flutter from sounding like a
/// buzz.
const BURST_SCHEDULE: [(f32, f32); BURSTS] = [
    (0.0, 0.9),
    (0.010, 1.0),
    (0.021, 0.92),
    (0.031, 0.85),
];
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
/// clip barely touches typical samples but rounds off the tall peaks,
/// which steadies the peak level from hit to hit.
const NOISE_DRIVE_RMS: f32 = 0.5;
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
const CALIBRATION: f32 = 0.3;
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
        let tail_noise = self
            .tail_lp
            .lowpass(math::soft_clip(self.tail_bp.process(white).band * self.tail_scale));

        // Slewed amplitude envelopes (the amplifiers' response).
        self.burst_amp += self.burst_slew * (self.burst_env - self.burst_amp);
        self.tail_amp += self.tail_slew * (self.tail_env - self.tail_amp);
        let mix = burst_noise * self.burst_amp + tail_noise * self.tail_amp;
        let out = self.hp.process(mix).high;

        self.level_now += self.level_coef * (self.params.level - self.level_now);
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
