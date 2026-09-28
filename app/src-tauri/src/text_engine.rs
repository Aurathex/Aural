//! The text model for AI cleanup, in its own ONNX worker process (never the speech
//! worker, so neither model's memory or a crash affects the other). Loaded only while AI
//! cleanup is in use and a text model is downloaded.

use aural_stt_protocol::client::{ClientError, SttClient, WorkerKiller, WorkerSpec};
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

/// Whether `model` should be (re)loaded: not when it is loading, ready, or failed to
/// start (a failed model is retried when the user turns AI tidy-up off and on again or
/// picks another model, not on every settings change).
pub fn needs_load(status: &TextStatus, model: &str) -> bool {
    match status {
        TextStatus::Off => true,
        TextStatus::Loading { model: m }
        | TextStatus::Ready { model: m, .. }
        | TextStatus::Error { model: m, .. } => m != model,
    }
}

/// A worker still busy with an abandoned rewrite (timeout) or gone (crash) would make
/// every later rewrite wait; it is replaced.
pub fn replace_after(e: &ClientError) -> bool {
    matches!(e, ClientError::Timeout | ClientError::WorkerDied(_))
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

    /// The model's rewrite of `text`, or None when no model is ready or it failed. After
    /// a timeout or crash the worker is stopped and the status set to Off, so the next
    /// `reload_text_engine` starts a fresh one.
    pub fn rewrite(&self, text: &str, allow: aural_text::rewrite::Allow) -> Option<String> {
        use aural_text::rewrite;
        let mut guard = self.client.lock().ok()?;
        let client = guard.as_mut()?;
        let result = client.generate(
            &rewrite::system(allow),
            &rewrite::examples_for(allow),
            &rewrite::user_prompt(text),
            rewrite::max_tokens(text),
            REWRITE_TIMEOUT,
        );
        match result {
            Ok(t) => Some(t),
            Err(e) => {
                if replace_after(&e) {
                    if let Some(k) = self.killer.lock().ok().and_then(|mut g| g.take()) {
                        k.kill();
                    }
                    *guard = None;
                    self.set(TextStatus::Off);
                }
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aural_stt_protocol::client::ClientError;

    #[test]
    fn a_model_that_failed_to_start_is_not_retried_on_every_settings_change() {
        let failed = TextStatus::Error {
            model: "m".into(),
            message: "x".into(),
        };
        assert!(!needs_load(&failed, "m"));
        assert!(needs_load(&failed, "other"));
        assert!(needs_load(&TextStatus::Off, "m"));
        assert!(!needs_load(
            &TextStatus::Ready {
                model: "m".into(),
                label: "l".into()
            },
            "m"
        ));
    }

    #[test]
    fn a_rewrite_that_timed_out_or_crashed_replaces_the_worker() {
        assert!(replace_after(&ClientError::Timeout));
        assert!(replace_after(&ClientError::WorkerDied("x".into())));
        assert!(!replace_after(&ClientError::Engine("bad input".into())));
    }
}
