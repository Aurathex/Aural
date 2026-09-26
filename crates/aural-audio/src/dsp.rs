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

#[cfg(test)]
mod tests {
    use super::*;

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
