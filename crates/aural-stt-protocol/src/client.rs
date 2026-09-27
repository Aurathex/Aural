//! The app side: spawn a worker, load a model, transcribe with a timeout. A worker that
//! crashes or hangs is replaced, the model reloaded and the request retried once, so a
//! driver hiccup costs the user a second, not their dictation.

use crate::codec::{read_msg, write_msg, write_pcm};
use crate::msg::{Request, Response};
use crate::PROTOCOL_VERSION;
use aural_engines::live::{LiveMode, LiveText};
use aural_engines::{Backend, Engine};
use std::os::windows::io::{AsHandle, OwnedHandle};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Memory a worker process uses (hardware test).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerStats {
    pub working_set_mb: u64,
    pub peak_working_set_mb: u64,
    pub gpu_memory_mb: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct WorkerSpec {
    pub exe: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug)]
pub enum ClientError {
    Spawn(String),
    Protocol(String),
    Timeout,
    WorkerDied(String),
    Engine(String),
    NotLoaded,
    /// The client was stopped on purpose (model switch, unload, Delete Aural).
    Stopped,
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Spawn(e) => write!(f, "could not start the speech engine: {e}"),
            ClientError::Protocol(e) => write!(f, "speech engine protocol error: {e}"),
            ClientError::Timeout => write!(f, "the speech engine took too long"),
            ClientError::WorkerDied(e) => write!(f, "the speech engine stopped: {e}"),
            ClientError::Engine(e) => write!(f, "{e}"),
            ClientError::NotLoaded => write!(f, "no speech model is loaded"),
            ClientError::Stopped => write!(f, "the speech engine was stopped"),
        }
    }
}

impl std::error::Error for ClientError {}

#[derive(Debug, Clone)]
struct LoadArgs {
    model: PathBuf,
    engine: Engine,
    backend: Backend,
    threads: usize,
    timeout: Duration,
}

struct Proc {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Result<Response, String>>,
}

impl Proc {
    fn start(spec: &WorkerSpec) -> Result<Self, ClientError> {
        let mut cmd = Command::new(&spec.exe);
        cmd.args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| ClientError::Spawn(format!("{}: {e}", spec.exe.display())))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ClientError::Spawn("no stdin".into()))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| ClientError::Spawn("no stdout".into()))?;
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("aural-stt-reader".into())
            .spawn(move || loop {
                match read_msg::<_, Response>(&mut stdout) {
                    Ok(Some(m)) => {
                        if tx.send(Ok(m)).is_err() {
                            return;
                        }
                    }
                    Ok(None) => {
                        let _ = tx.send(Err("worker exited".into()));
                        return;
                    }
                    Err(e) => {
                        let _ = tx.send(Err(format!("{e:#}")));
                        return;
                    }
                }
            })
            .map_err(|e| ClientError::Spawn(e.to_string()))?;
        let mut p = Proc { child, stdin, rx };
        match p.recv(Duration::from_secs(20))? {
            Response::Ready { protocol, .. } if protocol == PROTOCOL_VERSION => Ok(p),
            Response::Ready { protocol, .. } => {
                p.kill();
                Err(ClientError::Protocol(format!(
                    "worker speaks protocol {protocol}, app expects {PROTOCOL_VERSION}"
                )))
            }
            other => {
                p.kill();
                Err(ClientError::Protocol(format!(
                    "unexpected first message {other:?}"
                )))
            }
        }
    }

    fn recv(&mut self, timeout: Duration) -> Result<Response, ClientError> {
        match self.rx.recv_timeout(timeout) {
            Ok(Ok(m)) => Ok(m),
            Ok(Err(e)) => Err(ClientError::WorkerDied(e)),
            Err(RecvTimeoutError::Timeout) => Err(ClientError::Timeout),
            Err(RecvTimeoutError::Disconnected) => {
                Err(ClientError::WorkerDied("worker exited".into()))
            }
        }
    }

    fn send(&mut self, req: &Request, pcm: Option<&[f32]>) -> Result<(), ClientError> {
        let r = write_msg(&mut self.stdin, req).and_then(|_| match pcm {
            Some(p) => write_pcm(&mut self.stdin, p),
            None => Ok(()),
        });
        r.map_err(|e| ClientError::WorkerDied(format!("{e:#}")))
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Stops a client's worker from any thread, without waiting for whatever the client is
/// doing (a long load or transcription). The client then refuses further work instead
/// of restarting the worker. Holds a handle to the process itself, never a bare PID, so
/// it can't terminate an unrelated process that later reused the ID.
#[derive(Clone)]
pub struct WorkerKiller {
    process: Arc<Mutex<Option<OwnedHandle>>>,
    stopped: Arc<AtomicBool>,
}

impl WorkerKiller {
    pub fn kill(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Ok(guard) = self.process.lock() {
            if let Some(h) = guard.as_ref() {
                terminate(h);
            }
        }
    }
}

#[cfg(windows)]
fn terminate(h: &OwnedHandle) {
    use std::os::windows::io::AsRawHandle;
    // SAFETY: a valid process handle we own; terminating an already-exited process
    // just fails.
    unsafe {
        windows_sys::Win32::System::Threading::TerminateProcess(h.as_raw_handle(), 1);
    }
}

pub struct SttClient {
    spec: WorkerSpec,
    proc: Option<Proc>,
    loaded: Option<LoadArgs>,
    label: Option<String>,
    backend: Option<String>,
    restarts: u32,
    next_id: u64,
    /// Id of the open live stream.
    live: Option<u64>,
    process: Arc<Mutex<Option<OwnedHandle>>>,
    stopped: Arc<AtomicBool>,
}

impl SttClient {
    pub fn spawn(spec: WorkerSpec) -> Result<Self, ClientError> {
        let proc = Proc::start(&spec)?;
        let c = Self {
            spec,
            proc: Some(proc),
            loaded: None,
            label: None,
            backend: None,
            restarts: 0,
            next_id: 1,
            live: None,
            process: Arc::new(Mutex::new(None)),
            stopped: Arc::new(AtomicBool::new(false)),
        };
        c.track_process();
        Ok(c)
    }

    /// Remember the current worker's process handle for the kill switch.
    fn track_process(&self) {
        let handle = self
            .proc
            .as_ref()
            .and_then(|p| p.child.as_handle().try_clone_to_owned().ok());
        if let Ok(mut g) = self.process.lock() {
            *g = handle;
        }
    }

    pub fn killer(&self) -> WorkerKiller {
        WorkerKiller {
            process: self.process.clone(),
            stopped: self.stopped.clone(),
        }
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    pub fn backend(&self) -> Option<&str> {
        self.backend.as_deref()
    }

    pub fn restarts(&self) -> u32 {
        self.restarts
    }

    pub fn load(
        &mut self,
        model: PathBuf,
        engine: Engine,
        backend: Backend,
        threads: usize,
        timeout: Duration,
    ) -> Result<(), ClientError> {
        let args = LoadArgs {
            model,
            engine,
            backend,
            threads,
            timeout,
        };
        if self.is_stopped() {
            return Err(ClientError::Stopped);
        }
        match self.send_load(&args) {
            Err(_) if self.is_stopped() => Err(ClientError::Stopped),
            Err(e) => Err(e),
            Ok(()) => {
                self.loaded = Some(args);
                Ok(())
            }
        }
    }

    fn send_load(&mut self, a: &LoadArgs) -> Result<(), ClientError> {
        let proc = self.proc.as_mut().ok_or(ClientError::NotLoaded)?;
        proc.send(
            &Request::Load {
                model: a.model.clone(),
                engine: a.engine,
                backend: a.backend,
                threads: a.threads,
            },
            None,
        )?;
        loop {
            match proc.recv(a.timeout)? {
                Response::Loaded { label, backend, .. } => {
                    self.label = Some(label);
                    self.backend = Some(backend);
                    return Ok(());
                }
                Response::Error { id: None, message } => return Err(ClientError::Engine(message)),
                _ => continue,
            }
        }
    }

    fn restart(&mut self) -> Result<(), ClientError> {
        if let Some(mut p) = self.proc.take() {
            p.kill();
        }
        if self.is_stopped() {
            return Err(ClientError::Stopped);
        }
        self.restarts += 1;
        self.proc = Some(Proc::start(&self.spec)?);
        self.track_process();
        if let Some(args) = self.loaded.clone() {
            self.send_load(&args)?;
        }
        Ok(())
    }

    fn attempt(&mut self, pcm: &[f32], timeout: Duration) -> Result<String, ClientError> {
        let id = self.next_id;
        self.next_id += 1;
        let proc = self.proc.as_mut().ok_or(ClientError::NotLoaded)?;
        proc.send(
            &Request::Transcribe {
                id,
                samples: pcm.len() as u32,
            },
            Some(pcm),
        )?;
        loop {
            match proc.recv(timeout)? {
                Response::Transcript { id: got, text, .. } if got == id => return Ok(text),
                Response::Error {
                    id: Some(got),
                    message,
                } if got == id => return Err(ClientError::Engine(message)),
                _ => continue,
            }
        }
    }

    /// How much memory the worker uses. Only a measurement, so a failure is reported,
    /// never retried with a new worker.
    pub fn stats(&mut self, timeout: Duration) -> Result<WorkerStats, ClientError> {
        if self.is_stopped() {
            return Err(ClientError::Stopped);
        }
        let proc = self.proc.as_mut().ok_or(ClientError::NotLoaded)?;
        proc.send(&Request::Stats, None)?;
        loop {
            if let Response::Stats {
                working_set_mb,
                peak_working_set_mb,
                gpu_memory_mb,
            } = proc.recv(timeout)?
            {
                return Ok(WorkerStats {
                    working_set_mb,
                    peak_working_set_mb,
                    gpu_memory_mb,
                });
            }
        }
    }

    fn live_request(
        &mut self,
        req: Request,
        pcm: Option<&[f32]>,
        timeout: Duration,
    ) -> Result<Response, ClientError> {
        if self.is_stopped() {
            return Err(ClientError::Stopped);
        }
        let want = match &req {
            Request::LiveBegin { id }
            | Request::LiveAudio { id, .. }
            | Request::LiveEnd { id, .. } => *id,
            _ => return Err(ClientError::Protocol("not a live request".into())),
        };
        let proc = self.proc.as_mut().ok_or(ClientError::NotLoaded)?;
        proc.send(&req, pcm)?;
        loop {
            match proc.recv(timeout)? {
                Response::Error {
                    id: Some(got),
                    message,
                } if got == want => return Err(ClientError::Engine(message)),
                r @ (Response::LiveStarted { id: got, .. }
                | Response::LiveText { id: got, .. }
                | Response::Transcript { id: got, .. })
                    if got == want =>
                {
                    return Ok(r)
                }
                _ => continue,
            }
        }
    }

    /// Open a live-text stream. Live text is a preview: a failure is reported, never
    /// retried, and the caller falls back to `transcribe` for the final text.
    pub fn live_begin(&mut self, timeout: Duration) -> Result<LiveMode, ClientError> {
        if self.loaded.is_none() {
            return Err(ClientError::NotLoaded);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.live = None;
        match self.live_request(Request::LiveBegin { id }, None, timeout)? {
            Response::LiveStarted { mode, .. } => {
                self.live = Some(id);
                Ok(mode)
            }
            other => Err(ClientError::Protocol(format!("unexpected {other:?}"))),
        }
    }

    /// Send audio recorded since the last call; returns the text so far.
    pub fn live_push(&mut self, pcm: &[f32], timeout: Duration) -> Result<LiveText, ClientError> {
        let id = self.live.ok_or(ClientError::NotLoaded)?;
        let req = Request::LiveAudio {
            id,
            samples: pcm.len() as u32,
        };
        match self.live_request(req, Some(pcm), timeout) {
            Ok(Response::LiveText {
                stable, tentative, ..
            }) => Ok(LiveText { stable, tentative }),
            Ok(other) => Err(ClientError::Protocol(format!("unexpected {other:?}"))),
            Err(e) => {
                self.live = None;
                Err(e)
            }
        }
    }

    /// Send the last audio and close the stream; returns the final text.
    pub fn live_end(&mut self, pcm: &[f32], timeout: Duration) -> Result<String, ClientError> {
        let id = self.live.take().ok_or(ClientError::NotLoaded)?;
        let req = Request::LiveEnd {
            id,
            samples: pcm.len() as u32,
        };
        match self.live_request(req, Some(pcm), timeout)? {
            Response::Transcript { text, .. } => Ok(text),
            other => Err(ClientError::Protocol(format!("unexpected {other:?}"))),
        }
    }

    pub fn live_cancel(&mut self) {
        if let (Some(id), Some(proc)) = (self.live.take(), self.proc.as_mut()) {
            let _ = proc.send(&Request::LiveCancel { id }, None);
        }
    }

    /// Transcribe 16 kHz mono audio. On a crash or timeout the worker is replaced, the
    /// model reloaded and the request retried once.
    pub fn transcribe(&mut self, pcm: &[f32], timeout: Duration) -> Result<String, ClientError> {
        if self.is_stopped() {
            return Err(ClientError::Stopped);
        }
        if self.loaded.is_none() {
            return Err(ClientError::NotLoaded);
        }
        match self.attempt(pcm, timeout) {
            Err(_) if self.is_stopped() => Err(ClientError::Stopped),
            Err(ClientError::Timeout | ClientError::WorkerDied(_)) => {
                self.restart()?;
                let second = self.attempt(pcm, timeout);
                if matches!(second, Err(ClientError::Timeout)) {
                    // Leave a fresh worker behind rather than a hung one.
                    let _ = self.restart();
                }
                second
            }
            other => other,
        }
    }
}

impl Drop for SttClient {
    fn drop(&mut self) {
        if let Some(mut p) = self.proc.take() {
            let _ = p.send(&Request::Shutdown, None);
            std::thread::sleep(Duration::from_millis(50));
            p.kill();
        }
    }
}
