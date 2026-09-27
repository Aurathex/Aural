//! Graphics cards: which ones this PC has, and how much of their memory this process
//! is using.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

impl Vendor {
    /// From the PCI vendor id DXGI reports.
    pub fn from_pci(id: u32) -> Vendor {
        match id {
            0x10DE => Vendor::Nvidia,
            0x1002 | 0x1022 => Vendor::Amd,
            0x8086 => Vendor::Intel,
            _ => Vendor::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: Vendor,
    /// Dedicated video memory.
    pub vram_mb: u64,
    /// Built into the processor (shares the PC's RAM) rather than a separate card.
    pub integrated: bool,
    /// Display driver version, e.g. "32.0.15.6117"; empty if Windows didn't say.
    pub driver: String,
    /// Windows' id for this adapter in the current boot (not stable across reboots).
    pub luid: u64,
}

/// Integrated graphics: little dedicated memory, or an Intel part that leans on shared
/// memory more than its own.
pub fn is_integrated(vendor: Vendor, dedicated_mb: u64, shared_mb: u64) -> bool {
    dedicated_mb < 512 || (vendor == Vendor::Intel && shared_mb > dedicated_mb)
}

/// The graphics card to try first: the separate card with the most memory.
pub fn primary_discrete(gpus: &[GpuInfo]) -> Option<&GpuInfo> {
    gpus.iter()
        .filter(|g| !g.integrated)
        .max_by_key(|g| g.vram_mb)
}

/// The same physical card (the LUID changes per boot and per display path).
pub fn same_card(a: &GpuInfo, b: &GpuInfo) -> bool {
    a.name == b.name && a.vendor == b.vendor && a.vram_mb == b.vram_mb && a.driver == b.driver
}

/// DXGI packs the driver version into four 16-bit parts.
pub fn driver_version(umd: i64) -> String {
    let v = umd as u64;
    format!(
        "{}.{}.{}.{}",
        v >> 48,
        (v >> 32) & 0xFFFF,
        (v >> 16) & 0xFFFF,
        v & 0xFFFF
    )
}

#[cfg(windows)]
pub use win::{adapters, process_gpu_memory_mb};

#[cfg(windows)]
mod win {
    use super::{driver_version, is_integrated, same_card, GpuInfo, Vendor};
    use windows::core::Interface;
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIAdapter3, IDXGIDevice, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
        DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO,
    };

    const MB: u64 = 1024 * 1024;

    /// The real graphics adapters Windows knows about (software renderers skipped).
    pub fn adapters() -> Vec<GpuInfo> {
        let mut out = Vec::new();
        // SAFETY: plain DXGI queries on interfaces we own; no pointers escape.
        unsafe {
            let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
                return out;
            };
            let mut i = 0;
            while let Ok(adapter) = factory.EnumAdapters1(i) {
                i += 1;
                let Ok(desc) = adapter.GetDesc1() else {
                    continue;
                };
                if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                    continue;
                }
                let len = desc.Description.iter().position(|&c| c == 0).unwrap_or(128);
                let vendor = Vendor::from_pci(desc.VendorId);
                let vram_mb = desc.DedicatedVideoMemory as u64 / MB;
                let shared_mb = desc.SharedSystemMemory as u64 / MB;
                let driver = adapter
                    .CheckInterfaceSupport(&IDXGIDevice::IID)
                    .map(driver_version)
                    .unwrap_or_default();
                let luid = ((desc.AdapterLuid.HighPart as u32 as u64) << 32)
                    | desc.AdapterLuid.LowPart as u64;
                let info = GpuInfo {
                    name: String::from_utf16_lossy(&desc.Description[..len])
                        .trim()
                        .to_owned(),
                    vendor,
                    vram_mb,
                    integrated: is_integrated(vendor, vram_mb, shared_mb),
                    driver,
                    luid,
                };
                // Hybrid laptops list the built-in GPU once per display path.
                if !out.iter().any(|g| same_card(g, &info)) {
                    out.push(info);
                }
            }
        }
        out
    }

    /// Megabytes of graphics-card memory this process currently uses, summed over the
    /// real (non-software) adapters. `None` if Windows can't report it.
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
            answered.then_some(total / MB)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(name: &str, vendor: Vendor, vram_mb: u64, integrated: bool) -> GpuInfo {
        GpuInfo {
            name: name.into(),
            vendor,
            vram_mb,
            integrated,
            driver: String::new(),
            luid: 0,
        }
    }

    #[test]
    fn discrete_gpu_preferred_over_integrated() {
        let g = vec![
            gpu("Intel(R) Arc(TM) Graphics", Vendor::Intel, 128, true),
            gpu(
                "NVIDIA GeForce RTX 4070 Laptop GPU",
                Vendor::Nvidia,
                8188,
                false,
            ),
        ];
        assert_eq!(primary_discrete(&g).unwrap().vendor, Vendor::Nvidia);
    }

    #[test]
    fn no_discrete_gpu_means_none() {
        let g = vec![gpu("Intel(R) UHD Graphics", Vendor::Intel, 128, true)];
        assert!(primary_discrete(&g).is_none());
    }

    #[test]
    fn vendor_from_pci_id() {
        assert_eq!(Vendor::from_pci(0x10DE), Vendor::Nvidia);
        assert_eq!(Vendor::from_pci(0x1002), Vendor::Amd);
        assert_eq!(Vendor::from_pci(0x8086), Vendor::Intel);
        assert_eq!(Vendor::from_pci(0x1414), Vendor::Other);
    }

    #[test]
    fn integrated_by_memory() {
        assert!(is_integrated(Vendor::Intel, 128, 7_900));
        assert!(is_integrated(Vendor::Amd, 256, 7_900));
        assert!(!is_integrated(Vendor::Nvidia, 8_188, 7_900));
        // Intel Arc A770: 16 GB of its own memory is a separate card.
        assert!(!is_integrated(Vendor::Intel, 16_000, 7_900));
    }

    #[test]
    fn driver_version_unpacks_four_parts() {
        let v: i64 = (32 << 48) | (15 << 16) | 6117;
        assert_eq!(driver_version(v), "32.0.15.6117");
    }

    /// Lists this PC's graphics adapters. Run with `-- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn this_pc_has_its_gpus_listed() {
        let gpus = adapters();
        for g in &gpus {
            eprintln!("{g:?}");
        }
        assert!(!gpus.is_empty());
        assert!(gpus
            .iter()
            .all(|g| !g.name.is_empty() && !g.driver.is_empty()));
        // Hybrid laptops list the built-in GPU once per display path.
        for (i, g) in gpus.iter().enumerate() {
            assert!(
                !gpus[..i].iter().any(|o| same_card(o, g)),
                "listed twice: {g:?}"
            );
        }
    }

    #[test]
    fn same_card_ignores_the_per_boot_id() {
        let a = gpu("Intel(R) Arc(TM) Graphics", Vendor::Intel, 2048, true);
        let b = GpuInfo {
            luid: 99,
            ..a.clone()
        };
        assert!(same_card(&a, &b));
        let c = gpu(
            "NVIDIA GeForce RTX 4070 Laptop GPU",
            Vendor::Nvidia,
            8188,
            false,
        );
        assert!(!same_card(&a, &c));
    }

    /// Every Windows PC with a display driver reports a number (0 for a process that
    /// has put nothing on the GPU). Run with `-- --ignored` on real hardware.
    #[test]
    #[ignore]
    fn this_process_gpu_memory_is_readable() {
        let mb = process_gpu_memory_mb();
        assert!(mb.is_some(), "DXGI video-memory query failed");
    }
}
