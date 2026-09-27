//! Messages between app (client) and worker.

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
    Stats {
        working_set_mb: u64,
        peak_working_set_mb: u64,
        /// Graphics-card memory in use by the worker; None if Windows can't tell.
        gpu_memory_mb: Option<u64>,
    },
}
