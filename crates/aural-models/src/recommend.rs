//! Which model suits this PC. Recommendation never blocks choosing another compatible
//! model; incompatibility only comes from not enough RAM.

use crate::catalog::{Catalog, ModelEntry};
use aural_platform::gpu::GpuInfo;
use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct HardwareProfile {
    pub cpu_name: String,
    pub total_ram_mb: u64,
    /// Free when detected; changes all the time, so it is not part of the fingerprint.
    pub free_ram_mb: u64,
    pub logical_cores: usize,
    pub physical_cores: usize,
    pub avx2: bool,
    pub avx512: bool,
    pub gpus: Vec<GpuInfo>,
}

impl HardwareProfile {
    /// Probe this machine (RAM via the OS, cores via std/Windows, CPU features via
    /// cpuid, graphics cards via DXGI).
    pub fn detect() -> Self {
        let (total_ram_mb, free_ram_mb) = ram_mb();
        Self {
            cpu_name: aural_platform::cpu::name(),
            total_ram_mb,
            free_ram_mb,
            logical_cores: std::thread::available_parallelism().map_or(1, |n| n.get()),
            physical_cores: aural_platform::cpu::physical_cores(),
            avx2: cpu_feature("avx2"),
            avx512: cpu_feature("avx512f"),
            gpus: gpus(),
        }
    }

    /// Identifies "the same PC" for saved test results: processor, cores, RAM size to
    /// the GB, and each graphics card with its driver version and memory. A new driver
    /// or card changes it, so results are measured again.
    pub fn fingerprint(&self) -> String {
        let mut gpus: Vec<String> = self
            .gpus
            .iter()
            .map(|g| format!("{}/{}/{}MB", g.name, g.driver, g.vram_mb))
            .collect();
        gpus.sort();
        format!(
            "cpu={};cores={}/{};ram={}GB;gpus=[{}]",
            self.cpu_name,
            self.physical_cores,
            self.logical_cores,
            (self.total_ram_mb + 512) / 1024,
            gpus.join(";")
        )
    }
}

#[cfg(windows)]
fn gpus() -> Vec<GpuInfo> {
    aural_platform::gpu::adapters()
}

#[cfg(not(windows))]
fn gpus() -> Vec<GpuInfo> {
    Vec::new()
}

#[cfg(target_arch = "x86_64")]
fn cpu_feature(name: &str) -> bool {
    match name {
        "avx2" => std::arch::is_x86_feature_detected!("avx2"),
        "avx512f" => std::arch::is_x86_feature_detected!("avx512f"),
        _ => false,
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn cpu_feature(_: &str) -> bool {
    false
}
#[cfg(windows)]
/// (total, free) physical memory in MB.
fn ram_mb() -> (u64, u64) {
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }
    let mut s = MemoryStatusEx {
        length: std::mem::size_of::<MemoryStatusEx>() as u32,
        memory_load: 0,
        total_phys: 0,
        avail_phys: 0,
        total_page_file: 0,
        avail_page_file: 0,
        total_virtual: 0,
        avail_virtual: 0,
        avail_extended_virtual: 0,
    };
    // SAFETY: `s` is a correctly sized MEMORYSTATUSEX with dwLength set.
    if unsafe { GlobalMemoryStatusEx(&mut s) } != 0 {
        (s.total_phys / (1024 * 1024), s.avail_phys / (1024 * 1024))
    } else {
        (0, 0)
    }
}

#[cfg(not(windows))]
fn ram_mb() -> (u64, u64) {
    (0, 0)
}

pub fn compatible(entry: &ModelEntry, hw: &HardwareProfile) -> bool {
    hw.total_ram_mb >= entry.min_ram_mb()
}

/// Parakeet needs a reasonably modern CPU to feel instant; older or smaller machines
/// get Whisper small.en, and very low-memory machines get base.en.
pub fn recommend<'a>(catalog: &'a Catalog, hw: &HardwareProfile) -> Option<&'a str> {
    let modern = hw.total_ram_mb >= 8_000 && hw.logical_cores >= 4 && hw.avx2;
    let preferred: &[&str] = if modern {
        &[
            "parakeet-tdt-0.6b-v2-int8",
            "whisper-small.en-q8",
            "whisper-base.en-q8",
        ]
    } else if hw.total_ram_mb >= 4_000 {
        &["whisper-small.en-q8", "whisper-base.en-q8"]
    } else {
        &["whisper-base.en-q8"]
    };
    preferred
        .iter()
        .filter_map(|id| catalog.get(id))
        .find(|m| compatible(m, hw))
        .map(|m| m.id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use aural_platform::gpu::{GpuInfo, Vendor};

    fn hw(ram: u64, cores: usize, avx2: bool) -> HardwareProfile {
        HardwareProfile {
            total_ram_mb: ram,
            logical_cores: cores,
            avx2,
            ..HardwareProfile::default()
        }
    }

    fn laptop() -> HardwareProfile {
        HardwareProfile {
            cpu_name: "Intel(R) Core(TM) Ultra 9 185H".into(),
            total_ram_mb: 15_770,
            free_ram_mb: 6_000,
            logical_cores: 22,
            physical_cores: 16,
            avx2: true,
            avx512: false,
            gpus: vec![GpuInfo {
                name: "NVIDIA GeForce RTX 4070 Laptop GPU".into(),
                vendor: Vendor::Nvidia,
                vram_mb: 7_948,
                integrated: false,
                driver: "32.0.16.1714".into(),
                luid: 1,
            }],
        }
    }

    #[test]
    fn fingerprint_changes_with_driver_but_not_free_ram() {
        let a = laptop();
        let mut b = laptop();
        b.free_ram_mb = 1_200;
        b.gpus[0].luid = 7;
        assert_eq!(a.fingerprint(), b.fingerprint());
        b.gpus[0].driver = "32.0.16.1800".into();
        assert_ne!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn fingerprint_changes_with_ram_size_and_gpus() {
        let a = laptop();
        let mut more_ram = laptop();
        more_ram.total_ram_mb = 31_900;
        assert_ne!(a.fingerprint(), more_ram.fingerprint());
        let mut no_gpu = laptop();
        no_gpu.gpus.clear();
        assert_ne!(a.fingerprint(), no_gpu.fingerprint());
        // RAM reported a few MB differently after a driver update is the same PC.
        let mut jitter = laptop();
        jitter.total_ram_mb = 15_790;
        assert_eq!(a.fingerprint(), jitter.fingerprint());
    }

    #[test]
    fn detect_finds_this_pc() {
        let hw = HardwareProfile::detect();
        assert!(hw.total_ram_mb > 0 && hw.free_ram_mb <= hw.total_ram_mb);
        assert!(hw.physical_cores >= 1 && hw.physical_cores <= hw.logical_cores);
        assert!(!hw.fingerprint().is_empty());
    }

    #[test]
    fn modern_pc_gets_parakeet() {
        assert_eq!(
            recommend(&Catalog::builtin(), &hw(16_000, 8, true)),
            Some("parakeet-tdt-0.6b-v2-int8")
        );
    }

    #[test]
    fn low_ram_or_no_avx2_or_few_cores_gets_small_whisper() {
        let c = Catalog::builtin();
        assert_eq!(
            recommend(&c, &hw(6_000, 8, true)),
            Some("whisper-small.en-q8")
        );
        assert_eq!(
            recommend(&c, &hw(16_000, 8, false)),
            Some("whisper-small.en-q8")
        );
        assert_eq!(
            recommend(&c, &hw(16_000, 2, true)),
            Some("whisper-small.en-q8")
        );
    }

    #[test]
    fn very_low_ram_gets_base_whisper() {
        assert_eq!(
            recommend(&Catalog::builtin(), &hw(3_000, 2, false)),
            Some("whisper-base.en-q8")
        );
    }

    #[test]
    fn compatibility_is_about_ram_only() {
        let c = Catalog::builtin();
        let p = c.get("parakeet-tdt-0.6b-v2-int8").unwrap();
        assert!(compatible(p, &hw(4_096, 2, false)));
        assert!(!compatible(p, &hw(1_024, 16, true)));
    }
}
