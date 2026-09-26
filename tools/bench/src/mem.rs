//! Process memory probe for benchmark reports. VRAM is read manually (nvidia-smi /
//! Task Manager) during runs; this only covers system RAM.

use anyhow::{bail, Result};
use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

/// Peak working set of this process in MiB.
pub fn peak_working_set_mb() -> Result<f64> {
    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing, and
    // `counters` is a correctly sized, writable PROCESS_MEMORY_COUNTERS.
    let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    if ok == 0 {
        bail!(
            "GetProcessMemoryInfo failed: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(counters.PeakWorkingSetSize as f64 / (1024.0 * 1024.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_working_set_grows_after_touching_memory() {
        let before = peak_working_set_mb().unwrap();
        let big = vec![1u8; 64 * 1024 * 1024];
        std::hint::black_box(&big);
        let after = peak_working_set_mb().unwrap();
        assert!(before > 0.0);
        assert!(after >= before + 50.0, "before {before} after {after}");
    }
}
