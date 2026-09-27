//! Captures the engines' log lines (whisper.cpp through whisper-rs's `log_backend`,
//! transcribe-rs directly) so a model's startup log can be checked after loading, for
//! example to prove which device it really runs on. Each worker process runs one engine
//! family, so one process-wide buffer is enough.

use std::sync::{Mutex, Once};

static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static INSTALL: Once = Once::new();

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        let line = record.args().to_string();
        if std::env::var_os("AURAL_BENCH_VERBOSE").is_some() {
            eprintln!("{line}");
        }
        if let Ok(mut v) = LINES.lock() {
            v.push(line);
        }
    }
    fn flush(&self) {}
}

pub fn install() {
    INSTALL.call_once(|| {
        if log::set_boxed_logger(Box::new(Capture)).is_ok() {
            log::set_max_level(log::LevelFilter::Trace);
        }
    });
}

/// The lines captured since the last call.
pub fn take() -> Vec<String> {
    LINES
        .lock()
        .map(|mut v| std::mem::take(&mut *v))
        .unwrap_or_default()
}
