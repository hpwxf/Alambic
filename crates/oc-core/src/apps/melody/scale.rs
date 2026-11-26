//! The scales `MELODY` quantises to and the chords it stacks in them
//! (journal/D-047).
//!
//! A note is a number of semitones above the root, any sign; a scale is the
//! set of pitch classes it keeps, twelve bits from the root. Quantising
//! keeps a step's note as stored, so changing the scale changes what it
//! plays, never what it holds: back to the first scale, the same melody.

/// The scales, in the order `SCALE` turns through them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    /// Every semitone.
    Chromatic,
    /// Ionian.
    Major,
    /// Aeolian, the natural minor.
    Minor,
    /// Minor with a major sixth.
    Dorian,
    /// Minor with a minor second.
    Phrygian,
    /// Major with an augmented fourth.
    Lydian,
    /// Major with a minor seventh.
    Mixolydian,
    /// Minor with a minor second and a diminished fifth.
    Locrian,
    /// Minor with a major seventh.
    HarmonicMinor,
    /// Five notes, no fourth or seventh.
    PentaMajor,
    /// Five notes, no second or sixth.
    PentaMinor,
    /// The minor pentatonic and its flat fifth.
    Blues,
    /// Six whole tones.
    Whole,
}

impl Scale {
    /// Every scale, in the order `SCALE` turns through them.
    pub const ALL: [Self; 13] = [
        Self::Chromatic,
        Self::Major,
        Self::Minor,
        Self::Dorian,
        Self::Phrygian,
        Self::Lydian,
        Self::Mixolydian,
        Self::Locrian,
        Self::HarmonicMinor,
        Self::PentaMajor,
        Self::PentaMinor,
        Self::Blues,
        Self::Whole,
    ];

    /// Zero-based index of this scale in [`Self::ALL`].
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The scale with the given index, the chromatic one past the last.
    #[must_use]
    pub const fn from_index(index: usize) -> Self {
        if index < Self::ALL.len() {
            Self::ALL[index]
        } else {
            Self::Chromatic
        }
    }

    /// The pitch classes it keeps, bit `n` for `n` semitones above the
    /// root; bit 0, the root, always set.
    #[must_use]
    pub const fn mask(self) -> u16 {
        match self {
            Self::Chromatic => 0b1111_1111_1111,
            Self::Major => 0b1010_1011_0101,
            Self::Minor => 0b0101_1010_1101,
            Self::Dorian => 0b0110_1010_1101,
            Self::Phrygian => 0b0101_1010_1011,
            Self::Lydian => 0b1010_1101_0101,
            Self::Mixolydian => 0b0110_1011_0101,
            Self::Locrian => 0b0101_0110_1011,
            Self::HarmonicMinor => 0b1001_1010_1101,
            Self::PentaMajor => 0b0010_1001_0101,
            Self::PentaMinor => 0b0100_1010_1001,
            Self::Blues => 0b0100_1110_1001,
            Self::Whole => 0b0101_0101_0101,
        }
    }

    /// Notes per octave.
    #[must_use]
    pub const fn notes_per_octave(self) -> i32 {
        // At most twelve.
        #[allow(clippy::cast_possible_wrap)]
        let count = self.mask().count_ones() as i32;
        count
    }

    /// Whether `note` (semitones above the root) is in the scale.
    #[must_use]
    pub const fn contains(self, note: i32) -> bool {
        let class = note.rem_euclid(12);
        self.mask() & (1 << class) != 0
    }

    /// The note of the scale nearest `note`, the lower one of two as near.
    #[must_use]
    pub const fn quantize(self, note: i32) -> i32 {
        let mut distance = 0;
        // The root is always in: six semitones away at most.
        while distance <= 6 {
            if self.contains(note - distance) {
                return note - distance;
            }
            if self.contains(note + distance) {
                return note + distance;
            }
            distance += 1;
        }
        note
    }

    /// The rank of `note` in the scale counted from the root, so many per
    /// octave, of the nearest note in the scale for one outside it.
    #[must_use]
    pub const fn degree_of(self, note: i32) -> i32 {
        let note = self.quantize(note);
        let octave = note.div_euclid(12);
        let class = note.rem_euclid(12);
        // The pitch classes of the scale below this one.
        let below = (self.mask() & ((1 << class) - 1)).count_ones();
        // At most eleven.
        #[allow(clippy::cast_possible_wrap)]
        let below = below as i32;
        octave * self.notes_per_octave() + below
    }

    /// The note of rank `degree`, the inverse of [`Self::degree_of`].
    #[must_use]
    pub const fn note_of(self, degree: i32) -> i32 {
        let len = self.notes_per_octave();
        let octave = degree.div_euclid(len);
        let mut rank = degree.rem_euclid(len);
        let mut class = 0;
        while class < 12 {
            if self.mask() & (1 << class) != 0 {
                if rank == 0 {
                    break;
                }
                rank -= 1;
            }
            class += 1;
        }
        octave * 12 + class
    }

    /// The note `by` degrees from `note` in the scale, `note` taken to its
    /// nearest first.
    #[must_use]
    pub const fn step(self, note: i32, by: i32) -> i32 {
        self.note_of(self.degree_of(note) + by)
    }
}

/// [`Scale::ALL`] as the screen writes them.
pub const SCALE_NAMES: [&str; Scale::ALL.len()] = [
    "CHROMATIC",
    "MAJOR",
    "MINOR",
    "DORIAN",
    "PHRYGIAN",
    "LYDIAN",
    "MIXOLYD",
    "LOCRIAN",
    "HARM MIN",
    "PENTA MAJ",
    "PENTA MIN",
    "BLUES",
    "WHOLE",
];

/// The roots, as the screen writes them.
pub const ROOT_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// One tone of a chord above the note it is built on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    /// So many degrees of the scale up.
    Degrees(i32),
    /// So many octaves up, or down below zero.
    Octaves(i32),
}

/// The chords `CH1` and `CH2` carry beside the note, in the order `CHORD`
/// turns through them: two tones, a fourth port being the gate's
/// (journal/D-047, revision of 2026-11-26).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chord {
    /// The third and the fifth.
    Triad,
    /// The third and the seventh, the fifth left out as a shell voicing
    /// leaves it.
    Seventh,
    /// The second and the fifth.
    Sus2,
    /// The fourth and the fifth.
    Sus4,
    /// An octave up and one down.
    Octaves,
}

impl Chord {
    /// Every chord, in the order `CHORD` turns through them.
    pub const ALL: [Self; 5] = [
        Self::Triad,
        Self::Seventh,
        Self::Sus2,
        Self::Sus4,
        Self::Octaves,
    ];

    /// Zero-based index of this chord in [`Self::ALL`].
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The chord with the given index, the triad past the last.
    #[must_use]
    pub const fn from_index(index: usize) -> Self {
        if index < Self::ALL.len() {
            Self::ALL[index]
        } else {
            Self::Triad
        }
    }

    const fn tones(self) -> [Tone; CHORD_TONES] {
        match self {
            Self::Triad => [Tone::Degrees(2), Tone::Degrees(4)],
            Self::Seventh => [Tone::Degrees(2), Tone::Degrees(6)],
            Self::Sus2 => [Tone::Degrees(1), Tone::Degrees(4)],
            Self::Sus4 => [Tone::Degrees(3), Tone::Degrees(4)],
            Self::Octaves => [Tone::Octaves(1), Tone::Octaves(-1)],
        }
    }
}

/// [`Chord::ALL`] as the screen writes them.
pub const CHORD_NAMES: [&str; Chord::ALL.len()] = ["TRIAD", "7TH", "SUS2", "SUS4", "OCT"];

/// The tones of a chord beside its note.
pub const CHORD_TONES: usize = 2;

/// The tones of `chord` on `note` (semitones above the root, in `scale`),
/// lowest first, once inverted: `inversion` above 0 raises the lower tone an
/// octave, below 0 lowers the upper one, 0 leaves them as stacked.
///
/// The thirds are stacked in the scale, so the chord is the scale's own; the
/// chromatic scale, whose thirds would be seconds, borrows the major scale's
/// from the note.
#[must_use]
pub fn chord(scale: Scale, note: i32, chord: Chord, inversion: i8) -> [i32; CHORD_TONES] {
    let (harmony, base) = match scale {
        Scale::Chromatic => (Scale::Major, 0),
        _ => (scale, note),
    };
    let offset = note - base;
    let mut tones = chord.tones().map(|tone| match tone {
        Tone::Degrees(degrees) => harmony.step(base, degrees) + offset,
        Tone::Octaves(octaves) => note + 12 * octaves,
    });
    tones.sort_unstable();
    match inversion.signum() {
        1 => tones[0] += 12,
        -1 => tones[CHORD_TONES - 1] -= 12,
        _ => {}
    }
    tones.sort_unstable();
    tones
}

#[cfg(test)]
mod tests {
    use super::{CHORD_NAMES, Chord, SCALE_NAMES, Scale, chord};

    #[test]
    fn every_scale_holds_its_root_and_its_count() {
        let counts = [12, 7, 7, 7, 7, 7, 7, 7, 7, 5, 5, 6, 6];
        for (scale, count) in Scale::ALL.into_iter().zip(counts) {
            assert!(scale.contains(0), "{scale:?}");
            assert_eq!(scale.notes_per_octave(), count, "{scale:?}");
            assert_eq!(Scale::from_index(scale.index()), scale);
        }
        assert_eq!(SCALE_NAMES.len(), Scale::ALL.len());
        assert_eq!(CHORD_NAMES.len(), Chord::ALL.len());
    }

    #[test]
    fn the_major_scale_is_the_white_keys() {
        let white: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];
        for note in 0..12 {
            assert_eq!(Scale::Major.contains(note), white.contains(&note), "{note}");
        }
    }

    #[test]
    fn a_note_goes_to_the_nearest_lower_on_a_tie() {
        // C# between C and D: down to C.
        assert_eq!(Scale::Major.quantize(1), 0);
        // Below the root, in the octave under.
        assert_eq!(Scale::Major.quantize(-1), -1);
        assert_eq!(Scale::PentaMajor.quantize(-2), -3, "A below C, the nearer");
        assert_eq!(Scale::PentaMajor.quantize(5), 4, "F to E");
        assert_eq!(Scale::Chromatic.quantize(5), 5);
    }

    #[test]
    fn degrees_and_notes_are_inverse() {
        for scale in Scale::ALL {
            for degree in -20..40 {
                let note = scale.note_of(degree);
                assert!(scale.contains(note));
                assert_eq!(scale.degree_of(note), degree, "{scale:?} {degree}");
            }
        }
        assert_eq!(Scale::Major.note_of(7), 12);
        assert_eq!(Scale::Major.step(4, 1), 5, "E to F");
        assert_eq!(Scale::Major.step(0, -1), -1, "C to B below");
    }

    #[test]
    fn chords_stack_thirds_in_the_scale() {
        assert_eq!(chord(Scale::Major, 0, Chord::Triad, 0), [4, 7], "C: E G");
        assert_eq!(chord(Scale::Major, 2, Chord::Triad, 0), [5, 9], "D minor");
        assert_eq!(chord(Scale::Major, 0, Chord::Seventh, 0), [4, 11]);
        assert_eq!(chord(Scale::Major, 0, Chord::Sus2, 0), [2, 7]);
        assert_eq!(chord(Scale::Major, 0, Chord::Sus4, 0), [5, 7]);
        assert_eq!(chord(Scale::Minor, 0, Chord::Triad, 0), [3, 7]);
        assert_eq!(chord(Scale::Major, 0, Chord::Octaves, 0), [-12, 12]);
    }

    #[test]
    fn the_chromatic_scale_borrows_major_thirds() {
        assert_eq!(chord(Scale::Chromatic, 1, Chord::Triad, 0), [5, 8]);
    }

    #[test]
    fn an_inversion_raises_the_lower_tone_or_lowers_the_upper() {
        assert_eq!(chord(Scale::Major, 0, Chord::Triad, 1), [7, 16], "E up");
        assert_eq!(chord(Scale::Major, 0, Chord::Triad, -1), [-5, 4], "G down");
        assert_eq!(
            chord(Scale::Major, 0, Chord::Triad, 9),
            [7, 16],
            "one tone at most"
        );
    }
}
