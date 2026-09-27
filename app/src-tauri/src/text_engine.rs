//! The text model for AI cleanup, in its own ONNX worker process (never the speech
//! worker, so neither model's memory or a crash affects the other). Loaded only while AI
//! cleanup is in use and a text model is downloaded.

use aural_stt_protocol::client::{SttClient, WorkerKiller, WorkerSpec};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TextStatus {
    Off,
    Loading { model: String },
    Ready { model: String, label: String },
    Error { model: String, message: String },
}

pub struct TextHost {
    client: Mutex<Option<SttClient>>,
    killer: Mutex<Option<WorkerKiller>>,
    status: Mutex<TextStatus>,
    generation: AtomicU64,
}

impl Default for TextHost {
    fn default() -> Self {
        Self {
            client: Mutex::new(None),
            killer: Mutex::new(None),
            status: Mutex::new(TextStatus::Off),
            generation: AtomicU64::new(0),
        }
    }
}

/// How long a rewrite may take before the light result is used instead.
pub const REWRITE_TIMEOUT: Duration = Duration::from_secs(12);

impl TextHost {
    pub fn status(&self) -> TextStatus {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or(TextStatus::Off)
    }

    fn set(&self, s: TextStatus) {
        if let Ok(mut g) = self.status.lock() {
            *g = s;
        }
    }

    pub fn loaded_model(&self) -> Option<String> {
        match self.status() {
            TextStatus::Ready { model, .. } | TextStatus::Loading { model } => Some(model),
            _ => None,
        }
    }

    pub fn unload(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(k) = self.killer.lock().ok().and_then(|mut g| g.take()) {
            k.kill();
        }
        if let Ok(mut c) = self.client.try_lock() {
            *c = None;
        }
        self.set(TextStatus::Off);
    }

    /// Start a worker and load `dir` (blocking; call off the UI thread).
    pub fn load(&self, model: &str, dir: PathBuf) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(k) = self.killer.lock().ok().and_then(|mut g| g.take()) {
            k.kill();
        }
        self.set(TextStatus::Loading {
            model: model.to_owned(),
        });
        let result = SttClient::spawn(WorkerSpec {
            exe: crate::engine::worker_exe(aural_engines::Engine::Parakeet),
            args: vec![],
        })
        .and_then(|mut c| {
            if let Ok(mut k) = self.killer.lock() {
                *k = Some(c.killer());
            }
            let label = c.load_text(dir, crate::engine::threads(), Duration::from_secs(180))?;
            Ok((c, label))
        });
        if self.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        match result {
            Ok((client, label)) => {
                self.set(TextStatus::Ready {
                    model: model.to_owned(),
                    label,
                });
                *self.client.lock().unwrap_or_else(|p| p.into_inner()) = Some(client);
            }
            Err(e) => self.set(TextStatus::Error {
                model: model.to_owned(),
                message: e.to_string(),
            }),
        }
    }

    /// The model's rewrite of `text`, or None when no model is ready or it failed.
    pub fn rewrite(&self, text: &str) -> Option<String> {
        use aural_text::rewrite;
        let mut guard = self.client.lock().ok()?;
        let client = guard.as_mut()?;
        client
            .generate(
                rewrite::SYSTEM,
                &rewrite::examples(),
                &rewrite::user_prompt(text),
                rewrite::max_tokens(text),
                REWRITE_TIMEOUT,
            )
            .ok()
    }
}
