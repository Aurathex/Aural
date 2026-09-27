//! Load WAV files as the 16 kHz mono `f32` audio every STT engine expects.

use anyhow::{Context, Result};
use rubato::audioadapter::Adapter;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use std::path::Path;

pub const TARGET_RATE: u32 = 16_000;

/// Average interleaved frames down to one channel.
pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

pub fn load_wav_16k_mono(path: &Path) -> Result<Vec<f32>> {
    let mut reader =
        hound::WavReader::open(path).with_context(|| format!("open {}", path.display()))?;
    let spec = reader.spec();
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<_, _>>()?
        }
    };
    let mono = downmix(&interleaved, spec.channels as usize);
    resample_mono(&mono, spec.sample_rate)
}

pub fn resample_mono(mono: &[f32], rate: u32) -> Result<Vec<f32>> {
    if rate == TARGET_RATE || mono.is_empty() {
        return Ok(mono.to_vec());
    }
    let mut resampler = Fft::<f32>::new(
        rate as usize,
        TARGET_RATE as usize,
        1024,
        1,
        FixedSync::Both,
    )?;
    let input = InterleavedSlice::new(mono, 1, mono.len())?;
    let out = resampler.process_all(&input, mono.len(), None)?;
    Ok((0..out.frames())
        .map(|i| out.read_sample(0, i).unwrap_or(0.0))
        .collect())
}

/// Resamples a live recording piece by piece to 16 kHz mono. The output is the same
/// audio `resample_mono` produces for the whole recording, delivered as it arrives
/// (the resampler's start-up delay is trimmed, the end flushed by `finish`).
pub struct StreamResampler {
    inner: Option<Fft<f32>>,
    pending: Vec<f32>,
    to_trim: usize,
    consumed: usize,
    produced: usize,
    ratio: f64,
}

impl StreamResampler {
    pub fn new(rate: u32) -> Result<Self> {
        let inner = (rate != TARGET_RATE)
            .then(|| {
                Fft::<f32>::new(
                    rate as usize,
                    TARGET_RATE as usize,
                    1024,
                    1,
                    FixedSync::Both,
                )
            })
            .transpose()?;
        let to_trim = inner.as_ref().map_or(0, |r| r.output_delay());
        Ok(Self {
            inner,
            pending: Vec::new(),
            to_trim,
            consumed: 0,
            produced: 0,
            ratio: TARGET_RATE as f64 / rate as f64,
        })
    }

    fn run(&mut self, chunk: &[f32], partial: Option<usize>, out: &mut Vec<f32>) -> Result<()> {
        let Some(r) = self.inner.as_mut() else {
            return Ok(());
        };
        let input = InterleavedSlice::new(chunk, 1, chunk.len())?;
        let indexing = partial.map(|n| rubato::Indexing {
            input_offset: 0,
            output_offset: 0,
            partial_len: Some(n),
            active_channels_mask: None,
        });
        let res = r.process(&input, indexing.as_ref())?;
        let mut samples: Vec<f32> = (0..res.frames())
            .map(|i| res.read_sample(0, i).unwrap_or(0.0))
            .collect();
        let trim = self.to_trim.min(samples.len());
        samples.drain(..trim);
        self.to_trim -= trim;
        self.produced += samples.len();
        out.extend(samples);
        Ok(())
    }

    /// Add native-rate audio; returns whatever 16 kHz audio is ready.
    pub fn push(&mut self, mono: &[f32]) -> Result<Vec<f32>> {
        self.consumed += mono.len();
        let Some(r) = self.inner.as_ref() else {
            self.produced += mono.len();
            return Ok(mono.to_vec());
        };
        let need = r.input_frames_next();
        self.pending.extend_from_slice(mono);
        let mut out = Vec::new();
        while self.pending.len() >= need {
            let chunk: Vec<f32> = self.pending.drain(..need).collect();
            self.run(&chunk, None, &mut out)?;
        }
        Ok(out)
    }

    /// The rest of the audio, up to the recording's full resampled length.
    pub fn finish(&mut self) -> Result<Vec<f32>> {
        let mut out = Vec::new();
        if self.inner.is_none() {
            return Ok(out);
        }
        let expected = (self.ratio * self.consumed as f64).ceil() as usize;
        let need = self.inner.as_ref().map_or(1, |r| r.input_frames_next());
        let rest = std::mem::take(&mut self.pending);
        let mut chunk = rest.clone();
        chunk.resize(need, 0.0);
        if !rest.is_empty() {
            self.run(&chunk, Some(rest.len()), &mut out)?;
        }
        let silence = vec![0.0; need];
        while self.produced < expected {
            self.run(&silence, Some(0), &mut out)?;
        }
        let extra = self.produced - expected;
        out.truncate(out.len().saturating_sub(extra));
        self.produced = expected;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, secs: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .map(|n| ((n as f32 / rate as f32) * 440.0 * std::f32::consts::TAU).sin() * 0.5)
            .collect()
    }

    #[test]
    fn streaming_resample_matches_whole_recording_resample() {
        for rate in [48_000u32, 44_100, 16_000] {
            let mono = sine(rate, 2.3);
            let whole = resample_mono(&mono, rate).unwrap();
            let mut s = StreamResampler::new(rate).unwrap();
            let mut live = Vec::new();
            // Irregular pieces, like a microphone callback.
            let mut i = 0;
            for (k, n) in [441usize, 3000, 17, 960, 4800].iter().cycle().enumerate() {
                if i >= mono.len() || k > 10_000 {
                    break;
                }
                let end = (i + n).min(mono.len());
                live.extend(s.push(&mono[i..end]).unwrap());
                i = end;
            }
            live.extend(s.finish().unwrap());
            assert_eq!(live.len(), whole.len(), "{rate}");
            let worst = live
                .iter()
                .zip(&whole)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(worst < 1e-3, "{rate}: worst difference {worst}");
        }
    }

    fn write_sine(path: &std::path::Path, rate: u32, channels: u16, secs: f32) {
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for n in 0..(rate as f32 * secs) as u32 {
            let v = ((n as f32 / rate as f32) * 440.0 * std::f32::consts::TAU).sin();
            for _ in 0..channels {
                w.write_sample((v * 16000.0) as i16).unwrap();
            }
        }
        w.finalize().unwrap();
    }

    #[test]
    fn downmix_averages_channels() {
        assert_eq!(downmix(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
    }

    #[test]
    fn resamples_48k_stereo_to_16k_mono() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("s.wav");
        write_sine(&p, 48_000, 2, 1.0);
        let pcm = load_wav_16k_mono(&p).unwrap();
        assert!((pcm.len() as i64 - 16_000).abs() < 400, "len {}", pcm.len());
        assert!(pcm.iter().all(|s| s.abs() <= 1.0));
        // A 440 Hz sine at ~0.49 amplitude must survive resampling, not come out silent.
        let peak = pcm.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.4, "peak {peak}");
    }

    #[test]
    fn passes_through_16k_mono() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("m.wav");
        write_sine(&p, 16_000, 1, 0.5);
        assert_eq!(load_wav_16k_mono(&p).unwrap().len(), 8_000);
    }
}
