//! JSON pattern files.
//!
//! ```json
//! {
//!   "bpm": 120,
//!   "shuffle": 0.0,
//!   "accent": 0.5,
//!   "flam": 0.5,
//!   "voices": {
//!     "kick":       { "steps": "X--- x--- X--- x---", "tune": 0.5, "decay": 0.5, "level": 1.0 },
//!     "snare":      { "steps": "---- X--- ---- X---", "snappy": 0.6 },
//!     "closed_hat": { "steps": "x-x- x-x- x-x- x-x-", "mute": false }
//!   },
//!   "render": { "bars": 2, "sample_rate": 48000, "tail_seconds": 0.5 }
//! }
//! ```
//!
//! Voices: `kick`, `snare`, `low_tom`, `mid_tom`, `high_tom`, `rim`, `clap`,
//! `closed_hat`, `open_hat`, `cowbell`. Absent voices are silent.
//!
//! Step notation: one character per step, `-`/`.` off, `x` hit, `X` accented
//! hit, `f` flammed hit, `F` accented flammed hit; spaces are ignored.
//! Every field except `steps` has a default.

use serde::{Deserialize, Serialize};

use dsp::{KickParams, VoiceParams};
use sequencer::{Pattern, PatternParseError, Track, VoiceId};

use crate::Engine;

/// A complete pattern file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternSpec {
    /// Tempo for the internal clock.
    #[serde(default = "default_bpm")]
    pub bpm: f64,
    /// Shuffle amount `0..=1`.
    #[serde(default)]
    pub shuffle: f32,
    /// Accent amount `0..=1`.
    #[serde(default = "default_half")]
    pub accent: f32,
    /// Flam spacing `0..=1`.
    #[serde(default = "default_half")]
    pub flam: f32,
    /// Per-voice steps and controls.
    #[serde(default)]
    pub voices: VoicesSpec,
    /// Offline render settings (output gain and limiter also apply live).
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
    /// Snare drum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snare: Option<VoiceSpec>,
    /// Low tom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub low_tom: Option<VoiceSpec>,
    /// Mid tom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mid_tom: Option<VoiceSpec>,
    /// High tom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_tom: Option<VoiceSpec>,
    /// Rimshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rim: Option<VoiceSpec>,
    /// Hand clap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clap: Option<VoiceSpec>,
    /// Closed hi-hat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_hat: Option<VoiceSpec>,
    /// Open hi-hat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_hat: Option<VoiceSpec>,
    /// Cowbell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cowbell: Option<VoiceSpec>,
}

impl VoicesSpec {
    /// The entry for a voice.
    #[must_use]
    pub fn get(&self, voice: VoiceId) -> Option<&VoiceSpec> {
        match voice {
            VoiceId::Kick => self.kick.as_ref(),
            VoiceId::Snare => self.snare.as_ref(),
            VoiceId::LowTom => self.low_tom.as_ref(),
            VoiceId::MidTom => self.mid_tom.as_ref(),
            VoiceId::HighTom => self.high_tom.as_ref(),
            VoiceId::Rim => self.rim.as_ref(),
            VoiceId::Clap => self.clap.as_ref(),
            VoiceId::ClosedHat => self.closed_hat.as_ref(),
            VoiceId::OpenHat => self.open_hat.as_ref(),
            VoiceId::Cowbell => self.cowbell.as_ref(),
        }
    }

    /// Mutable entry for a voice.
    pub fn get_mut(&mut self, voice: VoiceId) -> &mut Option<VoiceSpec> {
        match voice {
            VoiceId::Kick => &mut self.kick,
            VoiceId::Snare => &mut self.snare,
            VoiceId::LowTom => &mut self.low_tom,
            VoiceId::MidTom => &mut self.mid_tom,
            VoiceId::HighTom => &mut self.high_tom,
            VoiceId::Rim => &mut self.rim,
            VoiceId::Clap => &mut self.clap,
            VoiceId::ClosedHat => &mut self.closed_hat,
            VoiceId::OpenHat => &mut self.open_hat,
            VoiceId::Cowbell => &mut self.cowbell,
        }
    }
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
    #[serde(default = "default_half")]
    pub tone: f32,
    /// `0..=1` (snare).
    #[serde(default = "default_half")]
    pub snappy: f32,
    /// `0..=1`.
    #[serde(default = "default_one")]
    pub level: f32,
    /// Muted tracks keep their steps but play nothing.
    #[serde(default, skip_serializing_if = "is_false")]
    pub mute: bool,
}

impl VoiceSpec {
    /// A voice with these steps and default controls.
    #[must_use]
    pub fn new(steps: &str) -> Self {
        Self {
            steps: steps.to_string(),
            tune: 0.5,
            decay: 0.5,
            tone: 0.5,
            snappy: 0.5,
            level: 1.0,
            mute: false,
        }
    }

    /// The DSP controls.
    #[must_use]
    pub fn params(&self) -> VoiceParams {
        VoiceParams {
            tune: self.tune,
            decay: self.decay,
            tone: self.tone,
            snappy: self.snappy,
            level: self.level,
        }
    }
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
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
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
        pattern.flam = self.flam;
        for voice in VoiceId::ALL {
            if let Some(v) = self.voices.get(voice) {
                let mut track =
                    Track::parse(&v.steps).map_err(|e| SpecError::Steps(voice.name(), e))?;
                track.mute = v.mute;
                *pattern.track_mut(voice) = track;
            }
        }
        Ok(pattern)
    }

    /// Controls for a voice (defaults when the voice is absent).
    #[must_use]
    pub fn voice_params(&self, voice: VoiceId) -> VoiceParams {
        self.voices
            .get(voice)
            .map_or_else(VoiceParams::default, VoiceSpec::params)
    }

    /// Kick controls (defaults when the voice is absent).
    #[must_use]
    pub fn kick_params(&self) -> KickParams {
        let p = self.voice_params(VoiceId::Kick);
        KickParams {
            tune: p.tune,
            decay: p.decay,
            level: p.level,
        }
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
        let mut engine = Engine::new(self.render.sample_rate as f32);
        engine.load_spec(self)?;
        // Play exactly `bars` bars; the tail is the last hits ringing out.
        engine.set_stop_after(Some(
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
        assert_eq!(spec.voice_params(VoiceId::Snare), VoiceParams::default());
        // 2 bars at 120 BPM = 4 s = 192 000 frames, plus 0.5 s tail.
        assert_eq!(spec.render_frames(), 216_000);
    }

    #[test]
    fn rejects_unknown_fields_and_bad_steps() {
        assert!(PatternSpec::from_json(r#"{ "bpm": 120, "swing": 1 }"#).is_err());
        assert!(PatternSpec::from_json(
            r#"{ "voices": { "cymbal": { "steps": "----------------" } } }"#
        )
        .is_err());
        let bad = r#"{ "voices": { "snare": { "steps": "x---" } } }"#;
        assert!(matches!(
            PatternSpec::from_json(bad),
            Err(SpecError::Steps("snare", _))
        ));
    }

    #[test]
    fn every_voice_parses_with_mute_and_flam() {
        let mut spec = PatternSpec::from_json(MINIMAL).unwrap();
        for v in VoiceId::ALL {
            let mut vs = VoiceSpec::new("f--- ---- X--- ----");
            vs.mute = v == VoiceId::Clap;
            *spec.voices.get_mut(v) = Some(vs);
        }
        let again = PatternSpec::from_json(&spec.to_json()).unwrap();
        assert_eq!(spec, again);
        let pattern = again.pattern().unwrap();
        assert!(pattern.track(VoiceId::Clap).mute);
        assert!(pattern.track(VoiceId::Cowbell).steps[0].flam);
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
