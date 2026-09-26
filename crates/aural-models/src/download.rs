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

/// Download every file of `entry` into the store, reporting progress over the whole
/// model. Cancelling keeps `.partial` files so the next attempt resumes.
pub fn download(
    entry: &ModelEntry,
    store: &ModelStore,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<(), DownloadError> {
    let dir = store.dir(&entry.id);
    std::fs::create_dir_all(&dir).map_err(io)?;
    let total = entry.total_size();
    let mut done_before = 0u64;
    for file in &entry.files {
        fetch_file(file, &dir, cancel, &mut |n| {
            on_progress(Progress {
                downloaded: done_before + n,
                total,
            })
        })?;
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
        let mut req = ureq::get(&file.url);
        if have > 0 {
            req = req.header("Range", &format!("bytes={have}-"));
        }
        let mut resp = req.call().map_err(|e| DownloadError::Http(e.to_string()))?;
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
        let mut reader = resp.body_mut().as_reader();
        let mut buf = vec![0u8; 256 * 1024];
        on_bytes(have);
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(DownloadError::Cancelled);
            }
            let n = reader
                .read(&mut buf)
                .map_err(|e| DownloadError::Http(e.to_string()))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(io)?;
            have += n as u64;
            on_bytes(have.min(file.size));
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
