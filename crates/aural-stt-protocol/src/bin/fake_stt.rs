//! Test double for the worker protocol. Flags: --crash-first (exit on the first
//! transcribe of a state dir, then behave), --crash-always, --hang-always,
//! --bad-version, --fail-load, --state=<dir>.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    let state = args
        .iter()
        .find_map(|a| a.strip_prefix("--state="))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let marker = state.join("crashed-once");
    let crash_first = has("--crash-first") && !marker.exists();
    let crash_always = has("--crash-always");
    let hang = has("--hang-always");
    let fail_load = has("--fail-load");

    if has("--bad-version") {
        let mut out = std::io::stdout().lock();
        let _ = aural_stt_protocol::codec::write_msg(
            &mut out,
            &aural_stt_protocol::msg::Response::Ready {
                protocol: 999,
                engines: vec![],
            },
        );
        return;
    }

    struct Fake {
        crash: bool,
        hang: bool,
        marker: std::path::PathBuf,
    }
    impl aural_engines::Transcriber for Fake {
        fn label(&self) -> String {
            "fake".into()
        }
        fn transcribe(&mut self, pcm: &[f32]) -> anyhow::Result<String> {
            if self.hang {
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
            if self.crash {
                let _ = std::fs::write(&self.marker, b"1");
                std::process::exit(3);
            }
            Ok(format!("{} samples", pcm.len()))
        }
    }

    let stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout().lock();
    let _ = aural_stt_protocol::worker::serve(
        stdin,
        &mut stdout,
        vec![aural_engines::Engine::Parakeet],
        move |_, _, _, _| {
            if fail_load {
                anyhow::bail!("model files missing");
            }
            Ok(Box::new(Fake {
                crash: crash_first || crash_always,
                hang,
                marker: marker.clone(),
            }) as Box<dyn aural_engines::Transcriber>)
        },
    );
}
