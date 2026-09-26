#[cfg(all(feature = "onnx", feature = "ggml"))]
compile_error!("features `onnx` and `ggml` are mutually exclusive (ONNX Runtime and whisper.cpp must not link into one binary); build them separately");

#[cfg(feature = "onnx")]
pub mod onnx_parakeet;
