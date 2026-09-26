//! Silence handling: a recording with no speech must not reach the engine (Whisper
//! hallucinates "Thank you." on silence), and leading/trailing silence is trimmed.

/// RMS above which a 10 ms frame counts as sound (about -40 dBFS).
const SOUND_RMS: f32 = 0.01;

fn rms(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt()
}

/// True when no 20 ms frame rises above the sound threshold.
pub fn is_silent(pcm: &[f32]) -> bool {
    pcm.chunks(320).all(|f| rms(f) < SOUND_RMS)
}

/// Drop leading and trailing silence, keeping 200 ms of padding around the sound.
pub fn trim_silence(pcm: &[f32], rate: u32) -> &[f32] {
    let frame = (rate / 100).max(1) as usize;
    let loud = |f: &[f32]| rms(f) >= SOUND_RMS;
    let frames: Vec<&[f32]> = pcm.chunks(frame).collect();
    let (Some(first), Some(last)) = (
        frames.iter().position(|f| loud(f)),
        frames.iter().rposition(|f| loud(f)),
    ) else {
        return &[];
    };
    let pad = (rate / 5) as usize;
    let start = (first * frame).saturating_sub(pad);
    let end = ((last + 1) * frame + pad).min(pcm.len());
    &pcm[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(len: usize, amp: f32) -> Vec<f32> {
        // Deterministic pseudo-noise.
        let mut x = 12345u32;
        (0..len)
            .map(|_| {
                x = x.wrapping_mul(1103515245).wrapping_add(12345);
                amp * ((x >> 16) as f32 / 32768.0 - 1.0)
            })
            .collect()
    }

    #[test]
    fn digital_silence_is_silent() {
        assert!(is_silent(&vec![0.0; 16_000]));
    }

    #[test]
    fn room_hiss_is_silent_but_speech_level_is_not() {
        assert!(is_silent(&noise(16_000, 0.003)));
        assert!(!is_silent(&noise(16_000, 0.2)));
    }

    #[test]
    fn empty_is_silent() {
        assert!(is_silent(&[]));
    }

    #[test]
    fn trim_keeps_the_loud_region_with_padding() {
        let mut pcm = vec![0.0; 16_000];
        pcm.extend(noise(8_000, 0.3));
        pcm.extend(vec![0.0; 16_000]);
        let t = trim_silence(&pcm, 16_000);
        // 8000 samples of speech + up to 200 ms (3200 samples) padding each side.
        assert!(
            t.len() >= 8_000 && t.len() <= 8_000 + 2 * 3_200 + 2 * 320,
            "{}",
            t.len()
        );
    }

    #[test]
    fn trim_of_all_silence_is_empty() {
        assert!(trim_silence(&vec![0.0; 1600], 16_000).is_empty());
    }
}
