//! Framing: `b"AUR1"` magic, u32 little-endian length, payload. JSON frames carry
//! messages; PCM frames carry f32 little-endian samples. The magic makes stray output
//! from a native library on stdout a detected error instead of a corrupt transcript.

use anyhow::{bail, Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::{ErrorKind, Read, Write};

pub const MAGIC: &[u8; 4] = b"AUR1";
/// 64 MiB: five minutes of 16 kHz f32 audio is ~19 MiB.
pub const MAX_FRAME: u32 = 64 * 1024 * 1024;

fn write_frame<W: Write>(w: &mut W, payload: &[u8]) -> Result<()> {
    if payload.len() > MAX_FRAME as usize {
        bail!("frame of {} bytes exceeds the limit", payload.len());
    }
    w.write_all(MAGIC)?;
    w.write_all(&(payload.len() as u32).to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()?;
    Ok(())
}

/// `Ok(None)` on a clean end of stream (between frames).
fn read_frame<R: Read>(r: &mut R) -> Result<Option<Vec<u8>>> {
    let mut magic = [0u8; 4];
    let mut got = 0;
    while got < 4 {
        match r.read(&mut magic[got..]) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => bail!("stream ended inside a frame header"),
            Ok(n) => got += n,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    if &magic != MAGIC {
        bail!("unexpected bytes on the worker channel (a library wrote to stdout?)");
    }
    let mut len = [0u8; 4];
    r.read_exact(&mut len).context("reading frame length")?;
    let len = u32::from_le_bytes(len);
    if len > MAX_FRAME {
        bail!("frame of {len} bytes exceeds the limit");
    }
    let mut payload = vec![0u8; len as usize];
    r.read_exact(&mut payload)
        .context("reading frame payload")?;
    Ok(Some(payload))
}

pub fn write_msg<W: Write, T: Serialize>(w: &mut W, msg: &T) -> Result<()> {
    write_frame(w, &serde_json::to_vec(msg)?)
}

pub fn read_msg<R: Read, T: DeserializeOwned>(r: &mut R) -> Result<Option<T>> {
    match read_frame(r)? {
        None => Ok(None),
        Some(p) => Ok(Some(
            serde_json::from_slice(&p).context("decoding message")?,
        )),
    }
}

pub fn write_pcm<W: Write>(w: &mut W, pcm: &[f32]) -> Result<()> {
    let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
    write_frame(w, &bytes)
}

pub fn read_pcm<R: Read>(r: &mut R, samples: usize) -> Result<Vec<f32>> {
    let p = read_frame(r)?.context("stream ended before the audio frame")?;
    if p.len() != samples * 4 {
        bail!(
            "audio frame has {} bytes, expected {}",
            p.len(),
            samples * 4
        );
    }
    Ok(p.as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::{Request, Response};
    use aural_engines::{Backend, Engine};
    use std::io::Cursor;

    fn roundtrip<
        T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    >(
        m: T,
    ) {
        let mut buf = Vec::new();
        write_msg(&mut buf, &m).unwrap();
        let back: T = read_msg(&mut Cursor::new(buf)).unwrap().unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn codec_roundtrip_all_messages() {
        roundtrip(Request::Load {
            model: "C:/m".into(),
            engine: Engine::Parakeet,
            backend: Backend::Cpu,
            threads: 4,
        });
        roundtrip(Request::Transcribe {
            id: 7,
            samples: 16_000,
        });
        roundtrip(Request::Shutdown);
        roundtrip(Response::Ready {
            protocol: 1,
            engines: vec![Engine::Whisper],
        });
        roundtrip(Response::Loaded {
            label: "x".into(),
            backend: "cpu".into(),
            ms: 5,
        });
        roundtrip(Response::Transcript {
            id: 7,
            text: "héllo 👋".into(),
            ms: 9,
        });
        roundtrip(Response::Error {
            id: None,
            message: "boom".into(),
        });
    }

    #[test]
    fn pcm_roundtrip_is_bit_exact() {
        let pcm = vec![0.0f32, -1.0, 0.5, f32::MIN_POSITIVE, 0.123_456_79];
        let mut buf = Vec::new();
        write_pcm(&mut buf, &pcm).unwrap();
        assert_eq!(read_pcm(&mut Cursor::new(buf), pcm.len()).unwrap(), pcm);
    }

    #[test]
    fn clean_eof_is_none() {
        let r: Option<Request> = read_msg(&mut Cursor::new(Vec::new())).unwrap();
        assert!(r.is_none());
    }

    #[test]
    fn stray_output_is_rejected() {
        let mut data = b"hello from a C library\n".to_vec();
        write_msg(&mut data, &Request::Shutdown).unwrap();
        assert!(read_msg::<_, Request>(&mut Cursor::new(data)).is_err());
    }

    #[test]
    fn oversized_frame_is_rejected_without_allocating() {
        let mut data = MAGIC.to_vec();
        data.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_msg::<_, Request>(&mut Cursor::new(data)).is_err());
    }

    #[test]
    fn pcm_length_mismatch_is_an_error() {
        let mut buf = Vec::new();
        write_pcm(&mut buf, &[1.0, 2.0]).unwrap();
        assert!(read_pcm(&mut Cursor::new(buf), 3).is_err());
    }

    #[test]
    fn truncated_frame_is_an_error() {
        let mut buf = Vec::new();
        write_msg(&mut buf, &Request::Shutdown).unwrap();
        buf.truncate(buf.len() - 2);
        assert!(read_msg::<_, Request>(&mut Cursor::new(buf)).is_err());
    }
}
