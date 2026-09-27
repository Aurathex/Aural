//! The processor: its name and how many physical cores it has.

/// The processor's marketing name from CPUID, e.g. "Intel(R) Core(TM) Ultra 9 185H";
/// empty if the processor doesn't report one.
pub fn name() -> String {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::__cpuid;
        let max = __cpuid(0x8000_0000).eax;
        if max < 0x8000_0004 {
            return String::new();
        }
        let mut bytes = Vec::with_capacity(48);
        for leaf in 0x8000_0002u32..=0x8000_0004 {
            let r = __cpuid(leaf);
            for reg in [r.eax, r.ebx, r.ecx, r.edx] {
                bytes.extend_from_slice(&reg.to_le_bytes());
            }
        }
        brand_string(&bytes)
    }
    #[cfg(not(target_arch = "x86_64"))]
    String::new()
}

/// CPUID's brand bytes: NUL-padded, sometimes with leading spaces.
pub fn brand_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_owned()
}

/// Physical processor cores (not hyper-threads); falls back to the logical count.
#[cfg(windows)]
pub fn physical_cores() -> usize {
    use windows::Win32::System::SystemInformation::{
        GetLogicalProcessorInformation, RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION,
    };
    let logical = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut len: u32 = 0;
    // SAFETY: the first call only reports the buffer size needed.
    let _ = unsafe { GetLogicalProcessorInformation(None, &mut len) };
    let item = std::mem::size_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION>();
    if len == 0 {
        return logical;
    }
    let mut buf = vec![SYSTEM_LOGICAL_PROCESSOR_INFORMATION::default(); len as usize / item + 1];
    // SAFETY: `buf` holds at least `len` bytes of correctly typed entries.
    if unsafe { GetLogicalProcessorInformation(Some(buf.as_mut_ptr()), &mut len) }.is_err() {
        return logical;
    }
    let cores = buf[..len as usize / item]
        .iter()
        .filter(|e| e.Relationship == RelationProcessorCore)
        .count();
    if cores == 0 {
        logical
    } else {
        cores
    }
}

#[cfg(not(windows))]
pub fn physical_cores() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_string_is_trimmed_at_the_first_nul() {
        let mut b = b"  Intel(R) Core(TM) Ultra 9 185H".to_vec();
        b.resize(48, 0);
        assert_eq!(brand_string(&b), "Intel(R) Core(TM) Ultra 9 185H");
    }

    #[test]
    fn this_pc_reports_a_name_and_cores() {
        assert!(!name().is_empty());
        let logical = std::thread::available_parallelism().unwrap().get();
        let physical = physical_cores();
        assert!(
            physical >= 1 && physical <= logical,
            "{physical} of {logical}"
        );
    }
}
