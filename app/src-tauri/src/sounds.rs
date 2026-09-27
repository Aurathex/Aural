//! Short start/stop listening sounds, synthesized at startup (no audio files):
//! two soft sine notes a fifth apart, rising for start and falling for stop.

use aural_core::session::PillState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    Start,
    Stop,
}

const RATE: u32 = 44_100;
/// Quiet on purpose: loud enough to notice, well below speech.
const PEAK: f32 = 0.16;
const LOW_HZ: f32 = 880.0; // A5
const HIGH_HZ: f32 = 1318.5; // E6, a fifth above

/// How much of a recording to silence after the start cue: the cue (112 ms) plus
/// speaker latency. The microphone is already open when the cue plays, and on its own
/// a captured cue is transcribed as "Mm-hmm."; nobody starts speaking this quickly.
pub const START_CUE_MUTE_MS: u32 = 200;

/// Silence the start of a 16 kHz recording that began with the start cue.
pub fn mute_start_cue(pcm: &mut [f32]) {
    let n = (16_000 * START_CUE_MUTE_MS as usize / 1000).min(pcm.len());
    pcm[..n].fill(0.0);
}

/// The same muting for a recording that arrives in pieces (live text): silences the
/// next `remaining` samples and counts them down.
pub fn mute_start_cue_piece(pcm: &mut [f32], remaining: &mut usize) {
    let n = (*remaining).min(pcm.len());
    pcm[..n].fill(0.0);
    *remaining -= n;
}

/// Samples `mute_start_cue` silences.
pub const START_CUE_MUTE_SAMPLES: usize = 16_000 * START_CUE_MUTE_MS as usize / 1000;

/// Which sound, if any, goes with a pill change: start on entering Listening, stop on
/// leaving it (including a cancel or a too-short tap).
pub fn cue_for(prev: &PillState, next: &PillState) -> Option<Cue> {
    match (prev, next) {
        (PillState::Listening, PillState::Listening) => None,
        (_, PillState::Listening) => Some(Cue::Start),
        (PillState::Listening, _) => Some(Cue::Stop),
        _ => None,
    }
}

/// One soft note: sine plus a little second harmonic, a 4 ms raised-cosine attack and
/// an exponential decay that is forced to exactly zero over the last 5 ms.
fn note(out: &mut [f32], start: usize, len: usize, hz: f32) {
    let attack = RATE as usize * 4 / 1000;
    let tail = RATE as usize * 5 / 1000;
    for i in 0..len.min(out.len().saturating_sub(start)) {
        let t = i as f32 / RATE as f32;
        let mut env = (-t / 0.028).exp();
        if i < attack {
            env *= 0.5 - 0.5 * (std::f32::consts::PI * i as f32 / attack as f32).cos();
        }
        if i + tail > len {
            env *= (len - i) as f32 / tail as f32;
        }
        let phase = std::f32::consts::TAU * hz * t;
        out[start + i] += env * (phase.sin() + 0.25 * (2.0 * phase).sin());
    }
}

fn synth(cue: Cue) -> Vec<f32> {
    let ms = |v: usize| RATE as usize * v / 1000;
    let (first, second) = match cue {
        Cue::Start => (LOW_HZ, HIGH_HZ),
        Cue::Stop => (HIGH_HZ, LOW_HZ),
    };
    let mut s = vec![0f32; ms(112)];
    note(&mut s, 0, ms(56), first);
    note(&mut s, ms(46), ms(66), second);
    let peak = s.iter().fold(0f32, |m, v| m.max(v.abs()));
    s.iter_mut().for_each(|v| *v *= PEAK / peak);
    s
}

/// 16-bit mono PCM WAV bytes for a cue.
pub fn wav(cue: Cue) -> Vec<u8> {
    let pcm: Vec<u8> = synth(cue)
        .iter()
        .flat_map(|v| ((v * i16::MAX as f32).round() as i16).to_le_bytes())
        .collect();
    let mut w = Vec::with_capacity(44 + pcm.len());
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&RATE.to_le_bytes());
    w.extend_from_slice(&(RATE * 2).to_le_bytes()); // byte rate
    w.extend_from_slice(&2u16.to_le_bytes()); // block align
    w.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    w.extend_from_slice(&pcm);
    w
}

/// Play a cue without blocking. The WAV bytes live for the whole process, as
/// PlaySound's SND_MEMORY | SND_ASYNC requires.
#[cfg(windows)]
pub fn play(cue: Cue) {
    use std::sync::OnceLock;
    use windows::core::PCWSTR;
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
    static START: OnceLock<Vec<u8>> = OnceLock::new();
    static STOP: OnceLock<Vec<u8>> = OnceLock::new();
    let bytes = match cue {
        Cue::Start => START.get_or_init(|| wav(Cue::Start)),
        Cue::Stop => STOP.get_or_init(|| wav(Cue::Stop)),
    };
    // SAFETY: with SND_MEMORY the "name" is a pointer to a complete WAV image, which is
    // 'static here.
    unsafe {
        let _ = PlaySoundW(
            PCWSTR(bytes.as_ptr() as *const u16),
            None,
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_pieces_are_muted_exactly_like_the_whole_recording() {
        let whole_len = 16_000;
        let mut whole = vec![0.5f32; whole_len];
        mute_start_cue(&mut whole);
        let mut remaining = START_CUE_MUTE_SAMPLES;
        let mut pieces = Vec::new();
        for n in [1_000usize, 1_500, 700, 12_800] {
            let mut p = vec![0.5f32; n];
            mute_start_cue_piece(&mut p, &mut remaining);
            pieces.extend(p);
        }
        assert_eq!(pieces, whole);
        assert_eq!(remaining, 0);
    }

    fn samples(wav: &[u8]) -> Vec<f32> {
        wav[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b) as f32 / i16::MAX as f32)
            .collect()
    }

    /// Zero crossings per second over a slice: a rough pitch.
    fn pitch(s: &[f32]) -> f32 {
        let crossings = s
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        crossings as f32 / 2.0 / (s.len() as f32 / RATE as f32)
    }

    #[test]
    fn cues_are_short_quiet_wav_files() {
        for cue in [Cue::Start, Cue::Stop] {
            let wav = wav(cue);
            assert_eq!(&wav[0..4], b"RIFF");
            assert_eq!(&wav[8..16], b"WAVEfmt ");
            assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), RATE);
            let s = samples(&wav);
            let ms = s.len() as u32 * 1000 / RATE;
            assert!((80..=150).contains(&ms), "{cue:?}: {ms} ms");
            let peak = s.iter().fold(0f32, |m, v| m.max(v.abs()));
            assert!(peak > 0.05 && peak <= 0.2, "{cue:?}: peak {peak}");
        }
    }

    #[test]
    fn cues_fade_in_and_out_so_they_never_click() {
        for cue in [Cue::Start, Cue::Stop] {
            let s = samples(&wav(cue));
            assert!(
                s[0].abs() < 0.001 && s[s.len() - 1].abs() < 0.001,
                "{cue:?}"
            );
            let edge = RATE as usize / 1000; // first and last millisecond
            assert!(s[..edge].iter().all(|v| v.abs() < 0.03), "{cue:?} attack");
            assert!(
                s[s.len() - edge..].iter().all(|v| v.abs() < 0.03),
                "{cue:?} tail"
            );
        }
    }

    #[test]
    fn start_rises_and_stop_falls() {
        let start = samples(&wav(Cue::Start));
        let stop = samples(&wav(Cue::Stop));
        let (a, b) = start.split_at(start.len() / 2);
        assert!(
            pitch(b) > pitch(a) * 1.2,
            "start: {} -> {}",
            pitch(a),
            pitch(b)
        );
        let (a, b) = stop.split_at(stop.len() / 2);
        assert!(
            pitch(b) < pitch(a) / 1.2,
            "stop: {} -> {}",
            pitch(a),
            pitch(b)
        );
    }

    #[test]
    fn the_start_cue_is_muted_out_of_the_recording() {
        let mut pcm = vec![0.5f32; 16_000];
        mute_start_cue(&mut pcm);
        let muted = 16_000 * START_CUE_MUTE_MS as usize / 1000;
        assert!(pcm[..muted].iter().all(|&v| v == 0.0));
        assert!(
            pcm[muted..].iter().all(|&v| v == 0.5),
            "speech after it is untouched"
        );
        // A recording shorter than the cue is simply silenced.
        let mut short = vec![0.5f32; 100];
        mute_start_cue(&mut short);
        assert!(short.iter().all(|&v| v == 0.0));
        // The cue itself must fit inside the muted span, with room for speaker delay.
        let cue_ms = (wav(Cue::Start).len() - 44) as u32 / 2 * 1000 / RATE;
        assert!(START_CUE_MUTE_MS >= cue_ms + 60, "cue {cue_ms} ms");
    }

    /// Writes the cues to $AURAL_SOUNDS_OUT (start.wav, stop.wav) to listen to them or
    /// to check what the recognizer makes of them. Run with `-- --ignored`.
    #[test]
    #[ignore]
    fn write_cue_files() {
        let dir = std::path::PathBuf::from(std::env::var("AURAL_SOUNDS_OUT").unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("start.wav"), wav(Cue::Start)).unwrap();
        std::fs::write(dir.join("stop.wav"), wav(Cue::Stop)).unwrap();
    }

    #[test]
    fn start_plays_on_entering_listening_and_stop_on_leaving_it() {
        use PillState::*;
        assert_eq!(cue_for(&Hidden, &Listening), Some(Cue::Start));
        assert_eq!(cue_for(&Listening, &Processing), Some(Cue::Stop));
        assert_eq!(
            cue_for(&Listening, &Hidden),
            Some(Cue::Stop),
            "cancel or tap"
        );
        assert_eq!(cue_for(&Listening, &Listening), None);
        assert_eq!(cue_for(&Processing, &Success), None);
        assert_eq!(cue_for(&Success, &Hidden), None);
    }
}
