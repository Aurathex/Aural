//! Owns the speech worker process for the active model. Loading happens on a
//! background thread; status is kept separately so the UI never waits on a load.

use aural_core::error::ErrorCode;
use aural_engines::{Backend, Engine};
use aural_models::{ModelEntry, ModelStore};
use aural_stt_protocol::client::{ClientError, SttClient, WorkerSpec};
use serde::Serialize;
use std::path::PathBuf;
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
    status: Mutex<EngineStatus>,
}

impl Default for EngineHost {
    fn default() -> Self {
        Self {
            client: Mutex::new(None),
            status: Mutex::new(EngineStatus::NoModel),
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

    /// Stop the worker and free the model's memory.
    pub fn unload(&self) {
        if let Ok(mut c) = self.client.lock() {
            *c = None;
        }
        self.set_status(EngineStatus::NoModel);
    }

    /// Start a fresh worker for `entry` and load it (blocking; call off the UI thread).
    pub fn load(&self, entry: &ModelEntry, store: &ModelStore) {
        self.set_status(EngineStatus::Loading {
            model: entry.id.clone(),
        });
        let mut guard = match self.client.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        *guard = None; // stop the previous worker first
        let model_path = match entry.engine {
            Engine::Parakeet => store.dir(&entry.id),
            Engine::Whisper => store.dir(&entry.id).join(&entry.files[0].name),
        };
        let result = SttClient::spawn(WorkerSpec {
            exe: worker_exe(entry.engine),
            args: vec![],
        })
        .and_then(|mut c| {
            c.load(
                model_path,
                entry.engine,
                Backend::Cpu,
                threads(),
                Duration::from_secs(180),
            )?;
            Ok(c)
        });
        match result {
            Ok(client) => {
                self.set_status(EngineStatus::Ready {
                    model: entry.id.clone(),
                    label: client.label().unwrap_or(&entry.name).to_owned(),
                    backend: client.backend().unwrap_or("cpu").to_owned(),
                });
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
            ClientError::NotLoaded => ErrorCode::NoModel,
            _ => ErrorCode::EngineFailed,
        })
    }
}
