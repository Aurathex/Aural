//! The app side: spawn a worker, load a model, transcribe with a timeout. A worker that
//! crashes or hangs is replaced, the model reloaded and the request retried once, so a
//! driver hiccup costs the user a second, not their dictation.

use crate::codec::{read_msg, write_msg, write_pcm};
use crate::msg::{Request, Response};
use crate::PROTOCOL_VERSION;
use aural_engines::{Backend, Engine};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

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

pub struct SttClient {
    spec: WorkerSpec,
    proc: Option<Proc>,
    loaded: Option<LoadArgs>,
    label: Option<String>,
    backend: Option<String>,
    restarts: u32,
    next_id: u64,
}

impl SttClient {
    pub fn spawn(spec: WorkerSpec) -> Result<Self, ClientError> {
        let proc = Proc::start(&spec)?;
        Ok(Self {
            spec,
            proc: Some(proc),
            loaded: None,
            label: None,
            backend: None,
            restarts: 0,
            next_id: 1,
        })
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
        self.send_load(&args)?;
        self.loaded = Some(args);
        Ok(())
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
        self.restarts += 1;
        self.proc = Some(Proc::start(&self.spec)?);
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

    /// Transcribe 16 kHz mono audio. On a crash or timeout the worker is replaced, the
    /// model reloaded and the request retried once.
    pub fn transcribe(&mut self, pcm: &[f32], timeout: Duration) -> Result<String, ClientError> {
        if self.loaded.is_none() {
            return Err(ClientError::NotLoaded);
        }
        match self.attempt(pcm, timeout) {
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
