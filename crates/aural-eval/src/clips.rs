//! The built-in test clips (assets/eval): short LibriSpeech recordings with their
//! reference text, used to measure accuracy and speed on the user's PC.

use anyhow::{bail, Context, Result};
use std::path::Path;

pub const SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Clone)]
pub struct Clip {
    pub id: String,
    pub reference: String,
    /// 16 kHz mono, -1.0..1.0
    pub pcm16k: Vec<f32>,
}

/// Reads `clips.tsv` (`id<TAB>file<TAB>reference`) in `dir` and decodes each FLAC file.
pub fn builtin(dir: &Path) -> Result<Vec<Clip>> {
    let list = dir.join("clips.tsv");
    let text =
        std::fs::read_to_string(&list).with_context(|| format!("reading {}", list.display()))?;
    let mut clips = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let (Some(id), Some(file), Some(reference)) = (parts.next(), parts.next(), parts.next())
        else {
            bail!(
                "clips.tsv line {}: expected <id>\\t<file>\\t<reference>",
                i + 1
            );
        };
        clips.push(Clip {
            id: id.to_owned(),
            reference: reference.to_owned(),
            pcm16k: decode_flac(&dir.join(file))?,
        });
    }
    Ok(clips)
}

/// Decodes a 16 kHz mono 16-bit FLAC file; anything else is an error.
pub fn decode_flac(path: &Path) -> Result<Vec<f32>> {
    let mut reader =
        claxon::FlacReader::open(path).with_context(|| format!("opening {}", path.display()))?;
    let info = reader.streaminfo();
    if info.sample_rate != SAMPLE_RATE || info.channels != 1 || info.bits_per_sample != 16 {
        bail!(
            "{}: expected 16 kHz mono 16-bit, got {} Hz, {} channel(s), {} bits",
            path.display(),
            info.sample_rate,
            info.channels,
            info.bits_per_sample
        );
    }
    reader
        .samples()
        .map(|s| {
            s.map(|v| v as f32 / 32_768.0)
                .with_context(|| format!("decoding {}", path.display()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn assets() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/eval")
    }

    #[test]
    fn builtin_clips_are_twenty_short_16k_recordings() {
        let clips = builtin(&assets()).unwrap();
        assert_eq!(clips.len(), 20);
        let mut words = 0;
        for c in &clips {
            assert!(!c.reference.trim().is_empty(), "{}", c.id);
            let secs = c.pcm16k.len() as f64 / SAMPLE_RATE as f64;
            assert!((4.9..=12.1).contains(&secs), "{}: {secs} s", c.id);
            assert!(c.pcm16k.iter().all(|s| s.abs() <= 1.0));
            words += crate::wer::normalize(&c.reference).len();
        }
        assert!((300..=600).contains(&words), "{words} words");
    }

    #[test]
    fn a_missing_list_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        assert!(builtin(d.path()).is_err());
    }

    #[test]
    fn a_malformed_line_names_its_line_number() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("clips.tsv"), "# c\nonly\ttwo\n").unwrap();
        let err = builtin(d.path()).unwrap_err();
        assert!(err.to_string().contains("line 2"), "{err}");
    }

    /// The FLAC copy holds exactly the samples of the source WAV. Needs
    /// AURAL_TEST_LIBRISPEECH_DIR (the librispeech-100 corpus with wav/). `-- --ignored`.
    #[test]
    #[ignore]
    fn flac_decode_matches_the_source_wav_length() {
        let corpus = PathBuf::from(std::env::var("AURAL_TEST_LIBRISPEECH_DIR").unwrap());
        let clips = builtin(&assets()).unwrap();
        for c in clips.iter().take(3) {
            let wav = std::fs::read(corpus.join("wav").join(format!("{}.wav", c.id))).unwrap();
            // 16-bit PCM: the `data` chunk's size / 2 bytes per sample
            let at = wav.windows(4).position(|w| w == b"data").unwrap();
            let bytes = u32::from_le_bytes(wav[at + 4..at + 8].try_into().unwrap());
            assert_eq!(c.pcm16k.len(), bytes as usize / 2, "{}", c.id);
        }
    }
}
