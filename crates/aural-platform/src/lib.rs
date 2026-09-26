//! Windows integration for Aural: global hotkey, text insertion into the focused app,
//! start-with-Windows and microphone privacy checks.

pub mod autostart;
pub mod chord;
pub mod consent;
pub mod insert;

#[cfg(windows)]
pub mod hook;
