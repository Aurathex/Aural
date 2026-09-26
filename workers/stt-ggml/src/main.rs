//! Speech worker process for ggml engines (whisper.cpp). Speaks the Aural worker
//! protocol on stdin/stdout; see `aural-stt-protocol`.
#![windows_subsystem = "windows"]

fn main() {
    let result = aural_stt_protocol::worker::serve_stdio(
        vec![aural_engines::Engine::Whisper],
        |model, engine, backend, threads| {
            aural_engines::build_engine(engine, model, backend, threads)
        },
    );
    if result.is_err() {
        std::process::exit(1);
    }
}
