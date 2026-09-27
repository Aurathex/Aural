//! Messages between app (client) and worker.

use aural_engines::live::LiveMode;
use aural_engines::{Backend, Engine};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Load {
        model: PathBuf,
        engine: Engine,
        backend: Backend,
        threads: usize,
    },
    /// Followed by one PCM frame of `samples` 16 kHz mono f32 samples.
    Transcribe {
        id: u64,
        samples: u32,
    },
    /// How much memory the worker uses (for the hardware test).
    Stats,
    /// Open a live-text stream (one at a time; a new one replaces the old).
    LiveBegin {
        id: u64,
    },
    /// Followed by one PCM frame: audio recorded since the previous LiveAudio.
    LiveAudio {
        id: u64,
        samples: u32,
    },
    /// Followed by one PCM frame with the last audio; answered with Transcript.
    LiveEnd {
        id: u64,
        samples: u32,
    },
    LiveCancel {
        id: u64,
    },
    /// Load a text model (AI cleanup) into this worker, replacing any other.
    LoadText {
        model: PathBuf,
        threads: usize,
    },
    /// Answered with `Generated`.
    Generate {
        id: u64,
        system: String,
        examples: Vec<(String, String)>,
        user: String,
        max_tokens: u32,
    },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    /// First message a worker sends.
    Ready {
        protocol: u32,
        engines: Vec<Engine>,
    },
    Loaded {
        label: String,
        backend: String,
        ms: u64,
    },
    Transcript {
        id: u64,
        text: String,
        ms: u64,
    },
    Error {
        id: Option<u64>,
        message: String,
    },
    LiveStarted {
        id: u64,
        mode: LiveMode,
    },
    LiveText {
        id: u64,
        stable: String,
        tentative: String,
        ms: u64,
    },
    TextLoaded {
        label: String,
        ms: u64,
    },
    Generated {
        id: u64,
        text: String,
        ms: u64,
    },
    Stats {
        working_set_mb: u64,
        peak_working_set_mb: u64,
        /// Graphics-card memory in use by the worker; None if Windows can't tell.
        gpu_memory_mb: Option<u64>,
    },
}
