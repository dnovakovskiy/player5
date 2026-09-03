//! JSON pattern files.
//!
//! ```json
//! {
//!   "bpm": 120,
//!   "shuffle": 0.0,
//!   "accent": 0.5,
//!   "voices": {
//!     "kick": { "steps": "X--- x--- X--- x---", "tune": 0.5, "decay": 0.5, "level": 1.0 }
//!   },
//!   "render": { "bars": 2, "sample_rate": 48000, "tail_seconds": 0.5 }
//! }
//! ```
//!
//! Step notation: one character per step, `-`/`.` off, `x` hit, `X` accented
//! hit; spaces are ignored. Every field except `voices` has a default.

use serde::{Deserialize, Serialize};

use dsp::KickParams;
use sequencer::{Pattern, PatternParseError, Track, VoiceId, VoiceParam};

use crate::Engine;

/// A complete pattern file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternSpec {
    /// Tempo.
    #[serde(default = "default_bpm")]
    pub bpm: f64,
    /// Shuffle amount `0..=1`.
    #[serde(default)]
    pub shuffle: f32,
    /// Accent amount `0..=1`.
    #[serde(default = "default_accent")]
    pub accent: f32,
    /// Per-voice steps and controls.
    #[serde(default)]
    pub voices: VoicesSpec,
    /// Offline render settings.
    #[serde(default)]
    pub render: RenderSpec,
}

/// The voices section. Voices that are absent stay silent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoicesSpec {
    /// Bass drum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kick: Option<VoiceSpec>,
}

/// One voice's steps and TR-style controls.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceSpec {
    /// Step notation (see module docs).
    pub steps: String,
    /// `0..=1`.
    #[serde(default = "default_half")]
    pub tune: f32,
    /// `0..=1`.
    #[serde(default = "default_half")]
    pub decay: f32,
    /// `0..=1`.
    #[serde(default = "default_one")]
    pub level: f32,
}

/// Offline render settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderSpec {
    /// Pattern repetitions.
    #[serde(default = "default_bars")]
    pub bars: u32,
    /// Output sample rate.
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    /// Seconds appended after the last bar so the final hits ring out.
    #[serde(default = "default_tail")]
    pub tail_seconds: f32,
    /// Master output gain.
    #[serde(default = "default_one")]
    pub output_gain: f32,
    /// Soft safety limiter.
    #[serde(default)]
    pub limiter: bool,
    /// Block size used when stepping the engine; exercises block boundaries.
    #[serde(default = "default_block_size")]
    pub block_size: usize,
}

impl Default for RenderSpec {
    fn default() -> Self {
        Self {
            bars: default_bars(),
            sample_rate: default_sample_rate(),
            tail_seconds: default_tail(),
            output_gain: 1.0,
            limiter: false,
            block_size: default_block_size(),
        }
    }
}

fn default_bpm() -> f64 {
    120.0
}
fn default_accent() -> f32 {
    0.5
}
fn default_half() -> f32 {
    0.5
}
fn default_one() -> f32 {
    1.0
}
fn default_bars() -> u32 {
    2
}
fn default_sample_rate() -> u32 {
    48_000
}
fn default_tail() -> f32 {
    0.5
}
fn default_block_size() -> usize {
    256
}

/// Errors from loading a pattern file.
#[derive(Debug)]
pub enum SpecError {
    /// Malformed JSON or unknown fields.
    Json(serde_json::Error),
    /// Bad step notation for the named voice.
    Steps(&'static str, PatternParseError),
}

impl std::fmt::Display for SpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(e) => write!(f, "invalid pattern JSON: {e}"),
            Self::Steps(voice, e) => write!(f, "bad steps for {voice}: {e}"),
        }
    }
}

impl std::error::Error for SpecError {}

impl From<serde_json::Error> for SpecError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

impl PatternSpec {
    /// Parses a pattern file.
    pub fn from_json(json: &str) -> Result<Self, SpecError> {
        let spec: Self = serde_json::from_str(json)?;
        spec.pattern()?; // validate notation up front
        Ok(spec)
    }

    /// Serialises to pretty JSON.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("PatternSpec is always serialisable")
    }

    /// The sequencer pattern this file describes.
    pub fn pattern(&self) -> Result<Pattern, SpecError> {
        let mut pattern = Pattern::empty();
        pattern.shuffle = self.shuffle;
        pattern.accent = self.accent;
        if let Some(kick) = &self.voices.kick {
            *pattern.track_mut(VoiceId::Kick) =
                Track::parse(&kick.steps).map_err(|e| SpecError::Steps(VoiceId::Kick.name(), e))?;
        }
        Ok(pattern)
    }

    /// Kick controls (defaults when the voice is absent).
    #[must_use]
    pub fn kick_params(&self) -> KickParams {
        self.voices
            .kick
            .as_ref()
            .map_or_else(KickParams::default, |k| KickParams {
                tune: k.tune,
                decay: k.decay,
                level: k.level,
            })
    }

    /// Total frames an offline render of this file produces.
    #[must_use]
    pub fn render_frames(&self) -> usize {
        let sr = f64::from(self.render.sample_rate);
        let beats = f64::from(self.render.bars) * sequencer::STEP_COUNT as f64 * 0.25;
        let body = beats * sr * 60.0 / self.bpm;
        let tail = f64::from(self.render.tail_seconds) * sr;
        (body + tail).round() as usize
    }

    /// Builds an engine, applies this file and renders it offline.
    pub fn render(&self) -> Result<Vec<f32>, SpecError> {
        let pattern = self.pattern()?;
        let mut engine = Engine::new(self.render.sample_rate as f32);
        engine.set_tempo(self.bpm);
        engine.set_pattern(pattern);
        let kick = self.kick_params();
        engine.set_kick_param(VoiceParam::Tune, kick.tune);
        engine.set_kick_param(VoiceParam::Decay, kick.decay);
        engine.set_kick_param(VoiceParam::Level, kick.level);
        engine.set_output_gain(self.render.output_gain);
        engine.set_limiter(self.render.limiter);
        // Play exactly `bars` bars; the tail is the last hits ringing out.
        engine.control().set_stop_after(Some(
            u64::from(self.render.bars) * sequencer::STEP_COUNT as u64,
        ));
        engine.start();
        Ok(engine.render_frames(self.render_frames(), self.render.block_size))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"{ "voices": { "kick": { "steps": "x---x---x---x---" } } }"#;

    #[test]
    fn defaults_fill_in() {
        let spec = PatternSpec::from_json(MINIMAL).unwrap();
        assert_eq!(spec.bpm, 120.0);
        assert_eq!(spec.render.bars, 2);
        assert_eq!(spec.kick_params(), KickParams::default());
        // 2 bars at 120 BPM = 4 s = 192 000 frames, plus 0.5 s tail.
        assert_eq!(spec.render_frames(), 216_000);
    }

    #[test]
    fn rejects_unknown_fields_and_bad_steps() {
        assert!(PatternSpec::from_json(r#"{ "bpm": 120, "swing": 1 }"#).is_err());
        let bad = r#"{ "voices": { "kick": { "steps": "x---" } } }"#;
        assert!(matches!(
            PatternSpec::from_json(bad),
            Err(SpecError::Steps("kick", _))
        ));
    }

    #[test]
    fn round_trips_through_json() {
        let spec = PatternSpec::from_json(MINIMAL).unwrap();
        let again = PatternSpec::from_json(&spec.to_json()).unwrap();
        assert_eq!(spec, again);
    }

    #[test]
    fn renders_audio() {
        let spec = PatternSpec::from_json(MINIMAL).unwrap();
        let audio = spec.render().unwrap();
        assert_eq!(audio.len(), spec.render_frames());
        assert!(audio.iter().any(|&s| s != 0.0));
        assert!(audio.iter().all(|s| s.is_finite()));
    }
}
