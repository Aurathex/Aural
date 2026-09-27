//! Graphics cards: how much of their memory this process is using. (Adapter
//! enumeration and the hardware fingerprint join this module in a later task.)

use windows::core::Interface;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter3, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
    DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO,
};

/// Megabytes of graphics-card memory this process currently uses, summed over the real
/// (non-software) adapters. `None` if Windows can't report it.
pub fn process_gpu_memory_mb() -> Option<u64> {
    // SAFETY: plain DXGI queries on interfaces we own; no pointers escape.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut total: u64 = 0;
        let mut answered = false;
        let mut i = 0;
        while let Ok(adapter) = factory.EnumAdapters1(i) {
            i += 1;
            let Ok(desc) = adapter.GetDesc1() else {
                continue;
            };
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let Ok(a3) = adapter.cast::<IDXGIAdapter3>() else {
                continue;
            };
            let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            if a3
                .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
                .is_ok()
            {
                total += info.CurrentUsage;
                answered = true;
            }
        }
        answered.then_some(total / (1024 * 1024))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every Windows PC with a display driver reports a number (0 for a process that
    /// has put nothing on the GPU). Run with `-- --ignored` on real hardware.
    #[test]
    #[ignore]
    fn this_process_gpu_memory_is_readable() {
        let mb = process_gpu_memory_mb();
        assert!(mb.is_some(), "DXGI video-memory query failed");
    }
}
