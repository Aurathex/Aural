//! How the app talks to STT worker processes. Each engine family (ONNX, ggml) runs in
//! its own process so incompatible native runtimes never share a binary, GPU/driver
//! crashes cannot take the app down, and unloading a model returns all its memory.

pub mod bench;
pub mod client;
pub mod codec;
pub mod msg;
pub mod worker;

/// Bumped on any incompatible change to `msg`.
/// 2: adds `Stats`.
/// 3: adds live text (`LiveBegin`, `LiveAudio`, `LiveEnd`, `LiveCancel`).
/// 4: adds text models (`LoadText`, `Generate`).
pub const PROTOCOL_VERSION: u32 = 4;
