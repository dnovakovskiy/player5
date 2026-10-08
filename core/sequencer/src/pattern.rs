use core::fmt;

/// Steps per pattern (one bar of sixteenths).
pub const STEP_COUNT: usize = 16;

/// Beats per step.
pub const BEATS_PER_STEP: f64 = 0.25;

/// Maximum shuffle delay of an off-beat sixteenth, in beats: a third of a
/// sixteenth, which turns straight sixteenths into a triplet feel.
pub const MAX_SHUFFLE_BEATS: f64 = BEATS_PER_STEP / 3.0;

/// Velocity of a step without accent. Accented steps rise from here to
/// `1.0` as the pattern's accent amount goes from 0 to 1.
pub const UNACCENTED_VELOCITY: f32 = 0.7;

/// Flam spacing at `flam = 0` and `flam = 1`, in seconds. The grace note
/// lands this long *before* the grid; the main hit stays exactly on it.
pub const FLAM_SPACING_RANGE_S: (f64, f64) = (0.008, 0.040);

/// Grace-note velocity as a fraction of the main hit's velocity.
pub const FLAM_GRACE_RATIO: f32 = 0.6;

/// The voices a pattern can address. The discriminant is the track index
/// and matches `dsp::slot`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum VoiceId {
    /// Bass drum.
    Kick = 0,
    /// Snare drum.
    Snare = 1,
    /// Low tom.
    LowTom = 2,
    /// Mid tom.
    MidTom = 3,
    /// High tom.
    HighTom = 4,
    /// Rimshot.
    Rim = 5,
    /// Hand clap.
    Clap = 6,
    /// Closed hi-hat (chokes the open hat).
    ClosedHat = 7,
    /// Open hi-hat.
    OpenHat = 8,
    /// Cowbell.
    Cowbell = 9,
}

impl VoiceId {
    /// Every voice, in track order.
    pub const ALL: [VoiceId; 10] = [
        VoiceId::Kick,
        VoiceId::Snare,
        VoiceId::LowTom,
        VoiceId::MidTom,
        VoiceId::HighTom,
        VoiceId::Rim,
        VoiceId::Clap,
        VoiceId::ClosedHat,
        VoiceId::OpenHat,
        VoiceId::Cowbell,
    ];
    /// Number of voices.
    pub const COUNT: usize = Self::ALL.len();

    /// Track index of this voice.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Voice for a track index, if it exists.
    #[must_use]
    pub fn from_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    /// Lowercase name used in pattern files.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            VoiceId::Kick => "kick",
            VoiceId::Snare => "snare",
            VoiceId::LowTom => "low_tom",
            VoiceId::MidTom => "mid_tom",
            VoiceId::HighTom => "high_tom",
            VoiceId::Rim => "rim",
            VoiceId::Clap => "clap",
            VoiceId::ClosedHat => "closed_hat",
            VoiceId::OpenHat => "open_hat",
            VoiceId::Cowbell => "cowbell",
        }
    }

    /// Voice for a pattern-file name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.name() == name)
    }

    /// Two-letter panel label (BD, SD, …).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            VoiceId::Kick => "BD",
            VoiceId::Snare => "SD",
            VoiceId::LowTom => "LT",
            VoiceId::MidTom => "MT",
            VoiceId::HighTom => "HT",
            VoiceId::Rim => "RS",
            VoiceId::Clap => "CP",
            VoiceId::ClosedHat => "CH",
            VoiceId::OpenHat => "OH",
            VoiceId::Cowbell => "CB",
        }
    }
}

/// One step of one track.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Step {
    /// The voice fires on this step.
    pub on: bool,
    /// The step is accented.
    pub accent: bool,
    /// A quieter grace hit precedes the main hit (see
    /// [`FLAM_SPACING_RANGE_S`]).
    pub flam: bool,
}

impl Step {
    /// Silent step.
    pub const OFF: Step = Step {
        on: false,
        accent: false,
        flam: false,
    };
    /// Plain hit.
    pub const ON: Step = Step {
        on: true,
        accent: false,
        flam: false,
    };
    /// Accented hit.
    pub const ACCENT: Step = Step {
        on: true,
        accent: true,
        flam: false,
    };
    /// Plain hit with flam.
    pub const FLAM: Step = Step {
        on: true,
        accent: false,
        flam: true,
    };
    /// Accented hit with flam.
    pub const ACCENT_FLAM: Step = Step {
        on: true,
        accent: true,
        flam: true,
    };

    /// The notation character for this step.
    #[must_use]
    pub const fn symbol(self) -> char {
        match (self.on, self.accent, self.flam) {
            (false, _, _) => '-',
            (true, false, false) => 'x',
            (true, true, false) => 'X',
            (true, false, true) => 'f',
            (true, true, true) => 'F',
        }
    }

    /// Parses one notation character.
    #[must_use]
    pub const fn from_symbol(c: char) -> Option<Self> {
        match c {
            '-' | '.' => Some(Step::OFF),
            'x' => Some(Step::ON),
            'X' => Some(Step::ACCENT),
            'f' => Some(Step::FLAM),
            'F' => Some(Step::ACCENT_FLAM),
            _ => None,
        }
    }
}

/// Sixteen steps for one voice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Track {
    /// The steps.
    pub steps: [Step; STEP_COUNT],
    /// A muted track schedules nothing but keeps its steps.
    pub mute: bool,
}

/// Error from parsing step notation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternParseError {
    /// The notation string was not exactly [`STEP_COUNT`] characters.
    WrongLength(usize),
    /// A character other than `-`, `.`, `x`, `X`, `f` or `F`.
    BadChar(char),
}

impl fmt::Display for PatternParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongLength(n) => {
                write!(f, "expected {STEP_COUNT} step characters, got {n}")
            }
            Self::BadChar(c) => {
                write!(f, "unexpected character {c:?} (use -, x, X, f or F)")
            }
        }
    }
}

impl std::error::Error for PatternParseError {}

impl Track {
    /// Parses step notation: one character per step, `-` or `.` for off,
    /// `x` hit, `X` accented hit, `f` flammed hit, `F` accented flammed hit.
    /// Spaces are ignored so steps can be grouped (`"X--- x--- X--- x---"`).
    pub fn parse(notation: &str) -> Result<Self, PatternParseError> {
        let mut steps = [Step::OFF; STEP_COUNT];
        let mut n = 0;
        for c in notation.chars().filter(|c| !c.is_whitespace()) {
            let step = Step::from_symbol(c).ok_or(PatternParseError::BadChar(c))?;
            if n < STEP_COUNT {
                steps[n] = step;
            }
            n += 1;
        }
        if n != STEP_COUNT {
            return Err(PatternParseError::WrongLength(n));
        }
        Ok(Self { steps, mute: false })
    }

    /// The notation [`Track::parse`] accepts, grouped in fours.
    #[must_use]
    pub fn notation(&self) -> String {
        let mut s = String::with_capacity(STEP_COUNT + 3);
        for (i, step) in self.steps.iter().enumerate() {
            if i > 0 && i % 4 == 0 {
                s.push(' ');
            }
            s.push(step.symbol());
        }
        s
    }
}

/// One bar of sixteenths for every voice, plus the pattern-wide feel
/// controls. Tempo is not part of the pattern; it belongs to the clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pattern {
    /// One track per [`VoiceId`], indexed by [`VoiceId::index`].
    pub tracks: [Track; VoiceId::COUNT],
    /// `0..=1`. Delays every off-beat sixteenth by up to a third of a step.
    pub shuffle: f32,
    /// `0..=1`. How much louder accented steps are than plain ones.
    pub accent: f32,
    /// `0..=1`. Flam spacing, mapped onto [`FLAM_SPACING_RANGE_S`].
    pub flam: f32,
}

impl Default for Pattern {
    fn default() -> Self {
        Self::empty()
    }
}

impl Pattern {
    /// All steps off, no shuffle, accent and flam amounts at half.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            tracks: [Track {
                steps: [Step::OFF; STEP_COUNT],
                mute: false,
            }; VoiceId::COUNT],
            shuffle: 0.0,
            accent: 0.5,
            flam: 0.5,
        }
    }

    /// Flam spacing in seconds for this pattern's `flam` amount.
    #[must_use]
    pub fn flam_seconds(&self) -> f64 {
        let (lo, hi) = FLAM_SPACING_RANGE_S;
        lo + (hi - lo) * f64::from(self.flam.clamp(0.0, 1.0))
    }

    /// Track for a voice.
    #[must_use]
    pub fn track(&self, voice: VoiceId) -> &Track {
        &self.tracks[voice.index()]
    }

    /// Mutable track for a voice.
    pub fn track_mut(&mut self, voice: VoiceId) -> &mut Track {
        &mut self.tracks[voice.index()]
    }

    /// Position of an absolute step index (counting from the start of
    /// playback, wrapping through the pattern) in beats from beat 0,
    /// including the shuffle delay for off-beat sixteenths.
    #[must_use]
    pub fn step_beat(&self, absolute_step: u64) -> f64 {
        let base = absolute_step as f64 * BEATS_PER_STEP;
        if absolute_step % 2 == 1 {
            base + f64::from(self.shuffle.clamp(0.0, 1.0)) * MAX_SHUFFLE_BEATS
        } else {
            base
        }
    }

    /// Velocity a step plays at under this pattern's accent amount.
    #[must_use]
    pub fn velocity(&self, step: Step) -> f32 {
        if step.accent {
            UNACCENTED_VELOCITY + (1.0 - UNACCENTED_VELOCITY) * self.accent.clamp(0.0, 1.0)
        } else {
            UNACCENTED_VELOCITY
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_notation() {
        let t = Track::parse("X--- x--- X-x- x--x").unwrap();
        assert_eq!(t.steps[0], Step::ACCENT);
        assert_eq!(t.steps[1], Step::OFF);
        assert_eq!(t.steps[4], Step::ON);
        assert_eq!(t.steps[10], Step::ON);
        assert_eq!(t.steps[15], Step::ON);
        assert_eq!(t.notation(), "X--- x--- X-x- x--x");
        assert_eq!(Track::parse("................").unwrap(), Track::default());
    }

    #[test]
    fn parses_flams() {
        let t = Track::parse("f--- F--- x--- X---").unwrap();
        assert_eq!(t.steps[0], Step::FLAM);
        assert_eq!(t.steps[4], Step::ACCENT_FLAM);
        assert_eq!(t.notation(), "f--- F--- x--- X---");
    }

    #[test]
    fn voice_names_round_trip() {
        for v in VoiceId::ALL {
            assert_eq!(VoiceId::from_name(v.name()), Some(v));
            assert_eq!(VoiceId::from_index(v.index()), Some(v));
            assert_eq!(v.label().len(), 2);
        }
        assert_eq!(VoiceId::from_name("cymbal"), None);
    }

    #[test]
    fn flam_spacing_maps_range() {
        let mut p = Pattern::empty();
        p.flam = 0.0;
        assert_eq!(p.flam_seconds(), 0.008);
        p.flam = 1.0;
        assert_eq!(p.flam_seconds(), 0.040);
    }

    #[test]
    fn rejects_bad_notation() {
        assert_eq!(Track::parse("x---"), Err(PatternParseError::WrongLength(4)));
        assert_eq!(
            Track::parse("x---x---x---x---x"),
            Err(PatternParseError::WrongLength(17))
        );
        assert_eq!(
            Track::parse("x---x---x---x--?"),
            Err(PatternParseError::BadChar('?'))
        );
    }

    #[test]
    fn straight_steps_fall_on_sixteenths() {
        let p = Pattern::empty();
        for k in 0..32 {
            assert_eq!(p.step_beat(k), k as f64 * 0.25);
        }
    }

    #[test]
    fn shuffle_delays_only_off_beats() {
        let mut p = Pattern::empty();
        p.shuffle = 1.0;
        assert_eq!(p.step_beat(0), 0.0);
        assert!((p.step_beat(1) - (0.25 + 1.0 / 12.0)).abs() < 1e-12);
        assert_eq!(p.step_beat(2), 0.5);
        p.shuffle = 0.5;
        assert!((p.step_beat(3) - (0.75 + 1.0 / 24.0)).abs() < 1e-12);
        // Shuffled steps never overtake the next straight step.
        p.shuffle = 1.0;
        assert!(p.step_beat(1) < p.step_beat(2));
    }

    #[test]
    fn accent_amount_scales_accented_steps_only() {
        let mut p = Pattern::empty();
        p.accent = 0.0;
        assert_eq!(p.velocity(Step::ON), UNACCENTED_VELOCITY);
        assert_eq!(p.velocity(Step::ACCENT), UNACCENTED_VELOCITY);
        p.accent = 1.0;
        assert_eq!(p.velocity(Step::ON), UNACCENTED_VELOCITY);
        assert_eq!(p.velocity(Step::ACCENT), 1.0);
        p.accent = 0.5;
        assert!((p.velocity(Step::ACCENT) - 0.85).abs() < 1e-6);
    }
}
