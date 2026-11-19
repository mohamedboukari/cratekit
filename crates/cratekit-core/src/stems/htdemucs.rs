//! The htdemucs separator (#31): runs Intel's split export on ONNX Runtime,
//! with the signal processing of [`super::dsp`] around it. The model's drums,
//! bass and vocals are kept; "other" is the mix minus those three, so the four
//! stems always sum back to the original exactly.

use std::path::Path;

use ort::session::Session;
use ort::value::Tensor;

use super::dsp::{
    Blend, CHANNELS, FRAMES, FREQS, SAMPLE_RATE, SEGMENT, Stft, TrackScale, chunks, model_input,
    model_output,
};
use super::runtime::Runtime;
use super::{StemSeparator, Stems};
use crate::CoreError;
use crate::audio::{AudioBuffer, resample};
use crate::progress::{CancellationToken, ProgressSink};

/// The graph file in the model's folder (its weights file sits next to it).
pub const GRAPH_FILE: &str = "htdemucs_fwd.onnx";

/// A loaded htdemucs model, ready to split tracks.
#[derive(Debug)]
pub struct HtDemucs {
    session: Session,
    stft: Stft,
}

fn stems_error(err: impl std::fmt::Display) -> CoreError {
    CoreError::Stems(err.to_string())
}

impl HtDemucs {
    /// Loads the model from its folder (as downloaded by
    /// [`super::models::HTDEMUCS`]) on the CPU.
    ///
    /// # Errors
    ///
    /// [`CoreError::Stems`] when ONNX Runtime cannot load the model.
    pub fn new(runtime: &Runtime, model_dir: &Path) -> Result<Self, CoreError> {
        let session = Session::builder(runtime.environment())
            .map_err(stems_error)?
            .commit_from_file(model_dir.join(GRAPH_FILE))
            .map_err(stems_error)?;
        Ok(Self {
            session,
            stft: Stft::new(),
        })
    }
}

impl StemSeparator for HtDemucs {
    fn separate(
        &mut self,
        audio: AudioBuffer,
        progress: &dyn ProgressSink,
        cancel: &CancellationToken,
    ) -> Result<Stems, CoreError> {
        let audio = if audio.sample_rate() == SAMPLE_RATE {
            audio
        } else {
            resample(audio, SAMPLE_RATE)?
        };
        let mut track: Vec<Vec<f32>> = audio.into_channels();
        if track.len() == 1 {
            let mono = track.first().cloned().unwrap_or_default();
            track.push(mono);
        }
        track.truncate(CHANNELS);
        let frames = track.iter().map(Vec::len).min().unwrap_or(0);
        if frames == 0 {
            return Err(CoreError::InvalidAudio("the track has no samples"));
        }
        let mix: Vec<Vec<f32>> = track
            .iter()
            .map(|channel| channel.get(..frames).unwrap_or_default().to_vec())
            .collect();
        let scale = TrackScale::normalize(&mut track);
        let segments = chunks(frames);
        let mut blend = Blend::new(frames);
        progress.progress(0.0);
        for (index, chunk) in segments.iter().enumerate() {
            cancel.check()?;
            let input = model_input(&mut self.stft, &chunk.window(&track));
            let spectrogram = Tensor::from_array((
                [1_usize, 2 * CHANNELS, FREQS, FRAMES],
                input.spectrogram.clone(),
            ))
            .map_err(stems_error)?;
            let waveform =
                Tensor::from_array(([1_usize, CHANNELS, SEGMENT], input.waveform.clone()))
                    .map_err(stems_error)?;
            let outputs = self
                .session
                .run(ort::inputs![spectrogram, waveform])
                .map_err(stems_error)?;
            let (_, spec_out) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(stems_error)?;
            let (_, wave_out) = outputs[1]
                .try_extract_tensor::<f32>()
                .map_err(stems_error)?;
            let separated = model_output(&mut self.stft, &input, spec_out, wave_out);
            blend.add(chunk, &separated);
            #[expect(clippy::cast_precision_loss, reason = "segment counts are small")]
            progress.progress((index + 1) as f32 / segments.len() as f32);
        }
        let mut sources: Vec<Vec<Vec<f32>>> = blend
            .finish()
            .into_iter()
            .map(|channels| {
                let mut channels = Vec::from(channels);
                scale.undo(&mut channels);
                channels
            })
            .collect();
        let [drums, bass, _, vocals] = [0, 1, 2, 3].map(|index| {
            sources
                .get_mut(index)
                .map(std::mem::take)
                .unwrap_or_default()
        });
        // "Other" is what the three other stems leave of the mix.
        let other: Vec<Vec<f32>> = mix
            .iter()
            .enumerate()
            .map(|(channel, samples)| {
                let take = |stem: &[Vec<f32>], i: usize| {
                    stem.get(channel)
                        .and_then(|c| c.get(i))
                        .copied()
                        .unwrap_or(0.0)
                };
                samples
                    .iter()
                    .enumerate()
                    .map(|(i, &m)| m - take(&drums, i) - take(&bass, i) - take(&vocals, i))
                    .collect()
            })
            .collect();
        Ok(Stems {
            drums: AudioBuffer::new(SAMPLE_RATE, drums)?,
            bass: AudioBuffer::new(SAMPLE_RATE, bass)?,
            other: AudioBuffer::new(SAMPLE_RATE, other)?,
            vocals: AudioBuffer::new(SAMPLE_RATE, vocals)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::NoProgress;
    use crate::stems::StemSeparator;
    use crate::stems::runtime::load;

    /// A mix of a kick on every beat, a bass line and a chord, 12 s.
    fn mix() -> AudioBuffer {
        let rate = f64::from(SAMPLE_RATE);
        let frames = 12 * SAMPLE_RATE as usize;
        let samples: Vec<f32> = (0..frames)
            .map(|i| {
                let t = i as f64 / rate;
                let beat = t % 0.5;
                let kick = (2.0 * std::f64::consts::PI * 55.0 * beat).sin() * (-beat * 30.0).exp();
                let bass = 0.3 * (2.0 * std::f64::consts::PI * 82.4 * t).sin();
                let chord = [261.6, 329.6, 392.0]
                    .iter()
                    .map(|f| 0.08 * (2.0 * std::f64::consts::PI * f * t).sin())
                    .sum::<f64>();
                (0.6 * kick + bass + chord) as f32
            })
            .collect();
        AudioBuffer::new(SAMPLE_RATE, vec![samples.clone(), samples]).expect("buffer")
    }

    /// Splits a synthetic mix with the real model and checks the SPEC's
    /// reconstruction rule (exact, up to rounding, since "other" is the
    /// remainder) and that the model did split it (the bass line lands in the
    /// bass stem).
    /// Needs `CRATEKIT_ONNXRUNTIME` (a library) and `CRATEKIT_STEMS_MODEL`
    /// (the folder with the downloaded model).
    #[test]
    #[ignore = "needs CRATEKIT_ONNXRUNTIME and CRATEKIT_STEMS_MODEL"]
    fn the_stems_sum_back_to_the_mix() {
        let library = std::env::var_os("CRATEKIT_ONNXRUNTIME").expect("CRATEKIT_ONNXRUNTIME");
        let model = std::env::var_os("CRATEKIT_STEMS_MODEL").expect("CRATEKIT_STEMS_MODEL");
        let runtime = load(Path::new(&library)).expect("runtime");
        let mut separator = HtDemucs::new(runtime, Path::new(&model)).expect("model");
        let original = mix();
        let stems = separator
            .separate(original.clone(), &NoProgress, &CancellationToken::new())
            .expect("separate");
        let (mut residual, mut energy) = (0.0_f64, 0.0_f64);
        for channel in 0..2 {
            let mix = &original.channels()[channel];
            for (i, &m) in mix.iter().enumerate() {
                let sum: f32 = [&stems.drums, &stems.bass, &stems.other, &stems.vocals]
                    .iter()
                    .map(|s| s.channels()[channel][i])
                    .sum();
                residual += f64::from(sum - m).powi(2);
                energy += f64::from(m).powi(2);
            }
        }
        let db = 10.0 * (residual / energy).log10();
        assert!(db < -100.0, "residual {db:.1} dB");
        let power = |stem: &AudioBuffer| -> f64 {
            stem.channels()[0]
                .iter()
                .map(|&v| f64::from(v).powi(2))
                .sum()
        };
        assert!(
            power(&stems.bass) > 100.0 * power(&stems.vocals),
            "the bass line is in the bass stem"
        );
        assert_eq!(stems.drums.frames(), original.frames());
    }
}
