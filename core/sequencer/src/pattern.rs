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

/// The voices a pattern can address. Session 1 has only the kick; the enum
/// grows with the voice set. The discriminant is the track index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum VoiceId {
    /// Bass drum.
    Kick = 0,
}

impl VoiceId {
    /// Every voice, in track order.
    pub const ALL: [VoiceId; 1] = [VoiceId::Kick];
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

    /// Short lowercase name used in pattern files.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            VoiceId::Kick => "kick",
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
}

impl Step {
    /// Silent step.
    pub const OFF: Step = Step {
        on: false,
        accent: false,
    };
    /// Plain hit.
    pub const ON: Step = Step {
        on: true,
        accent: false,
    };
    /// Accented hit.
    pub const ACCENT: Step = Step {
        on: true,
        accent: true,
    };
}

/// Sixteen steps for one voice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Track {
    /// The steps.
    pub steps: [Step; STEP_COUNT],
}

/// Error from parsing step notation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternParseError {
    /// The notation string was not exactly [`STEP_COUNT`] characters.
    WrongLength(usize),
    /// A character other than `x`, `X`, `-` or `.`.
    BadChar(char),
}

impl fmt::Display for PatternParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongLength(n) => {
                write!(f, "expected {STEP_COUNT} step characters, got {n}")
            }
            Self::BadChar(c) => write!(f, "unexpected character {c:?} (use x, X, - or .)"),
        }
    }
}

impl std::error::Error for PatternParseError {}

impl Track {
    /// Parses step notation: one character per step, `-` or `.` for off,
    /// `x` for a hit, `X` for an accented hit. Spaces are ignored so steps
    /// can be grouped (`"X--- x--- X--- x---"`).
    pub fn parse(notation: &str) -> Result<Self, PatternParseError> {
        let mut steps = [Step::OFF; STEP_COUNT];
        let mut n = 0;
        for c in notation.chars().filter(|c| !c.is_whitespace()) {
            let step = match c {
                '-' | '.' => Step::OFF,
                'x' => Step::ON,
                'X' => Step::ACCENT,
                other => return Err(PatternParseError::BadChar(other)),
            };
            if n < STEP_COUNT {
                steps[n] = step;
            }
            n += 1;
        }
        if n != STEP_COUNT {
            return Err(PatternParseError::WrongLength(n));
        }
        Ok(Self { steps })
    }

    /// The notation [`Track::parse`] accepts, grouped in fours.
    #[must_use]
    pub fn notation(&self) -> String {
        let mut s = String::with_capacity(STEP_COUNT + 3);
        for (i, step) in self.steps.iter().enumerate() {
            if i > 0 && i % 4 == 0 {
                s.push(' ');
            }
            s.push(match (step.on, step.accent) {
                (false, _) => '-',
                (true, false) => 'x',
                (true, true) => 'X',
            });
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
}

impl Default for Pattern {
    fn default() -> Self {
        Self::empty()
    }
}

impl Pattern {
    /// All steps off, no shuffle, accent amount at half.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            tracks: [Track {
                steps: [Step::OFF; STEP_COUNT],
            }; VoiceId::COUNT],
            shuffle: 0.0,
            accent: 0.5,
        }
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
