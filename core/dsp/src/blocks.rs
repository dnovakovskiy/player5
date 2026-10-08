//! Small, deterministic building blocks shared by the voices: noise,
//! band-limited square oscillators, filters and envelopes.
//!
//! Everything here is real-time safe and uses only [`crate::math`] for
//! transcendental functions, so voices built from these blocks render
//! bit-identically on every platform (ADR-0002). Coefficient setters that
//! call `math::exp`/`tan_turns` are cheap enough to call on trigger, but
//! avoid calling them per sample.

use crate::math;

/// White noise from a 32-bit xorshift generator. Deterministic for a given
/// seed; state carries across hits so consecutive hits differ slightly, as
/// on analogue hardware, while renders stay reproducible.
#[derive(Clone, Debug)]
pub struct Noise {
    state: u32,
}

impl Noise {
    /// A generator with a non-zero seed (zero is replaced by a constant).
    #[must_use]
    pub const fn new(seed: u32) -> Self {
        Self {
            state: if seed == 0 { 0x9E37_79B9 } else { seed },
        }
    }

    /// Next sample, uniform in `[-1, 1)`.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        // Top 24 bits → [0, 1) exactly representable, then to [-1, 1).
        ((x >> 8) as f32) * (1.0 / 8_388_608.0) - 1.0
    }
}

/// PolyBLEP correction for a unit step at phase 0 with increment `dt`.
#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

/// Square oscillator with PolyBLEP anti-aliasing. Output in roughly
/// `[-1, 1]`, 50 % duty cycle.
#[derive(Clone, Debug, Default)]
pub struct Square {
    phase: f32,
    inc: f32,
}

impl Square {
    /// Oscillator at `freq_hz`, starting at `phase` (turns, `0..1`).
    #[must_use]
    pub fn new(freq_hz: f32, sample_rate: f32, phase: f32) -> Self {
        let mut s = Self { phase, inc: 0.0 };
        s.set_frequency(freq_hz, sample_rate);
        s
    }

    /// Changes frequency without resetting phase.
    pub fn set_frequency(&mut self, freq_hz: f32, sample_rate: f32) {
        self.inc = (freq_hz / sample_rate).clamp(0.0, 0.49);
    }

    /// Resets the phase (turns).
    pub fn reset(&mut self, phase: f32) {
        self.phase = phase - phase.floor();
    }

    /// Next sample.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        let dt = self.inc;
        let t = self.phase;
        let naive = if t < 0.5 { 1.0 } else { -1.0 };
        let mut t2 = t + 0.5;
        if t2 >= 1.0 {
            t2 -= 1.0;
        }
        let out = naive + poly_blep(t, dt) - poly_blep(t2, dt);
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        out
    }
}

/// Sine oscillator driven by a phase accumulator.
#[derive(Clone, Debug, Default)]
pub struct Sine {
    phase: f32,
    inc: f32,
}

impl Sine {
    /// Oscillator at `freq_hz`.
    #[must_use]
    pub fn new(freq_hz: f32, sample_rate: f32) -> Self {
        let mut s = Self::default();
        s.set_frequency(freq_hz, sample_rate);
        s
    }

    /// Changes frequency without resetting phase.
    pub fn set_frequency(&mut self, freq_hz: f32, sample_rate: f32) {
        self.inc = (freq_hz / sample_rate).clamp(0.0, 0.49);
    }

    /// Resets the phase (turns).
    pub fn reset(&mut self, phase: f32) {
        self.phase = phase - phase.floor();
    }

    /// Next sample.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        let out = math::sin_turns(self.phase);
        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        out
    }
}

/// One-pole low-pass (and the complementary high-pass).
#[derive(Clone, Debug, Default)]
pub struct OnePole {
    coef: f32,
    state: f32,
}

impl OnePole {
    /// Filter with cutoff `cutoff_hz`.
    #[must_use]
    pub fn new(cutoff_hz: f32, sample_rate: f32) -> Self {
        let mut f = Self::default();
        f.set_cutoff(cutoff_hz, sample_rate);
        f
    }

    /// Changes the cutoff.
    pub fn set_cutoff(&mut self, cutoff_hz: f32, sample_rate: f32) {
        self.coef = math::onepole_coefficient(cutoff_hz.max(1.0), sample_rate);
    }

    /// Clears the state.
    pub fn reset(&mut self) {
        self.state = 0.0;
    }

    /// Low-pass output.
    #[inline]
    pub fn lowpass(&mut self, x: f32) -> f32 {
        self.state += (1.0 - self.coef) * (x - self.state);
        self.state
    }

    /// High-pass output (input minus low-pass).
    #[inline]
    pub fn highpass(&mut self, x: f32) -> f32 {
        x - self.lowpass(x)
    }
}

/// All three outputs of one [`Svf`] step.
#[derive(Clone, Copy, Debug, Default)]
pub struct SvfOut {
    /// Low-pass.
    pub low: f32,
    /// Band-pass (unity peak gain at resonance when scaled by `k`; raw here).
    pub band: f32,
    /// High-pass.
    pub high: f32,
}

/// Topology-preserving-transform state-variable filter (A. Simper). Stable
/// under fast modulation, cheap, with simultaneous LP/BP/HP outputs.
#[derive(Clone, Debug, Default)]
pub struct Svf {
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
    ic1: f32,
    ic2: f32,
}

impl Svf {
    /// Filter at `cutoff_hz` with quality factor `q` (0.5 = no resonance).
    #[must_use]
    pub fn new(cutoff_hz: f32, q: f32, sample_rate: f32) -> Self {
        let mut f = Self::default();
        f.set(cutoff_hz, q, sample_rate);
        f
    }

    /// Changes cutoff and Q. Keeps state, so it can be swept.
    pub fn set(&mut self, cutoff_hz: f32, q: f32, sample_rate: f32) {
        let x = (cutoff_hz / sample_rate).clamp(1e-5, 0.49);
        let g = math::tan_turns(x * 0.5);
        let k = 1.0 / q.max(0.05);
        self.k = k;
        self.a1 = 1.0 / (1.0 + g * (g + k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    /// Clears the state.
    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    /// Damping factor `1/Q`; multiply `band` by it for unity peak gain.
    #[must_use]
    pub fn k(&self) -> f32 {
        self.k
    }

    /// One step; returns all outputs.
    #[inline]
    pub fn process(&mut self, x: f32) -> SvfOut {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        SvfOut {
            low: v2,
            band: v1,
            high: x - self.k * v1 - v2,
        }
    }
}

/// Exponential decay envelope: jumps to a level on trigger and falls by a
/// constant ratio per sample.
#[derive(Clone, Debug, Default)]
pub struct Decay {
    value: f32,
    coef: f32,
}

impl Decay {
    /// Starts at `level`, falling 60 dB in `t60_seconds`.
    pub fn trigger(&mut self, level: f32, t60_seconds: f32, sample_rate: f32) {
        self.value = level;
        self.coef = math::decay_coefficient(t60_seconds, sample_rate);
    }

    /// Starts at `level` with time constant `tau_seconds` (falls to 1/e).
    pub fn trigger_tau(&mut self, level: f32, tau_seconds: f32, sample_rate: f32) {
        self.value = level;
        self.coef = math::tau_coefficient(tau_seconds, sample_rate);
    }

    /// Changes the fall rate mid-flight (e.g. a choke).
    pub fn set_t60(&mut self, t60_seconds: f32, sample_rate: f32) {
        self.coef = math::decay_coefficient(t60_seconds, sample_rate);
    }

    /// Current value without advancing.
    #[must_use]
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Silences immediately.
    pub fn reset(&mut self) {
        self.value = 0.0;
    }

    /// Returns the current value, then advances one sample.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        let v = self.value;
        self.value *= self.coef;
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_uniform_and_deterministic() {
        let mut a = Noise::new(1);
        let mut b = Noise::new(1);
        let mut sum = 0.0f64;
        let mut min = 1.0f32;
        let mut max = -1.0f32;
        for _ in 0..100_000 {
            let x = a.tick();
            assert_eq!(x, b.tick());
            sum += f64::from(x);
            min = min.min(x);
            max = max.max(x);
        }
        assert!((sum / 100_000.0).abs() < 0.01);
        assert!(min >= -1.0 && max < 1.0 && min < -0.99 && max > 0.99);
    }

    #[test]
    fn square_has_expected_period_and_bounded_output() {
        let sr = 48_000.0;
        let mut sq = Square::new(1_000.0, sr, 0.0);
        let out: Vec<f32> = (0..4_800).map(|_| sq.tick()).collect();
        let rising = out.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert!((99..=101).contains(&rising), "{rising}");
        assert!(out.iter().all(|s| s.abs() <= 1.01));
    }

    #[test]
    fn svf_bandpass_peaks_at_cutoff() {
        let sr = 48_000.0;
        let gain_at = |hz: f32| {
            let mut f = Svf::new(2_000.0, 4.0, sr);
            let mut osc = Sine::new(hz, sr);
            let mut peak = 0.0f32;
            for i in 0..9_600 {
                let y = f.process(osc.tick()).band * f.k();
                if i > 4_800 {
                    peak = peak.max(y.abs());
                }
            }
            peak
        };
        let at = gain_at(2_000.0);
        assert!((at - 1.0).abs() < 0.05, "{at}");
        assert!(gain_at(500.0) < 0.2);
        assert!(gain_at(8_000.0) < 0.2);
    }

    #[test]
    fn svf_lowpass_and_highpass_split() {
        let sr = 48_000.0;
        let run = |hz: f32| {
            let mut f = Svf::new(1_000.0, 0.707, sr);
            let mut osc = Sine::new(hz, sr);
            let (mut lo, mut hi) = (0.0f32, 0.0f32);
            for i in 0..9_600 {
                let o = f.process(osc.tick());
                if i > 4_800 {
                    lo = lo.max(o.low.abs());
                    hi = hi.max(o.high.abs());
                }
            }
            (lo, hi)
        };
        let (lo, hi) = run(100.0);
        assert!(lo > 0.95 && hi < 0.05, "{lo} {hi}");
        let (lo, hi) = run(10_000.0);
        assert!(lo < 0.05 && hi > 0.95, "{lo} {hi}");
    }

    #[test]
    fn decay_falls_60_db() {
        let sr = 48_000.0;
        let mut d = Decay::default();
        d.trigger(1.0, 0.1, sr);
        for _ in 0..4_800 {
            d.tick();
        }
        assert!((d.value() - 1e-3).abs() < 2e-5, "{}", d.value());
    }

    #[test]
    fn onepole_splits_spectrum() {
        let sr = 48_000.0;
        let mut f = OnePole::new(1_000.0, sr);
        let mut osc = Sine::new(50.0, sr);
        let mut peak = 0.0f32;
        for i in 0..9_600 {
            let y = f.highpass(osc.tick());
            if i > 4_800 {
                peak = peak.max(y.abs());
            }
        }
        assert!(peak < 0.1, "{peak}");
    }
}
