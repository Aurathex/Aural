//! Owns the speech worker process for the active model. Loading happens on a
//! background thread; status is kept separately so the UI never waits on a load.

use aural_core::error::ErrorCode;
use aural_engines::{Backend, Engine};
use aural_models::{ModelEntry, ModelStore};
use aural_stt_protocol::client::{ClientError, SttClient, WorkerKiller, WorkerSpec};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum EngineStatus {
    NoModel,
    Loading {
        model: String,
    },
    Ready {
        model: String,
        label: String,
        backend: String,
    },
    Error {
        model: String,
        message: String,
    },
}

pub struct EngineHost {
    client: Mutex<Option<SttClient>>,
    /// Stops the current worker without waiting for `client` (held during a
    /// transcription); lives outside that lock on purpose.
    killer: Mutex<Option<WorkerKiller>>,
    status: Mutex<EngineStatus>,
    /// Bumped by every load/unload so a slow, superseded load can't install itself.
    generation: AtomicU64,
}

impl Default for EngineHost {
    fn default() -> Self {
        Self {
            client: Mutex::new(None),
            killer: Mutex::new(None),
            status: Mutex::new(EngineStatus::NoModel),
            generation: AtomicU64::new(0),
        }
    }
}

/// Worker executables are installed next to aural.exe (Tauri sidecars).
fn worker_exe(engine: Engine) -> PathBuf {
    let name = match engine {
        Engine::Parakeet => "aural-stt-onnx.exe",
        Engine::Whisper => "aural-stt-ggml.exe",
    };
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

fn threads() -> usize {
    let logical = std::thread::available_parallelism().map_or(4, |n| n.get());
    (logical / 2).saturating_sub(1).clamp(1, 8)
}

impl EngineHost {
    pub fn status(&self) -> EngineStatus {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or(EngineStatus::NoModel)
    }

    fn set_status(&self, s: EngineStatus) {
        if let Ok(mut g) = self.status.lock() {
            *g = s;
        }
    }

    pub fn mark_loading(&self, model: &str) {
        self.set_status(EngineStatus::Loading {
            model: model.to_owned(),
        });
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.status(), EngineStatus::Ready { .. })
    }

    /// Kill the current worker immediately; a transcription in progress ends with an
    /// error instead of being waited for.
    fn stop_current(&self) {
        if let Some(k) = self.killer.lock().ok().and_then(|mut g| g.take()) {
            k.kill();
        }
    }

    /// Stop the worker and free the model's memory. Never blocks on a transcription
    /// or a load in progress.
    pub fn unload(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.stop_current();
        if let Ok(mut c) = self.client.try_lock() {
            *c = None;
        }
        self.set_status(EngineStatus::NoModel);
    }

    /// Start a fresh worker for `entry` on `backend` and load it (blocking; call off the
    /// UI thread). The client lock is only taken briefly to swap workers, never during
    /// the load.
    pub fn load(&self, entry: &ModelEntry, backend: Backend, store: &ModelStore) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.stop_current();
        self.set_status(EngineStatus::Loading {
            model: entry.id.clone(),
        });
        let model_path = match entry.engine {
            Engine::Parakeet => store.dir(&entry.id),
            Engine::Whisper => store.dir(&entry.id).join(&entry.files[0].name),
        };
        let spawned = SttClient::spawn(WorkerSpec {
            exe: worker_exe(entry.engine),
            args: vec![],
        });
        let result = spawned.and_then(|mut c| {
            if let Ok(mut k) = self.killer.lock() {
                *k = Some(c.killer());
            }
            c.load(
                model_path,
                entry.engine,
                backend,
                threads(),
                Duration::from_secs(180),
            )?;
            Ok(c)
        });
        if self.generation.load(Ordering::SeqCst) != generation {
            return; // superseded by a newer load or an unload; drop this worker
        }
        match result {
            Ok(client) => {
                self.set_status(EngineStatus::Ready {
                    model: entry.id.clone(),
                    label: client.label().unwrap_or(&entry.name).to_owned(),
                    backend: client.backend().unwrap_or("cpu").to_owned(),
                });
                let mut guard = self.client.lock().unwrap_or_else(|p| p.into_inner());
                *guard = Some(client);
            }
            Err(e) => self.set_status(EngineStatus::Error {
                model: entry.id.clone(),
                message: e.to_string(),
            }),
        }
    }

    pub fn transcribe(&self, pcm: &[f32]) -> Result<String, ErrorCode> {
        let mut guard = self.client.lock().map_err(|_| ErrorCode::EngineFailed)?;
        let client = guard.as_mut().ok_or(ErrorCode::NoModel)?;
        let seconds = pcm.len() as u64 / 16_000;
        let timeout = Duration::from_secs(30 + 2 * seconds);
        client.transcribe(pcm, timeout).map_err(|e| match e {
            ClientError::NotLoaded | ClientError::Stopped => ErrorCode::NoModel,
            _ => ErrorCode::EngineFailed,
        })
    }
}
