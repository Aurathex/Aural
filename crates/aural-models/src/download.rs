//! Resumable, verified downloads. Each file streams to `<name>.partial` (resuming with
//! an HTTP Range request), is SHA-256 checked, then renamed into place; the receipt is
//! written only after every file verified.

use crate::catalog::{ModelEntry, ModelFile};
use crate::store::ModelStore;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub downloaded: u64,
    pub total: u64,
}

#[derive(Debug)]
pub enum DownloadError {
    Cancelled,
    Http(String),
    Io(String),
    Checksum { file: String },
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadError::Cancelled => write!(f, "download cancelled"),
            DownloadError::Http(e) => write!(f, "download failed: {e}"),
            DownloadError::Io(e) => write!(f, "could not save the model: {e}"),
            DownloadError::Checksum { file } => {
                write!(
                    f,
                    "{file} did not match its expected checksum and was discarded"
                )
            }
        }
    }
}

impl std::error::Error for DownloadError {}

fn io(e: std::io::Error) -> DownloadError {
    DownloadError::Io(e.to_string())
}

fn hash_file(path: &Path) -> Result<String, DownloadError> {
    let mut f = File::open(path).map_err(io)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(io)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[derive(Debug, Clone)]
pub struct DownloadOptions {
    /// Time allowed to connect and to receive the response headers.
    pub connect_timeout: Duration,
    /// Give up when no data arrives for this long (a stalled connection).
    pub stall_timeout: Duration,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(20),
            stall_timeout: Duration::from_secs(60),
        }
    }
}

/// Download every file of `entry` into the store, reporting progress over the whole
/// model. Cancelling keeps `.partial` files so the next attempt resumes.
pub fn download(
    entry: &ModelEntry,
    store: &ModelStore,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<(), DownloadError> {
    download_with(
        entry,
        store,
        cancel,
        on_progress,
        &DownloadOptions::default(),
    )
}

pub fn download_with(
    entry: &ModelEntry,
    store: &ModelStore,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(Progress),
    opts: &DownloadOptions,
) -> Result<(), DownloadError> {
    let dir = store.dir(&entry.id);
    std::fs::create_dir_all(&dir).map_err(io)?;
    let total = entry.total_size();
    let mut done_before = 0u64;
    for file in &entry.files {
        fetch_file(
            file,
            &dir,
            cancel,
            &mut |n| {
                on_progress(Progress {
                    downloaded: done_before + n,
                    total,
                })
            },
            opts,
        )?;
        done_before += file.size;
    }
    store
        .write_receipt(entry)
        .map_err(|e| DownloadError::Io(e.to_string()))?;
    on_progress(Progress {
        downloaded: total,
        total,
    });
    Ok(())
}

fn fetch_file(
    file: &ModelFile,
    dir: &Path,
    cancel: &AtomicBool,
    on_bytes: &mut dyn FnMut(u64),
    opts: &DownloadOptions,
) -> Result<(), DownloadError> {
    let target = dir.join(&file.name);
    let partial = dir.join(format!("{}.partial", file.name));

    // Already present and correct (e.g. an interrupted multi-file install).
    if std::fs::metadata(&target).is_ok_and(|m| m.len() == file.size)
        && hash_file(&target)? == file.sha256
    {
        on_bytes(file.size);
        return Ok(());
    }

    let mut have = std::fs::metadata(&partial).map_or(0, |m| m.len());
    if have > file.size {
        std::fs::remove_file(&partial).map_err(io)?;
        have = 0;
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(DownloadError::Cancelled);
    }

    if have < file.size {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(opts.connect_timeout))
            .timeout_recv_response(Some(opts.connect_timeout))
            .build()
            .into();
        let mut req = agent.get(&file.url);
        if have > 0 {
            req = req.header("Range", &format!("bytes={have}-"));
        }
        let resp = req.call().map_err(|e| DownloadError::Http(e.to_string()))?;
        // A server that ignores Range sends the whole file again: start over.
        if have > 0 && resp.status().as_u16() != 206 {
            have = 0;
        }
        let mut out = OpenOptions::new()
            .create(true)
            .write(true)
            .append(have > 0)
            .truncate(have == 0)
            .open(&partial)
            .map_err(io)?;
        on_bytes(have);

        // Reads block, so they happen on a helper thread; this loop stays free to
        // notice Cancel and stalled connections within a quarter of a second.
        let (tx, rx) = mpsc::sync_channel::<Result<Vec<u8>, String>>(8);
        let mut reader = resp.into_body().into_reader();
        std::thread::spawn(move || loop {
            let mut buf = vec![0u8; 256 * 1024];
            match reader.read(&mut buf) {
                Ok(0) => {
                    let _ = tx.send(Ok(Vec::new()));
                    return;
                }
                Ok(n) => {
                    buf.truncate(n);
                    if tx.send(Ok(buf)).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                    return;
                }
            }
        });
        let mut last_data = Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(DownloadError::Cancelled);
            }
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(Ok(chunk)) if chunk.is_empty() => break,
                Ok(Ok(chunk)) => {
                    if have + chunk.len() as u64 > file.size {
                        drop(out);
                        let _ = std::fs::remove_file(&partial);
                        return Err(DownloadError::Http(format!(
                            "the server sent more than the expected {} bytes for {}",
                            file.size, file.name
                        )));
                    }
                    out.write_all(&chunk).map_err(io)?;
                    have += chunk.len() as u64;
                    on_bytes(have);
                    last_data = Instant::now();
                }
                Ok(Err(e)) => return Err(DownloadError::Http(e)),
                Err(RecvTimeoutError::Timeout) => {
                    if last_data.elapsed() > opts.stall_timeout {
                        return Err(DownloadError::Http(format!(
                            "no data received for {} s; check your connection and try again",
                            opts.stall_timeout.as_secs().max(1)
                        )));
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(DownloadError::Http("the connection closed".into()))
                }
            }
        }
        out.flush().map_err(io)?;
    }

    if hash_file(&partial)? != file.sha256 {
        let _ = std::fs::remove_file(&partial);
        return Err(DownloadError::Checksum {
            file: file.name.clone(),
        });
    }
    std::fs::rename(&partial, &target).map_err(io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Catalog, ModelEntry};
    use crate::store::ModelStore;
    use sha2::{Digest, Sha256};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    struct Server {
        base: String,
        requests: Arc<Mutex<Vec<Option<String>>>>,
    }

    /// Serves `body` at /f.bin, honouring `Range: bytes=N-`, recording Range headers.
    fn serve(body: Vec<u8>) -> Server {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        std::thread::spawn(move || {
            for req in server.incoming_requests() {
                let range = req
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Range"))
                    .map(|h| h.value.to_string());
                seen.lock().unwrap().push(range.clone());
                let start = range
                    .as_deref()
                    .and_then(|r| r.strip_prefix("bytes="))
                    .and_then(|r| r.strip_suffix('-'))
                    .and_then(|n| n.parse::<usize>().ok())
                    .unwrap_or(0);
                let status = if start > 0 { 206 } else { 200 };
                let resp =
                    tiny_http::Response::from_data(body[start..].to_vec()).with_status_code(status);
                let _ = req.respond(resp);
            }
        });
        Server {
            base: format!("http://127.0.0.1:{port}"),
            requests,
        }
    }

    fn sha(b: &[u8]) -> String {
        Sha256::digest(b)
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect()
    }

    fn entry(url: &str, body: &[u8], sha256: String) -> ModelEntry {
        let mut e = Catalog::builtin()
            .get("whisper-base.en-q8")
            .unwrap()
            .clone();
        e.id = "test-model".into();
        e.files[0].name = "f.bin".into();
        e.files[0].url = url.into();
        e.files[0].sha256 = sha256;
        e.files[0].size = body.len() as u64;
        e
    }

    fn body() -> Vec<u8> {
        (0..200_000u32).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn downloads_verifies_and_installs() {
        let b = body();
        let srv = serve(b.clone());
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let e = entry(&format!("{}/f.bin", srv.base), &b, sha(&b));
        let mut last = None;
        download(&e, &store, &AtomicBool::new(false), &mut |p| last = Some(p)).unwrap();
        assert!(store.is_installed(&e));
        assert_eq!(std::fs::read(store.dir(&e.id).join("f.bin")).unwrap(), b);
        let p = last.unwrap();
        assert_eq!((p.downloaded, p.total), (b.len() as u64, b.len() as u64));
        assert!(!store.dir(&e.id).join("f.bin.partial").exists());
    }

    #[test]
    fn resumes_from_a_partial_file_with_a_range_request() {
        let b = body();
        let srv = serve(b.clone());
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let e = entry(&format!("{}/f.bin", srv.base), &b, sha(&b));
        std::fs::create_dir_all(store.dir(&e.id)).unwrap();
        std::fs::write(store.dir(&e.id).join("f.bin.partial"), &b[..50_000]).unwrap();
        download(&e, &store, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(std::fs::read(store.dir(&e.id).join("f.bin")).unwrap(), b);
        assert_eq!(
            srv.requests.lock().unwrap()[0].as_deref(),
            Some("bytes=50000-")
        );
    }

    #[test]
    fn checksum_mismatch_fails_deletes_partial_and_does_not_install() {
        let b = body();
        let srv = serve(b.clone());
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let e = entry(&format!("{}/f.bin", srv.base), &b, "0".repeat(64));
        let err = download(&e, &store, &AtomicBool::new(false), &mut |_| {}).unwrap_err();
        assert!(matches!(err, DownloadError::Checksum { .. }), "{err}");
        assert!(!store.is_installed(&e));
        assert!(!store.dir(&e.id).join("f.bin").exists());
        assert!(!store.dir(&e.id).join("f.bin.partial").exists());
    }

    #[test]
    fn cancel_stops_and_keeps_the_partial_for_resume() {
        let b = body();
        let srv = serve(b.clone());
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let e = entry(&format!("{}/f.bin", srv.base), &b, sha(&b));
        let cancel = AtomicBool::new(true);
        let err = download(&e, &store, &cancel, &mut |_| {}).unwrap_err();
        assert!(matches!(err, DownloadError::Cancelled));
        assert!(!store.is_installed(&e));
    }

    #[test]
    fn already_verified_file_is_not_downloaded_again() {
        let b = body();
        let srv = serve(b.clone());
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let e = entry(&format!("{}/f.bin", srv.base), &b, sha(&b));
        std::fs::create_dir_all(store.dir(&e.id)).unwrap();
        std::fs::write(store.dir(&e.id).join("f.bin"), &b).unwrap();
        download(&e, &store, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert!(srv.requests.lock().unwrap().is_empty());
        assert!(store.is_installed(&e));
    }

    /// Sends the first `send` bytes of a `total`-byte body, then goes silent.
    fn serve_stalling(total: usize, send: usize) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                use std::io::{Read, Write};
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {total}\r\n\r\n");
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&vec![7u8; send]);
                let _ = s.flush();
                std::thread::sleep(std::time::Duration::from_secs(30));
            }
        });
        format!("http://127.0.0.1:{port}/f.bin")
    }

    fn quick() -> DownloadOptions {
        DownloadOptions {
            stall_timeout: std::time::Duration::from_millis(700),
            ..Default::default()
        }
    }

    #[test]
    fn a_stalled_download_fails_instead_of_hanging() {
        let url = serve_stalling(100_000, 1_000);
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let e = entry(&url, &[0; 100_000], sha(&[0; 100_000]));
        let t0 = std::time::Instant::now();
        let err =
            download_with(&e, &store, &AtomicBool::new(false), &mut |_| {}, &quick()).unwrap_err();
        assert!(matches!(err, DownloadError::Http(_)), "{err}");
        assert!(
            t0.elapsed() < std::time::Duration::from_secs(5),
            "took {:?}",
            t0.elapsed()
        );
    }

    #[test]
    fn cancel_works_even_while_the_server_is_silent() {
        let url = serve_stalling(100_000, 1_000);
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let e = entry(&url, &[0; 100_000], sha(&[0; 100_000]));
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            c2.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        let slow = DownloadOptions {
            stall_timeout: std::time::Duration::from_secs(60),
            ..Default::default()
        };
        let t0 = std::time::Instant::now();
        let err = download_with(&e, &store, &cancel, &mut |_| {}, &slow).unwrap_err();
        assert!(matches!(err, DownloadError::Cancelled), "{err}");
        assert!(
            t0.elapsed() < std::time::Duration::from_secs(3),
            "took {:?}",
            t0.elapsed()
        );
    }

    #[test]
    fn a_server_sending_more_than_expected_is_stopped_at_the_expected_size() {
        let b = body();
        let big: Vec<u8> = b.iter().chain(b.iter()).copied().collect();
        let srv = serve(big);
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let e = entry(&format!("{}/f.bin", srv.base), &b, sha(&b));
        let err = download(&e, &store, &AtomicBool::new(false), &mut |_| {}).unwrap_err();
        assert!(matches!(err, DownloadError::Http(_)), "{err}");
        let partial = store.dir(&e.id).join("f.bin.partial");
        assert!(!partial.exists() || std::fs::metadata(&partial).unwrap().len() <= b.len() as u64);
        assert!(!store.is_installed(&e));
    }

    #[test]
    fn http_error_is_reported() {
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        // Nothing listens on port 9 (discard) on a dev box; connection fails fast.
        let e = entry("http://127.0.0.1:9/f.bin", b"x", sha(b"x"));
        let err = download(&e, &store, &AtomicBool::new(false), &mut |_| {}).unwrap_err();
        assert!(matches!(err, DownloadError::Http(_)), "{err}");
    }
}
