//! Bit-exact hashing and a tolerant fingerprint of rendered audio.
//!
//! The hash catches *any* change. The fingerprint says *what kind* of change:
//! level, spectrum or timing. Golden tests store both.

use serde::{Deserialize, Serialize};

use crate::to_pcm16;

/// FNV-1a (64-bit) over little-endian 16-bit PCM. Stable, dependency-free,
/// and plenty for change detection.
#[must_use]
pub fn hash_pcm16(samples: &[f32]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for s in to_pcm16(samples) {
        for b in s.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(PRIME);
        }
    }
    h
}

/// Band edges (Hz) for the spectral part of the fingerprint. The top band
/// runs to Nyquist.
pub const BAND_EDGES_HZ: [f32; 8] = [0.0, 60.0, 120.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0];

const FFT_SIZE: usize = 2_048;
const HOP: usize = 1_024;
const ONSET_WINDOW_MS: f32 = 5.0;
const ONSET_REFRACTORY_MS: f32 = 30.0;
/// An onset needs a jump of this many dB between adjacent windows.
const ONSET_JUMP_DB: f32 = 12.0;
const ONSET_FLOOR_DBFS: f32 = -40.0;

/// A coarse, human-readable description of a render.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fingerprint {
    /// Sample rate the audio was rendered at.
    pub sample_rate: u32,
    /// Length in samples.
    pub frames: usize,
    /// Peak level.
    pub peak_dbfs: f32,
    /// RMS level over the whole file.
    pub rms_dbfs: f32,
    /// Mean power per band (see [`BAND_EDGES_HZ`]), in dB.
    pub bands_db: Vec<f32>,
    /// Detected onsets, as sample indices rounded to the analysis window.
    pub onsets: Vec<u64>,
}

/// Tolerances used by [`Fingerprint::matches`].
#[derive(Clone, Copy, Debug)]
pub struct Tolerance {
    /// Allowed peak/RMS deviation.
    pub level_db: f32,
    /// Allowed per-band deviation.
    pub band_db: f32,
}

impl Default for Tolerance {
    fn default() -> Self {
        Self {
            level_db: 0.25,
            band_db: 0.75,
        }
    }
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

impl Fingerprint {
    /// Analyses `samples`.
    #[must_use]
    pub fn of(samples: &[f32], sample_rate: u32) -> Self {
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let rms = if samples.is_empty() {
            0.0
        } else {
            (samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / samples.len() as f64)
                .sqrt() as f32
        };
        Self {
            sample_rate,
            frames: samples.len(),
            peak_dbfs: round2(db(peak)),
            rms_dbfs: round2(db(rms)),
            bands_db: band_energies(samples, sample_rate)
                .into_iter()
                .map(round2)
                .collect(),
            onsets: onsets(samples, sample_rate),
        }
    }

    /// Compares against a stored fingerprint. Returns a list of human-readable
    /// differences; empty means it matches within tolerance.
    #[must_use]
    pub fn differences(&self, other: &Self, tol: Tolerance) -> Vec<String> {
        let mut out = Vec::new();
        if self.sample_rate != other.sample_rate {
            out.push(format!(
                "sample rate {} vs {}",
                self.sample_rate, other.sample_rate
            ));
        }
        if self.frames != other.frames {
            out.push(format!("length {} vs {} frames", self.frames, other.frames));
        }
        if (self.peak_dbfs - other.peak_dbfs).abs() > tol.level_db {
            out.push(format!(
                "peak {:.2} vs {:.2} dBFS",
                self.peak_dbfs, other.peak_dbfs
            ));
        }
        if (self.rms_dbfs - other.rms_dbfs).abs() > tol.level_db {
            out.push(format!(
                "rms {:.2} vs {:.2} dBFS",
                self.rms_dbfs, other.rms_dbfs
            ));
        }
        if self.bands_db.len() != other.bands_db.len() {
            out.push("band count differs".to_string());
        } else {
            for (i, (a, b)) in self.bands_db.iter().zip(&other.bands_db).enumerate() {
                if (a - b).abs() > tol.band_db {
                    let hi = BAND_EDGES_HZ
                        .get(i + 1)
                        .map_or("nyquist".to_string(), |h| format!("{h}"));
                    out.push(format!(
                        "band {}-{} Hz: {a:.2} vs {b:.2} dB",
                        BAND_EDGES_HZ[i], hi
                    ));
                }
            }
        }
        if self.onsets != other.onsets {
            out.push(format!("onsets {:?} vs {:?}", self.onsets, other.onsets));
        }
        out
    }

    /// Whether the fingerprints agree within `tol`.
    #[must_use]
    pub fn matches(&self, other: &Self, tol: Tolerance) -> bool {
        self.differences(other, tol).is_empty()
    }
}

fn round2(x: f32) -> f32 {
    (x * 100.0).round() / 100.0
}

/// Mean power per band across Hann-windowed frames, in dB.
fn band_energies(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    let mut acc = vec![0.0f64; BAND_EDGES_HZ.len()];
    let mut frames = 0usize;
    let bin_hz = f64::from(sample_rate) / FFT_SIZE as f64;
    let mut re = vec![0.0f64; FFT_SIZE];
    let mut im = vec![0.0f64; FFT_SIZE];
    let window: Vec<f64> = (0..FFT_SIZE)
        .map(|i| 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / FFT_SIZE as f64).cos())
        .collect();
    let mut start = 0;
    while start < samples.len() {
        for i in 0..FFT_SIZE {
            re[i] = samples.get(start + i).copied().map_or(0.0, f64::from) * window[i];
            im[i] = 0.0;
        }
        fft_in_place(&mut re, &mut im);
        for bin in 0..FFT_SIZE / 2 {
            let hz = bin as f64 * bin_hz;
            let band = BAND_EDGES_HZ
                .iter()
                .rposition(|&edge| hz >= f64::from(edge))
                .unwrap_or(0);
            acc[band] += re[bin] * re[bin] + im[bin] * im[bin];
        }
        frames += 1;
        start += HOP;
    }
    let norm = (frames.max(1) as f64) * (FFT_SIZE as f64).powi(2);
    acc.into_iter()
        .map(|p| (10.0 * (p / norm).max(1e-18).log10()) as f32)
        .collect()
}

/// Radix-2 decimation-in-time FFT. Analysis only; allocation and `std` math
/// are fine here.
fn fft_in_place(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two());
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
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
        }
        len <<= 1;
    }
}

/// Onsets: a jump of `ONSET_JUMP_DB` in short-window RMS above a floor,
/// with a refractory period. Reported at the window start.
fn onsets(samples: &[f32], sample_rate: u32) -> Vec<u64> {
    let win = ((ONSET_WINDOW_MS / 1_000.0) * sample_rate as f32)
        .round()
        .max(1.0) as usize;
    let refractory = ((ONSET_REFRACTORY_MS / 1_000.0) * sample_rate as f32).round() as usize;
    let rms: Vec<f32> = samples
        .chunks(win)
        .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let mut out = Vec::new();
    let mut last: Option<usize> = None;
    for i in 0..rms.len() {
        let prev = if i == 0 { 0.0 } else { rms[i - 1] };
        let cur_db = db(rms[i]);
        let jump = cur_db - db(prev);
        let start = i * win;
        let clear = last.map_or(true, |l| start >= l + refractory);
        if cur_db > ONSET_FLOOR_DBFS && jump > ONSET_JUMP_DB && clear {
            out.push(start as u64);
            last = Some(start);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_and_sensitive() {
        let a = vec![0.0, 0.5, -0.5, 0.25];
        let mut b = a.clone();
        assert_eq!(hash_pcm16(&a), hash_pcm16(&a));
        b[3] += 1.0 / 32_767.0;
        assert_ne!(hash_pcm16(&a), hash_pcm16(&b));
        assert_eq!(hash_pcm16(&[]), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn fft_of_impulse_is_flat() {
        let mut re = vec![0.0; 8];
        let mut im = vec![0.0; 8];
        re[0] = 1.0;
        fft_in_place(&mut re, &mut im);
        assert!(re.iter().all(|&r| (r - 1.0).abs() < 1e-12));
        assert!(im.iter().all(|&i| i.abs() < 1e-12));
    }

    #[test]
    fn sine_lands_in_its_band() {
        let sr = 48_000;
        let samples: Vec<f32> = (0..sr)
            .map(|i| 0.5 * (std::f32::consts::TAU * 1_500.0 * i as f32 / sr as f32).sin())
            .collect();
        let fp = Fingerprint::of(&samples, sr as u32);
        let loudest = fp
            .bands_db
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        assert_eq!(BAND_EDGES_HZ[loudest], 1_000.0);
        assert!((fp.peak_dbfs + 6.02).abs() < 0.1);
        assert!((fp.rms_dbfs + 9.03).abs() < 0.1);
    }

    #[test]
    fn detects_onsets() {
        let sr = 48_000u32;
        let mut samples = vec![0.0f32; sr as usize];
        for &at in &[1_000usize, 24_000, 30_000] {
            for i in 0..2_000 {
                samples[at + i] = 0.5 * (-(i as f32) / 400.0).exp();
            }
        }
        let fp = Fingerprint::of(&samples, sr);
        assert_eq!(fp.onsets, vec![960, 24_000, 30_000]);
        assert!(fp.matches(&fp, Tolerance::default()));
        let mut other = fp.clone();
        other.peak_dbfs -= 1.0;
        assert!(!fp.matches(&other, Tolerance::default()));
    }
}
