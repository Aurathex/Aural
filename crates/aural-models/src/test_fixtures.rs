//! Shared test builders for catalog entries and hardware profiles.

use crate::catalog::{Engine, ModelEntry, ModelLicense, Runtime};

pub fn entry(id: &str, engine: Engine, streaming: bool) -> ModelEntry {
    ModelEntry {
        id: id.into(),
        name: id.into(),
        family: id.split('-').next().unwrap_or(id).into(),
        engine,
        runtime: match engine {
            Engine::Whisper => Runtime::Ggml,
            _ => Runtime::Onnx,
        },
        precision: "int8".into(),
        streaming,
        probe: false,
        description: String::new(),
        languages: vec!["en".into()],
        license: ModelLicense {
            id: "MIT".into(),
            url: String::new(),
            attribution: String::new(),
        },
        source: String::new(),
        files: vec![],
        variants: vec![],
    }
}
