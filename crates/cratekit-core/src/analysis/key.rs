//! Musical keys and the Camelot wheel (SPEC, Module 2).
//!
//! DJs mix harmonically with the Camelot wheel: 12 numbers around a circle,
//! `A` for minor keys and `B` for major keys. Two tracks mix well when they
//! share a code, sit one step apart on the wheel with the same letter, or
//! share a number with the other letter (a key and its relative major/minor).
//!
//! # Examples
//!
//! ```
//! use cratekit_core::analysis::key::{Camelot, Key, Mode, PitchClass};
//!
//! let a_minor = Key { tonic: PitchClass::A, mode: Mode::Minor };
//! let code = a_minor.camelot();
//! assert_eq!(code.to_string(), "8A");
//!
//! // C major is A minor's relative major: same number, other letter.
//! let c_major: Camelot = "8B".parse()?;
//! assert!(code.matches(c_major));
//! assert_eq!(c_major.key().to_string(), "C major");
//! # Ok::<(), cratekit_core::analysis::key::ParseCamelotError>(())
//! ```

use std::fmt;
use std::str::FromStr;

/// One of the 12 notes of the chromatic scale, C to B (sharps, no flats).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PitchClass {
    /// C
    C,
    /// C sharp (D flat)
    CSharp,
    /// D
    D,
    /// D sharp (E flat)
    DSharp,
    /// E
    E,
    /// F
    F,
    /// F sharp (G flat)
    FSharp,
    /// G
    G,
    /// G sharp (A flat)
    GSharp,
    /// A
    A,
    /// A sharp (B flat)
    ASharp,
    /// B
    B,
}

impl PitchClass {
    /// All 12 pitch classes in order, so `ALL[p.index()]` is `p`.
    pub const ALL: [Self; 12] = [
        Self::C,
        Self::CSharp,
        Self::D,
        Self::DSharp,
        Self::E,
        Self::F,
        Self::FSharp,
        Self::G,
        Self::GSharp,
        Self::A,
        Self::ASharp,
        Self::B,
    ];

    /// Semitones above C: 0 for C up to 11 for B.
    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            Self::C => 0,
            Self::CSharp => 1,
            Self::D => 2,
            Self::DSharp => 3,
            Self::E => 4,
            Self::F => 5,
            Self::FSharp => 6,
            Self::G => 7,
            Self::GSharp => 8,
            Self::A => 9,
            Self::ASharp => 10,
            Self::B => 11,
        }
    }

    /// The pitch class `index` semitones above C, wrapping every octave
    /// (so 12 is C again and 14 is D).
    #[must_use]
    pub const fn from_index(index: u8) -> Self {
        Self::ALL[(index % 12) as usize]
    }

    /// The note name, e.g. `"C#"`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::C => "C",
            Self::CSharp => "C#",
            Self::D => "D",
            Self::DSharp => "D#",
            Self::E => "E",
            Self::F => "F",
            Self::FSharp => "F#",
            Self::G => "G",
            Self::GSharp => "G#",
            Self::A => "A",
            Self::ASharp => "A#",
            Self::B => "B",
        }
    }
}

impl fmt::Display for PitchClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Major or minor (natural minor for the scale).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Major key (Camelot letter `B`).
    Major,
    /// Minor key (Camelot letter `A`).
    Minor,
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Major => "major",
            Self::Minor => "minor",
        })
    }
}

/// Scale steps above the tonic, in semitones.
const MAJOR_STEPS: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
const MINOR_STEPS: [u8; 7] = [0, 2, 3, 5, 7, 8, 10];

/// A musical key: a tonic note and a mode, e.g. A minor.
///
/// Displays as `"A minor"` or `"C# major"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    /// The home note of the key.
    pub tonic: PitchClass,
    /// Major or minor.
    pub mode: Mode,
}

impl Key {
    /// The key's position on the Camelot wheel.
    ///
    /// Each step of a fifth (7 semitones) moves one number round the wheel,
    /// and C major is 8B. A minor key sits on the same number as its
    /// relative major, which is 3 semitones higher (A minor and C major are
    /// both 8).
    #[must_use]
    pub const fn camelot(self) -> Camelot {
        let (major_tonic, letter) = match self.mode {
            Mode::Major => (self.tonic.index(), Letter::B),
            Mode::Minor => ((self.tonic.index() + 3) % 12, Letter::A),
        };
        Camelot {
            number: (major_tonic * 7 + 7) % 12 + 1,
            letter,
        }
    }

    /// Which of the 12 pitch classes (C to B) belong to this key's scale:
    /// the major scale, or the natural minor scale. Always 7 of the 12.
    #[must_use]
    pub fn scale(self) -> [bool; 12] {
        let steps = match self.mode {
            Mode::Major => MAJOR_STEPS,
            Mode::Minor => MINOR_STEPS,
        };
        let mut in_key = [false; 12];
        for step in steps {
            in_key[usize::from((self.tonic.index() + step) % 12)] = true;
        }
        in_key
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.tonic, self.mode)
    }
}

/// The letter half of a Camelot code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Letter {
    /// Minor keys.
    A,
    /// Major keys.
    B,
}

impl Letter {
    /// The other letter: a key's relative major or minor.
    #[must_use]
    pub const fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

/// A position on the Camelot wheel, e.g. `8A`.
///
/// The number is always 1 to 12: the fields are private, so a `Camelot` can
/// only come from [`Camelot::new`], [`Key::camelot`] or parsing, which all
/// check it. Displays and parses as `"8A"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Camelot {
    number: u8,
    letter: Letter,
}

impl Camelot {
    /// A Camelot code, or `None` when `number` is not 1 to 12.
    #[must_use]
    pub const fn new(number: u8, letter: Letter) -> Option<Self> {
        if number >= 1 && number <= 12 {
            Some(Self { number, letter })
        } else {
            None
        }
    }

    /// The wheel number, 1 to 12.
    #[must_use]
    pub const fn number(self) -> u8 {
        self.number
    }

    /// `A` (minor) or `B` (major).
    #[must_use]
    pub const fn letter(self) -> Letter {
        self.letter
    }

    /// The key at this position: the reverse of [`Key::camelot`].
    #[must_use]
    pub const fn key(self) -> Key {
        // Undo `number = (7 * tonic + 7) % 12 + 1`. Multiplying by 7 twice
        // gives 49, which is 1 more than 48, so 7 undoes itself modulo 12.
        let major_tonic = (7 * ((self.number + 4) % 12)) % 12;
        match self.letter {
            Letter::B => Key {
                tonic: PitchClass::from_index(major_tonic),
                mode: Mode::Major,
            },
            Letter::A => Key {
                tonic: PitchClass::from_index(major_tonic + 9),
                mode: Mode::Minor,
            },
        }
    }

    /// True when the two keys mix well: the same code, one step apart on
    /// the wheel with the same letter (12 and 1 are neighbours), or the same
    /// number with the other letter.
    #[must_use]
    pub const fn matches(self, other: Self) -> bool {
        // `==` on enums is not usable in a `const fn` yet; comparing the
        // discriminants (the variant numbers) is the same test.
        if self.letter as u8 == other.letter as u8 {
            let distance = (self.number + 12 - other.number) % 12;
            distance == 0 || distance == 1 || distance == 11
        } else {
            self.number == other.number
        }
    }

    /// The other codes that [`matches`](Self::matches) this one, in the
    /// order the UI shows them: the relative major or minor (same number,
    /// other letter), then the neighbours one step down and one step up the
    /// wheel. The code itself is left out: it matches too, but it is not a
    /// partner (this is what the UI's `mixesWellWith` expects).
    #[must_use]
    pub const fn mixes_well_with(self) -> [Self; 3] {
        let previous = (self.number + 10) % 12 + 1;
        let next = self.number % 12 + 1;
        [
            Self {
                number: self.number,
                letter: self.letter.other(),
            },
            Self {
                number: previous,
                letter: self.letter,
            },
            Self {
                number: next,
                letter: self.letter,
            },
        ]
    }
}

impl fmt::Display for Camelot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let letter = match self.letter {
            Letter::A => 'A',
            Letter::B => 'B',
        };
        write!(f, "{}{letter}", self.number)
    }
}

/// Why a string is not a Camelot code.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseCamelotError {
    /// The string was empty (or only spaces).
    #[error("empty Camelot code")]
    Empty,
    /// The last character was not `A` or `B`.
    #[error("a Camelot code ends in A or B, got {0:?}")]
    Letter(String),
    /// The part before the letter was not a number from 1 to 12.
    #[error("a Camelot number is 1 to 12, got {0:?}")]
    Number(String),
}

impl FromStr for Camelot {
    type Err = ParseCamelotError;

    /// Parses `"8A"` or `"12b"`: a number from 1 to 12 then `A` or `B`, in
    /// either case, with surrounding spaces ignored.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let Some(last) = s.chars().last() else {
            return Err(ParseCamelotError::Empty);
        };
        let letter = match last.to_ascii_uppercase() {
            'A' => Letter::A,
            'B' => Letter::B,
            _ => return Err(ParseCamelotError::Letter(last.to_string())),
        };
        let digits = &s[..s.len() - last.len_utf8()];
        // Digits only: `u8::from_str` would also accept a leading `+`.
        let number = if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            digits.parse::<u8>().ok()
        } else {
            None
        };
        number
            .and_then(|n| Self::new(n, letter))
            .ok_or_else(|| ParseCamelotError::Number(digits.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Camelot table from docs/SPEC.md (Module 2), typed out by hand so a
    /// wrong formula in `Key::camelot` cannot pass.
    const SPEC_TABLE: [(PitchClass, &str, &str); 12] = [
        (PitchClass::C, "8B", "5A"),
        (PitchClass::CSharp, "3B", "12A"),
        (PitchClass::D, "10B", "7A"),
        (PitchClass::DSharp, "5B", "2A"),
        (PitchClass::E, "12B", "9A"),
        (PitchClass::F, "7B", "4A"),
        (PitchClass::FSharp, "2B", "11A"),
        (PitchClass::G, "9B", "6A"),
        (PitchClass::GSharp, "4B", "1A"),
        (PitchClass::A, "11B", "8A"),
        (PitchClass::ASharp, "6B", "3A"),
        (PitchClass::B, "1B", "10A"),
    ];

    /// Every key with its code from the SPEC table.
    fn all_keys() -> Vec<(Key, &'static str)> {
        SPEC_TABLE
            .iter()
            .flat_map(|&(tonic, major, minor)| {
                [
                    (
                        Key {
                            tonic,
                            mode: Mode::Major,
                        },
                        major,
                    ),
                    (
                        Key {
                            tonic,
                            mode: Mode::Minor,
                        },
                        minor,
                    ),
                ]
            })
            .collect()
    }

    fn code(s: &str) -> Camelot {
        s.parse().unwrap()
    }

    #[test]
    fn every_key_maps_to_its_spec_code() {
        let keys = all_keys();
        assert_eq!(keys.len(), 24);
        for (key, expected) in keys {
            assert_eq!(key.camelot().to_string(), expected, "{key}");
        }
    }

    #[test]
    fn every_code_maps_back_to_its_key_and_parses() {
        for (key, expected) in all_keys() {
            let camelot = key.camelot();
            assert_eq!(camelot.key(), key, "{expected}");
            assert_eq!(code(expected), camelot);
        }
    }

    #[test]
    fn the_24_codes_are_all_different() {
        let mut codes: Vec<String> = all_keys()
            .iter()
            .map(|(k, _)| k.camelot().to_string())
            .collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), 24);
    }

    #[test]
    fn matches_itself_and_exactly_the_three_partners_for_every_key() {
        let all: Vec<Camelot> = all_keys().iter().map(|(k, _)| k.camelot()).collect();
        for &a in &all {
            let partners = a.mixes_well_with();
            assert!(!partners.contains(&a), "{a} lists itself");
            for &b in &all {
                assert_eq!(a.matches(b), a == b || partners.contains(&b), "{a} vs {b}");
                assert_eq!(a.matches(b), b.matches(a), "symmetry {a} vs {b}");
            }
            assert_eq!(all.iter().filter(|&&b| a.matches(b)).count(), 4, "{a}");
        }
    }

    #[test]
    fn neighbours_and_relatives_match() {
        let eight_a = code("8A");
        assert!(eight_a.matches(code("8A")));
        assert!(eight_a.matches(code("7A")));
        assert!(eight_a.matches(code("9A")));
        assert!(eight_a.matches(code("8B")));
        assert!(!eight_a.matches(code("9B")));
        assert!(!eight_a.matches(code("10A")));
        assert!(!eight_a.matches(code("2A")));
    }

    #[test]
    fn the_wheel_wraps_from_12_to_1() {
        assert!(code("12A").matches(code("1A")));
        assert!(code("1B").matches(code("12B")));
        assert_eq!(
            code("12A").mixes_well_with().map(|c| c.to_string()),
            ["12B", "11A", "1A"]
        );
        assert_eq!(
            code("1B").mixes_well_with().map(|c| c.to_string()),
            ["1A", "12B", "2B"]
        );
    }

    #[test]
    fn parses_codes_in_either_case_with_spaces() {
        assert_eq!(code("8A"), Camelot::new(8, Letter::A).unwrap());
        assert_eq!(code(" 12b "), Camelot::new(12, Letter::B).unwrap());
        assert_eq!(code("1a").to_string(), "1A");
    }

    #[test]
    fn rejects_malformed_codes() {
        let err = |s: &str| s.parse::<Camelot>().unwrap_err();
        assert_eq!(err(""), ParseCamelotError::Empty);
        assert_eq!(err("   "), ParseCamelotError::Empty);
        assert_eq!(err("0A"), ParseCamelotError::Number("0".into()));
        assert_eq!(err("13B"), ParseCamelotError::Number("13".into()));
        assert_eq!(err("+8A"), ParseCamelotError::Number("+8".into()));
        assert_eq!(err("A"), ParseCamelotError::Number(String::new()));
        assert_eq!(err("8C"), ParseCamelotError::Letter("C".into()));
        assert_eq!(err("A8"), ParseCamelotError::Letter("8".into()));
        assert_eq!(err("8"), ParseCamelotError::Letter("8".into()));
        assert_eq!(err("8é"), ParseCamelotError::Letter("é".into()));
    }

    #[test]
    fn new_checks_the_range() {
        assert!(Camelot::new(0, Letter::A).is_none());
        assert!(Camelot::new(13, Letter::B).is_none());
        assert_eq!(Camelot::new(12, Letter::A).map(Camelot::number), Some(12));
    }

    #[test]
    fn scales_have_seven_notes_and_include_the_tonic() {
        for (key, _) in all_keys() {
            let scale = key.scale();
            assert_eq!(scale.iter().filter(|&&n| n).count(), 7, "{key}");
            assert!(scale[usize::from(key.tonic.index())], "{key}");
        }
    }

    #[test]
    fn c_major_is_the_white_keys_and_a_minor_shares_them() {
        let c_major = Key {
            tonic: PitchClass::C,
            mode: Mode::Major,
        };
        let a_minor = Key {
            tonic: PitchClass::A,
            mode: Mode::Minor,
        };
        let white: Vec<&str> = PitchClass::ALL
            .iter()
            .filter(|p| c_major.scale()[usize::from(p.index())])
            .map(|p| p.name())
            .collect();
        assert_eq!(white, ["C", "D", "E", "F", "G", "A", "B"]);
        assert_eq!(a_minor.scale(), c_major.scale());
    }

    #[test]
    fn relative_keys_share_a_scale_and_a_number() {
        for (key, _) in all_keys()
            .into_iter()
            .filter(|(k, _)| k.mode == Mode::Major)
        {
            let relative = Key {
                tonic: PitchClass::from_index(key.tonic.index() + 9),
                mode: Mode::Minor,
            };
            assert_eq!(key.scale(), relative.scale(), "{key} vs {relative}");
            assert_eq!(key.camelot().number(), relative.camelot().number());
        }
    }

    #[test]
    fn names_read_like_musicians_write_them() {
        let key = |tonic, mode| Key { tonic, mode }.to_string();
        assert_eq!(key(PitchClass::A, Mode::Minor), "A minor");
        assert_eq!(key(PitchClass::CSharp, Mode::Major), "C# major");
    }

    #[test]
    fn pitch_class_index_round_trips_and_wraps() {
        for p in PitchClass::ALL {
            assert_eq!(PitchClass::from_index(p.index()), p);
        }
        assert_eq!(PitchClass::from_index(12), PitchClass::C);
        assert_eq!(PitchClass::from_index(14), PitchClass::D);
    }
}
