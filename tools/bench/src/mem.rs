//! Process memory probe for benchmark reports (system RAM; graphics memory comes from
//! aural_platform::gpu).

use anyhow::{bail, Result};
use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

/// Peak working set of this process in MiB.
pub fn peak_working_set_mb() -> Result<f64> {
    Ok(counters()?.PeakWorkingSetSize as f64 / (1024.0 * 1024.0))
}

/// Current working set of this process in MiB.
pub fn current_working_set_mb() -> Result<f64> {
    Ok(counters()?.WorkingSetSize as f64 / (1024.0 * 1024.0))
}

fn counters() -> Result<PROCESS_MEMORY_COUNTERS> {
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
    Ok(counters)
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test on purpose: process-wide counters are skewed by other tests allocating on
    // parallel threads, so every comparison here must hold regardless of test order.
    #[test]
    fn working_set_tracks_touched_memory_and_peak_never_drops() {
        let big = vec![1u8; 256 * 1024 * 1024];
        std::hint::black_box(&big);
        let during = current_working_set_mb().unwrap();
        let peak_during = peak_working_set_mb().unwrap();
        assert!(
            during > 256.0,
            "working set {during} MB while holding 256 MB"
        );
        assert!(peak_during >= during);
        drop(big);
        let after = current_working_set_mb().unwrap();
        assert!(after < during - 128.0, "during {during} after {after}");
        assert!(peak_working_set_mb().unwrap() >= peak_during);
    }
}
