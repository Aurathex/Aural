//! The worker side: announce, load a model on request, transcribe PCM frames.

use crate::codec::{read_msg, read_pcm, write_msg};
use crate::msg::{Request, Response};
use crate::PROTOCOL_VERSION;
use anyhow::Result;
use aural_engines::live::LiveSession;
use aural_engines::{Backend, Engine, Transcriber};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Instant;

pub type Builder<'a> =
    dyn FnMut(&Path, Engine, Backend, usize) -> Result<Box<dyn Transcriber>> + 'a;

pub fn serve<R: Read, W: Write>(
    mut input: R,
    output: &mut W,
    engines: Vec<Engine>,
    mut build: impl FnMut(&Path, Engine, Backend, usize) -> Result<Box<dyn Transcriber>>,
) -> Result<()> {
    write_msg(
        output,
        &Response::Ready {
            protocol: PROTOCOL_VERSION,
            engines,
        },
    )?;
    let mut engine: Option<Box<dyn Transcriber>> = None;
    let mut live: Option<(u64, LiveSession)> = None;
    while let Some(req) = read_msg::<_, Request>(&mut input)? {
        match req {
            Request::Load {
                model,
                engine: kind,
                backend,
                threads,
            } => {
                let t0 = Instant::now();
                live = None;
                engine = None; // free the previous model first
                match build(&model, kind, backend, threads) {
                    Ok(e) => {
                        write_msg(
                            output,
                            &Response::Loaded {
                                label: e.label(),
                                backend: e.backend_used(),
                                ms: t0.elapsed().as_millis() as u64,
                            },
                        )?;
                        engine = Some(e);
                    }
                    Err(e) => write_msg(
                        output,
                        &Response::Error {
                            id: None,
                            message: format!("{e:#}"),
                        },
                    )?,
                }
            }
            Request::Transcribe { id, samples } => {
                let pcm = read_pcm(&mut input, samples as usize)?;
                let t0 = Instant::now();
                let resp = match engine.as_mut() {
                    None => Response::Error {
                        id: Some(id),
                        message: "no model is loaded".into(),
                    },
                    Some(e) => match e.transcribe(&pcm) {
                        Ok(text) => Response::Transcript {
                            id,
                            text,
                            ms: t0.elapsed().as_millis() as u64,
                        },
                        Err(err) => Response::Error {
                            id: Some(id),
                            message: format!("{err:#}"),
                        },
                    },
                };
                write_msg(output, &resp)?;
            }
            Request::LiveBegin { id } => {
                live = None;
                let resp = match engine.as_mut() {
                    None => no_model(id),
                    Some(e) => match LiveSession::begin(e.as_mut()) {
                        Ok(s) => {
                            let mode = s.mode();
                            live = Some((id, s));
                            Response::LiveStarted { id, mode }
                        }
                        Err(err) => engine_error(id, err),
                    },
                };
                write_msg(output, &resp)?;
            }
            Request::LiveAudio { id, samples } => {
                let pcm = read_pcm(&mut input, samples as usize)?;
                let t0 = Instant::now();
                let resp = match (engine.as_mut(), live.as_mut()) {
                    (Some(e), Some((open, s))) if *open == id => match s.push(e.as_mut(), &pcm) {
                        Ok(text) => Response::LiveText {
                            id,
                            stable: text.stable,
                            tentative: text.tentative,
                            ms: t0.elapsed().as_millis() as u64,
                        },
                        Err(err) => {
                            live = None; // a failed stream is over; the app falls back
                            engine_error(id, err)
                        }
                    },
                    _ => no_stream(id),
                };
                write_msg(output, &resp)?;
            }
            Request::LiveEnd { id, samples } => {
                let pcm = read_pcm(&mut input, samples as usize)?;
                let t0 = Instant::now();
                let resp = match (engine.as_mut(), live.take()) {
                    (Some(e), Some((open, mut s))) if open == id => {
                        match s.push(e.as_mut(), &pcm).and_then(|_| s.finish(e.as_mut())) {
                            Ok(text) => Response::Transcript {
                                id,
                                text,
                                ms: t0.elapsed().as_millis() as u64,
                            },
                            Err(err) => engine_error(id, err),
                        }
                    }
                    (_, other) => {
                        live = other;
                        no_stream(id)
                    }
                };
                write_msg(output, &resp)?;
            }
            Request::LiveCancel { id } => {
                if let (Some(e), Some((open, s))) = (engine.as_mut(), live.take()) {
                    if open == id {
                        s.cancel(e.as_mut());
                    } else {
                        live = Some((open, s));
                    }
                }
            }
            Request::Stats => {
                let (working_set_mb, peak_working_set_mb) = process_memory_mb();
                write_msg(
                    output,
                    &Response::Stats {
                        working_set_mb,
                        peak_working_set_mb,
                        gpu_memory_mb: gpu_memory_mb(),
                    },
                )?;
            }
            Request::Shutdown => return Ok(()),
        }
    }
    Ok(())
}

fn no_model(id: u64) -> Response {
    Response::Error {
        id: Some(id),
        message: "no model is loaded".into(),
    }
}

fn no_stream(id: u64) -> Response {
    Response::Error {
        id: Some(id),
        message: "no live stream is open".into(),
    }
}

fn engine_error(id: u64, err: anyhow::Error) -> Response {
    Response::Error {
        id: Some(id),
        message: format!("{err:#}"),
    }
}

/// Current and peak working set of this process, in MB.
#[cfg(windows)]
fn process_memory_mb() -> (u64, u64) {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut c: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: the pseudo-handle for this process and a correctly sized struct.
    let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) } != 0;
    if !ok {
        return (0, 0);
    }
    let mb = |b: usize| (b / (1024 * 1024)) as u64;
    (mb(c.WorkingSetSize), mb(c.PeakWorkingSetSize))
}

#[cfg(windows)]
fn gpu_memory_mb() -> Option<u64> {
    aural_platform::gpu::process_gpu_memory_mb()
}

/// Entry point for worker binaries: protocol on stdin/stdout.
pub fn serve_stdio(
    engines: Vec<Engine>,
    build: impl FnMut(&Path, Engine, Backend, usize) -> Result<Box<dyn Transcriber>>,
) -> Result<()> {
    let stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout().lock();
    serve(stdin, &mut stdout, engines, build)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{read_msg, write_msg, write_pcm};
    use crate::msg::{Request, Response};
    use aural_engines::{Backend, Engine, Transcriber};
    use std::io::Cursor;

    struct Counter;
    impl Transcriber for Counter {
        fn label(&self) -> String {
            "counter".into()
        }
        fn transcribe(&mut self, pcm: &[f32]) -> anyhow::Result<String> {
            Ok(format!("{} samples", pcm.len()))
        }
    }

    fn run(requests: Vec<(Request, Option<Vec<f32>>)>, fail_load: bool) -> Vec<Response> {
        let mut input = Vec::new();
        for (r, pcm) in requests {
            write_msg(&mut input, &r).unwrap();
            if let Some(p) = pcm {
                write_pcm(&mut input, &p).unwrap();
            }
        }
        let mut output = Vec::new();
        serve(
            Cursor::new(input),
            &mut output,
            vec![Engine::Parakeet],
            |_, _, _, _| {
                if fail_load {
                    anyhow::bail!("model files missing")
                }
                Ok(Box::new(Counter) as Box<dyn Transcriber>)
            },
        )
        .unwrap();
        let mut out = Vec::new();
        let mut c = Cursor::new(output);
        while let Some(m) = read_msg::<_, Response>(&mut c).unwrap() {
            out.push(m);
        }
        out
    }

    fn load() -> Request {
        Request::Load {
            model: "m".into(),
            engine: Engine::Parakeet,
            backend: Backend::Cpu,
            threads: 2,
        }
    }

    #[test]
    fn announces_then_loads_then_transcribes() {
        let out = run(
            vec![
                (load(), None),
                (
                    Request::Transcribe { id: 3, samples: 4 },
                    Some(vec![0.0; 4]),
                ),
                (Request::Shutdown, None),
            ],
            false,
        );
        assert!(matches!(
            out[0],
            Response::Ready {
                protocol: crate::PROTOCOL_VERSION,
                ..
            }
        ));
        assert!(matches!(&out[1], Response::Loaded { label, .. } if label == "counter"));
        assert!(matches!(&out[2], Response::Transcript { id: 3, text, .. } if text == "4 samples"));
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn transcribe_before_load_is_an_error_not_a_crash() {
        let out = run(
            vec![(
                Request::Transcribe { id: 1, samples: 2 },
                Some(vec![0.0; 2]),
            )],
            false,
        );
        assert!(matches!(&out[1], Response::Error { id: Some(1), .. }));
    }

    #[test]
    fn load_failure_is_reported() {
        let out = run(vec![(load(), None)], true);
        assert!(
            matches!(&out[1], Response::Error { id: None, message } if message.contains("missing"))
        );
    }

    fn loud(n: usize) -> Vec<f32> {
        (0..n).map(|i| (i as f32 * 0.07).sin() * 0.3).collect()
    }

    #[test]
    fn live_stream_gives_text_while_audio_arrives_then_one_final_transcript() {
        let out = run(
            vec![
                (load(), None),
                (Request::LiveBegin { id: 7 }, None),
                (
                    Request::LiveAudio {
                        id: 7,
                        samples: 8_000,
                    },
                    Some(loud(8_000)),
                ),
                (
                    Request::LiveEnd {
                        id: 7,
                        samples: 1_600,
                    },
                    Some(loud(1_600)),
                ),
            ],
            false,
        );
        assert!(matches!(
            &out[2],
            Response::LiveStarted {
                id: 7,
                mode: aural_engines::live::LiveMode::Phrases
            }
        ));
        assert!(
            matches!(&out[3], Response::LiveText { id: 7, tentative, .. } if tentative.contains("samples")),
            "{:?}",
            out[3]
        );
        // The final text covers all the audio, read once.
        assert!(
            matches!(&out[4], Response::Transcript { id: 7, text, .. } if text.ends_with("samples")),
            "{:?}",
            out[4]
        );
        assert_eq!(out.len(), 5);
    }

    #[test]
    fn live_audio_without_an_open_stream_is_an_error_not_a_crash() {
        let out = run(
            vec![
                (load(), None),
                (Request::LiveAudio { id: 1, samples: 4 }, Some(vec![0.0; 4])),
                (Request::LiveBegin { id: 2 }, None),
                (Request::LiveEnd { id: 3, samples: 4 }, Some(vec![0.0; 4])),
                // The stream opened as 2 is still usable after the stray end.
                (Request::LiveEnd { id: 2, samples: 4 }, Some(vec![0.0; 4])),
            ],
            false,
        );
        assert!(matches!(&out[2], Response::Error { id: Some(1), .. }));
        assert!(matches!(&out[4], Response::Error { id: Some(3), .. }));
        assert!(matches!(&out[5], Response::Transcript { id: 2, .. }));
    }

    #[test]
    fn a_cancelled_stream_is_closed() {
        let out = run(
            vec![
                (load(), None),
                (Request::LiveBegin { id: 4 }, None),
                (Request::LiveCancel { id: 4 }, None),
                (Request::LiveEnd { id: 4, samples: 4 }, Some(vec![0.0; 4])),
            ],
            false,
        );
        assert!(matches!(&out[3], Response::Error { id: Some(4), .. }));
        assert_eq!(out.len(), 4, "cancel itself has no reply");
    }

    #[test]
    fn stops_at_end_of_input() {
        let out = run(vec![(load(), None)], false);
        assert_eq!(out.len(), 2);
    }
}
