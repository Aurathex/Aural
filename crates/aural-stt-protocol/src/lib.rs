//! How the app talks to STT worker processes. Each engine family (ONNX, ggml) runs in
//! its own process so incompatible native runtimes never share a binary, GPU/driver
//! crashes cannot take the app down, and unloading a model returns all its memory.

pub mod client;
pub mod codec;
pub mod msg;
pub mod worker;

/// Bumped on any incompatible change to `msg`.
pub const PROTOCOL_VERSION: u32 = 1;
