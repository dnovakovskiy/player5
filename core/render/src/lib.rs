//! Offline rendering support: WAV output, PCM hashing and a coarse audio
//! fingerprint. None of this runs on the render thread.

#![forbid(unsafe_code)]

pub mod analysis;

use std::path::Path;

/// Converts float samples to 16-bit PCM the way the WAV writer does.
#[must_use]
pub fn to_pcm16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32_767.0).round() as i16)
        .collect()
}

/// Writes mono 16-bit PCM.
pub fn write_wav(path: &Path, sample_rate: u32, samples: &[f32]) -> Result<(), hound::Error> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for s in to_pcm16(samples) {
        writer.write_sample(s)?;
    }
    writer.finalize()
}
