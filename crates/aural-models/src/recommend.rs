//! Which model suits this PC. Recommendation never blocks choosing another compatible
//! model; incompatibility only comes from not enough RAM.

use crate::catalog::{Catalog, ModelEntry};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HardwareProfile {
    pub total_ram_mb: u64,
    pub logical_cores: usize,
    pub avx2: bool,
}

impl HardwareProfile {
    /// Probe this machine (RAM via the OS, cores via std, AVX2 via cpuid).
    pub fn detect() -> Self {
        Self {
            total_ram_mb: total_ram_mb(),
            logical_cores: std::thread::available_parallelism().map_or(1, |n| n.get()),
            avx2: avx2(),
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn avx2() -> bool {
    std::arch::is_x86_feature_detected!("avx2")
}

#[cfg(not(target_arch = "x86_64"))]
fn avx2() -> bool {
    false
}

#[cfg(windows)]
fn total_ram_mb() -> u64 {
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
        s.total_phys / (1024 * 1024)
    } else {
        0
    }
}

#[cfg(not(windows))]
fn total_ram_mb() -> u64 {
    0
}

pub fn compatible(entry: &ModelEntry, hw: &HardwareProfile) -> bool {
    hw.total_ram_mb >= entry.min_ram_mb
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

    fn hw(ram: u64, cores: usize, avx2: bool) -> HardwareProfile {
        HardwareProfile {
            total_ram_mb: ram,
            logical_cores: cores,
            avx2,
        }
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
