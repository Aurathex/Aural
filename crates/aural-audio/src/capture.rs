//! Microphone capture via WASAPI (cpal). The stream opens when dictation starts and
//! closes when it stops, so the Windows microphone indicator is only on while the user
//! is actually dictating.

use crate::dsp::resample_mono;
use crate::levels::{LevelAnalyzer, BANDS, WINDOW};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::Serialize;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InputDevice {
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct LevelFrame {
    pub bands: [f32; BANDS],
}

pub type LevelCallback = Box<dyn Fn(LevelFrame) + Send>;

#[derive(Debug)]
pub enum CaptureError {
    DeviceNotFound(String),
    NoDefaultDevice,
    PermissionDenied,
    Unavailable(String),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureError::DeviceNotFound(n) => write!(f, "microphone {n:?} is not connected"),
            CaptureError::NoDefaultDevice => write!(f, "no microphone is available"),
            CaptureError::PermissionDenied => write!(f, "Windows is blocking microphone access"),
            CaptureError::Unavailable(e) => write!(f, "microphone unavailable: {e}"),
        }
    }
}

impl std::error::Error for CaptureError {}

fn device_name(d: &cpal::Device) -> Option<String> {
    d.description().ok().map(|desc| desc.name().to_owned())
}

pub fn input_devices() -> Result<Vec<InputDevice>, CaptureError> {
    let host = cpal::default_host();
    let default = host.default_input_device().and_then(|d| device_name(&d));
    let devices = host
        .input_devices()
        .map_err(|e| CaptureError::Unavailable(e.to_string()))?;
    let mut out: Vec<InputDevice> = devices
        .filter_map(|d| device_name(&d))
        .map(|name| InputDevice {
            is_default: Some(&name) == default.as_ref(),
            name,
        })
        .collect();
    out.dedup_by(|a, b| a.name == b.name);
    Ok(out)
}

fn find_device(name: Option<&str>) -> Result<cpal::Device, CaptureError> {
    let host = cpal::default_host();
    match name {
        None => host
            .default_input_device()
            .ok_or(CaptureError::NoDefaultDevice),
        Some(wanted) => host
            .input_devices()
            .map_err(|e| CaptureError::Unavailable(e.to_string()))?
            .find(|d| device_name(d).as_deref() == Some(wanted))
            .ok_or_else(|| CaptureError::DeviceNotFound(wanted.to_owned())),
    }
}

fn map_err(e: cpal::Error) -> CaptureError {
    match e.kind() {
        cpal::ErrorKind::PermissionDenied => CaptureError::PermissionDenied,
        _ => CaptureError::Unavailable(e.to_string()),
    }
}

/// A playing stream, the buffer it fills (mono, native rate) and that rate.
type OpenStream = (cpal::Stream, Arc<Mutex<Vec<f32>>>, u32);

pub struct CaptureHandle {
    stop_tx: mpsc::Sender<()>,
    thread: std::thread::JoinHandle<Vec<f32>>,
}

impl CaptureHandle {
    /// Stop recording and return the audio as 16 kHz mono.
    pub fn stop(self) -> Vec<f32> {
        let _ = self.stop_tx.send(());
        self.thread.join().unwrap_or_default()
    }
}

/// Open the microphone (`None` = Windows default) and start recording. Level frames
/// are delivered about 30 times a second until `stop`.
pub fn start(
    device: Option<&str>,
    on_levels: LevelCallback,
) -> Result<CaptureHandle, CaptureError> {
    let device = device.map(str::to_owned);
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), CaptureError>>();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let thread = std::thread::Builder::new()
        .name("aural-capture".into())
        .spawn(move || {
            let run = || -> Result<OpenStream, CaptureError> {
                let dev = find_device(device.as_deref())?;
                let supported = dev.default_input_config().map_err(map_err)?;
                let config = supported.config();
                let channels = config.channels as usize;
                let buf = Arc::new(Mutex::new(Vec::<f32>::with_capacity(48_000 * 30)));
                let sink = buf.clone();
                let push = move |mono: &mut dyn Iterator<Item = f32>| {
                    if let Ok(mut b) = sink.lock() {
                        b.extend(mono);
                    }
                };
                let err_cb = |_e: cpal::Error| {};
                let timeout = Some(Duration::from_secs(3));
                let stream = match supported.sample_format() {
                    cpal::SampleFormat::F32 => dev.build_input_stream::<f32, _, _>(
                        config.clone(),
                        move |data, _| {
                            push(
                                &mut data
                                    .chunks(channels)
                                    .map(|f| f.iter().sum::<f32>() / channels as f32),
                            )
                        },
                        err_cb,
                        timeout,
                    ),
                    cpal::SampleFormat::I16 => dev.build_input_stream::<i16, _, _>(
                        config.clone(),
                        move |data, _| {
                            push(&mut data.chunks(channels).map(|f| {
                                f.iter().map(|s| *s as f32 / 32768.0).sum::<f32>() / channels as f32
                            }))
                        },
                        err_cb,
                        timeout,
                    ),
                    other => {
                        return Err(CaptureError::Unavailable(format!(
                            "unsupported sample format {other:?}"
                        )))
                    }
                }
                .map_err(map_err)?;
                stream.play().map_err(map_err)?;
                Ok((stream, buf, config.sample_rate))
            };
            let (stream, buf, rate) = match run() {
                Ok(v) => {
                    let _ = ready_tx.send(Ok(()));
                    v
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return Vec::new();
                }
            };
            let mut analyzer = LevelAnalyzer::new(rate);
            let mut window = vec![0.0f32; WINDOW];
            while let Err(mpsc::RecvTimeoutError::Timeout) =
                stop_rx.recv_timeout(Duration::from_millis(33))
            {
                if let Ok(b) = buf.lock() {
                    let tail = &b[b.len().saturating_sub(WINDOW)..];
                    window.clear();
                    window.extend_from_slice(tail);
                }
                on_levels(LevelFrame {
                    bands: analyzer.analyze(&window),
                });
            }
            drop(stream);
            let native = std::mem::take(&mut *buf.lock().unwrap_or_else(|p| p.into_inner()));
            resample_mono(&native, rate).unwrap_or_default()
        })
        .map_err(|e| CaptureError::Unavailable(e.to_string()))?;

    match ready_rx.recv() {
        Ok(Ok(())) => Ok(CaptureHandle { stop_tx, thread }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err(CaptureError::Unavailable("capture thread exited".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_selection_by_missing_name_is_an_error() {
        let err = start(Some("No Such Microphone 9f3a"), Box::new(|_| {}))
            .err()
            .unwrap();
        assert!(matches!(err, CaptureError::DeviceNotFound(_)), "{err}");
    }

    /// Uses the real default microphone for ~0.5 s. Run with `-- --ignored`.
    #[test]
    #[ignore]
    fn records_from_the_default_microphone_at_16k() {
        let frames = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let f = frames.clone();
        let cap = start(
            None,
            Box::new(move |_| {
                f.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let pcm = cap.stop();
        assert!(pcm.len() > 16_000 / 4, "only {} samples", pcm.len());
        assert!(
            frames.load(std::sync::atomic::Ordering::Relaxed) >= 5,
            "level frames not delivered"
        );
    }

    #[test]
    #[ignore]
    fn lists_input_devices() {
        let devices = input_devices().unwrap();
        assert!(!devices.is_empty());
        assert!(devices.iter().filter(|d| d.is_default).count() <= 1);
    }
}
