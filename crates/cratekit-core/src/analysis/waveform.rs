//! Waveform columns (SPEC, Module 2): the shape of a whole track, computed
//! once during analysis and stored with it.
//!
//! Each column covers `1 / COLUMNS_PER_SECOND` of audio and keeps its lowest
//! and highest sample and its RMS, over every channel. Values are stored as
//! one byte each (about 22 KB for six minutes), which is plenty for drawing.

use crate::audio::AudioBuffer;

/// Columns per second of audio: one every 50 ms.
pub const COLUMNS_PER_SECOND: u32 = 20;

/// Columns the deck draws (an overview of the whole track); see
/// [`Waveform::overview`].
pub const OVERVIEW_COLUMNS: usize = 1200;

/// Format of the stored bytes; bump when the layout changes.
const FORMAT_VERSION: u8 = 1;

/// The stored waveform of a track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waveform {
    /// Lowest sample per column, scaled so -127 is full scale.
    min: Vec<i8>,
    /// Highest sample per column, scaled so 127 is full scale.
    max: Vec<i8>,
    /// RMS per column, scaled so 255 is full scale.
    rms: Vec<u8>,
}

/// What the deck draws: one peak and one RMS value per column, 0 to 1.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct WaveformView {
    /// Highest absolute level per column.
    pub peaks: Vec<f32>,
    /// RMS per column.
    pub rms: Vec<f32>,
}

impl Waveform {
    /// Computes the columns of `audio` (all channels together).
    #[must_use]
    pub fn of(audio: &AudioBuffer) -> Self {
        let width = (audio.sample_rate() / COLUMNS_PER_SECOND).max(1) as usize;
        let frames = audio.frames();
        let count = frames.div_ceil(width);
        let mut waveform = Self {
            min: Vec::with_capacity(count),
            max: Vec::with_capacity(count),
            rms: Vec::with_capacity(count),
        };
        for start in (0..frames).step_by(width) {
            let end = (start + width).min(frames);
            let (mut low, mut high, mut squares, mut samples) = (0.0_f32, 0.0_f32, 0.0_f64, 0_u32);
            for channel in audio.channels() {
                for &sample in channel.get(start..end).unwrap_or_default() {
                    let sample = if sample.is_finite() { sample } else { 0.0 };
                    low = low.min(sample);
                    high = high.max(sample);
                    squares += f64::from(sample) * f64::from(sample);
                    samples += 1;
                }
            }
            let rms = if samples > 0 {
                (squares / f64::from(samples)).sqrt() as f32
            } else {
                0.0
            };
            waveform.min.push(to_signed(low));
            waveform.max.push(to_signed(high));
            waveform.rms.push(to_unsigned(rms));
        }
        waveform
    }

    /// Number of columns.
    #[must_use]
    pub fn columns(&self) -> usize {
        self.rms.len()
    }

    /// The bytes stored in the library: a format byte, the column count (u32
    /// little-endian), then every min, every max and every RMS.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let count = u32::try_from(self.columns()).unwrap_or(u32::MAX);
        let mut bytes = Vec::with_capacity(5 + 3 * self.columns());
        bytes.push(FORMAT_VERSION);
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend(self.min.iter().map(|&v| v.to_le_bytes()[0]));
        bytes.extend(self.max.iter().map(|&v| v.to_le_bytes()[0]));
        bytes.extend_from_slice(&self.rms);
        bytes
    }

    /// A waveform from stored bytes, or `None` when they are not in the
    /// current format (written by another version, or damaged): the track
    /// then shows no waveform until it is analyzed again.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let (&format, rest) = bytes.split_first()?;
        if format != FORMAT_VERSION {
            return None;
        }
        let (count, rest) = rest.split_first_chunk::<4>()?;
        let count = usize::try_from(u32::from_le_bytes(*count)).ok()?;
        if rest.len() != count.checked_mul(3)? {
            return None;
        }
        let (min, rest) = rest.split_at(count);
        let (max, rms) = rest.split_at(count);
        Some(Self {
            min: min.iter().map(|&b| i8::from_le_bytes([b])).collect(),
            max: max.iter().map(|&b| i8::from_le_bytes([b])).collect(),
            rms: rms.to_vec(),
        })
    }

    /// At most `columns` columns for drawing: each one the highest peak and
    /// the RMS (power average) of the stored columns it covers. Fewer stored
    /// columns than asked are returned as they are.
    #[must_use]
    pub fn overview(&self, columns: usize) -> WaveformView {
        let stored = self.columns();
        let groups = columns.clamp(1, stored.max(1));
        let mut view = WaveformView {
            peaks: Vec::with_capacity(groups),
            rms: Vec::with_capacity(groups),
        };
        for group in 0..groups {
            let start = group * stored / groups;
            let end = ((group + 1) * stored / groups).max(start + 1).min(stored);
            let (mut peak, mut power) = (0.0_f32, 0.0_f32);
            for i in start..end {
                let low = f32::from(self.min.get(i).copied().unwrap_or(0)).abs() / 127.0;
                let high = f32::from(self.max.get(i).copied().unwrap_or(0)).abs() / 127.0;
                let rms = f32::from(self.rms.get(i).copied().unwrap_or(0)) / 255.0;
                peak = peak.max(low).max(high);
                power += rms * rms;
            }
            let span = (end - start).max(1) as f32;
            view.peaks.push(peak.min(1.0));
            view.rms.push((power / span).sqrt().min(1.0));
        }
        if stored == 0 {
            view.peaks.clear();
            view.rms.clear();
        }
        view
    }
}

fn to_signed(value: f32) -> i8 {
    (value.clamp(-1.0, 1.0) * 127.0).round() as i8
}

fn to_unsigned(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::{kick, sine};

    const RATE: u32 = 44_100;

    fn mono(samples: Vec<f32>) -> AudioBuffer {
        AudioBuffer::new(RATE, vec![samples]).expect("a valid buffer")
    }

    #[test]
    fn a_tone_has_steady_columns_at_its_level() {
        // A full-scale sine peaks at 1.0 and has an RMS of 0.707.
        let waveform = Waveform::of(&mono(sine(440.0, 0.0, 10.0, RATE)));
        assert_eq!(waveform.columns(), 200);
        let view = waveform.overview(200);
        assert!(
            view.peaks.iter().all(|&p| (p - 1.0).abs() < 0.02),
            "{:?}",
            &view.peaks[..4]
        );
        assert!(
            view.rms.iter().all(|&r| (r - 0.707).abs() < 0.02),
            "{:?}",
            &view.rms[..4]
        );
    }

    #[test]
    fn kicks_show_as_peaks_and_silence_as_nothing() {
        // Two seconds of silence, then kicks at 120 BPM.
        let mut samples = vec![0.0; 2 * RATE as usize];
        samples.extend(kick(120.0, 4.0, RATE));
        let view = Waveform::of(&mono(samples)).overview(120);
        assert!(view.peaks[..40].iter().all(|&p| p == 0.0), "silence first");
        let loud = view.peaks[40..].iter().filter(|&&p| p > 0.5).count();
        assert!(loud >= 8, "a peak per kick, at least: {loud}");
    }

    #[test]
    fn stores_and_reads_back_the_same_columns() {
        let waveform = Waveform::of(&mono(sine(220.0, -6.0, 3.0, RATE)));
        let bytes = waveform.to_bytes();
        assert_eq!(bytes.len(), 5 + 3 * waveform.columns());
        assert_eq!(Waveform::from_bytes(&bytes), Some(waveform));
        assert_eq!(Waveform::from_bytes(&[]), None);
        assert_eq!(
            Waveform::from_bytes(&[9, 0, 0, 0, 0]),
            None,
            "another format"
        );
        assert_eq!(
            Waveform::from_bytes(&bytes[..bytes.len() - 1]),
            None,
            "cut short"
        );
    }

    #[test]
    fn the_overview_keeps_the_loudest_peak_of_each_group() {
        // One loud column among quiet ones survives a 10 to 1 reduction.
        let mut samples = vec![0.1_f32; 100 * (RATE as usize / 20)];
        samples[55 * (RATE as usize / 20) + 10] = 0.9;
        let view = Waveform::of(&mono(samples)).overview(10);
        assert_eq!(view.peaks.len(), 10);
        assert!((view.peaks[5] - 0.9).abs() < 0.01, "{:?}", view.peaks);
        assert!(
            view.peaks
                .iter()
                .enumerate()
                .all(|(i, &p)| i == 5 || (p - 0.1).abs() < 0.01)
        );
    }

    #[test]
    fn short_or_empty_audio_is_fine() {
        let empty = Waveform::of(&mono(Vec::new()));
        assert_eq!(empty.columns(), 0);
        assert!(empty.overview(OVERVIEW_COLUMNS).peaks.is_empty());
        let short = Waveform::of(&mono(vec![0.5; 100]));
        assert_eq!(short.columns(), 1);
        assert_eq!(short.overview(OVERVIEW_COLUMNS).peaks.len(), 1);
    }
}
