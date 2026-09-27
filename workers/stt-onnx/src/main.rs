//! Speech worker process for ONNX engines, which also runs the local text models for
//! AI cleanup (a separate process of this binary, so they never share memory with the
//! speech model). Speaks the Aural worker protocol on
//! stdin/stdout; see `aural-stt-protocol`.
#![windows_subsystem = "windows"]

fn main() {
    let result = aural_stt_protocol::worker::serve_stdio_all(
        vec![
            aural_engines::Engine::Parakeet,
            aural_engines::Engine::Moonshine,
        ],
        |model, engine, backend, threads| {
            aural_engines::build_engine(engine, model, backend, threads)
        },
        |model, threads| {
            aural_engines::onnx_accel::select(aural_engines::Backend::Cpu)?;
            aural_engines::build_text_model(model, threads)
        },
    );
    if result.is_err() {
        std::process::exit(1);
    }
}
