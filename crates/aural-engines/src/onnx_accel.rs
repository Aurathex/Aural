//! Which device the ONNX engines run on, and proof that they really run there.
//!
//! transcribe-rs takes the device as a process-wide preference and, if the GPU provider
//! can't be used, logs a warning and quietly runs on the CPU. Aural never accepts that:
//! a model asked to run on the graphics card counts as running there only if nothing
//! logged a fall-back *and* the worker process actually holds graphics-card memory.

use crate::Backend;
use anyhow::{bail, Result};
use aural_platform::gpu::AdapterMemory;
use transcribe_rs::accel::{set_ort_accelerator, OrtAccelerator};

/// The ONNX Runtime device for a backend; ONNX models run on the processor or DirectML.
pub fn accelerator(backend: Backend) -> Result<OrtAccelerator> {
    match backend {
        Backend::Cpu => Ok(OrtAccelerator::CpuOnly),
        Backend::DirectMl => Ok(OrtAccelerator::DirectMl),
        other => bail!("ONNX models can't run on {other:?}; use the processor or DirectML"),
    }
}

/// Set the device before loading a model (transcribe-rs reads it when it creates the
/// model's sessions; one worker process runs one model at a time).
pub fn select(backend: Backend) -> Result<()> {
    set_ort_accelerator(accelerator(backend)?);
    Ok(())
}

/// Prove the model runs where it was asked to, from the load log and the worker's
/// graphics memory per adapter after loading. With a separate graphics card
/// (`card_luid`), DirectML must be using that card: on hybrid laptops its default device
/// can be the built-in graphics. Returns the backend in use, e.g. "directml:<card>".
pub fn verify(
    requested: Backend,
    log: &[String],
    used: &[AdapterMemory],
    card_luid: Option<u64>,
) -> Result<String> {
    match requested {
        Backend::Cpu => Ok("cpu".into()),
        Backend::DirectMl => {
            if let Some(line) = log.iter().find(|l| l.contains("falling back to CPU")) {
                bail!("DirectML was requested but the model fell back to the CPU: {line}");
            }
            if used.is_empty() {
                bail!("DirectML was requested but its graphics-card use couldn't be confirmed");
            }
            let Some(busiest) = used
                .iter()
                .filter(|a| a.used_mb > 0)
                .max_by_key(|a| a.used_mb)
            else {
                bail!(
                    "DirectML was requested but the model uses no graphics-card memory, so it ran on the CPU"
                );
            };
            if let Some(card) = card_luid {
                if !used.iter().any(|a| a.luid == card && a.used_mb > 0) {
                    bail!(
                        "DirectML ran on {} instead of the separate graphics card",
                        busiest.name
                    );
                }
                let name = used.iter().find(|a| a.luid == card).map_or("", |a| &a.name);
                return Ok(format!("directml:{name}"));
            }
            Ok(format!("directml:{}", busiest.name))
        }
        other => bail!("ONNX models can't run on {other:?}"),
    }
}

/// The separate graphics card's LUID on this PC, if there is one.
pub fn card_luid() -> Option<u64> {
    let gpus = aural_platform::gpu::adapters();
    aural_platform::gpu::primary_discrete(&gpus).map(|g| g.luid)
}
#[cfg(test)]
mod tests {
    use super::*;

    const RTX: u64 = 70_961_178;
    const ARC: u64 = 65_907;

    fn used(rtx_mb: u64, arc_mb: u64) -> Vec<AdapterMemory> {
        vec![
            AdapterMemory {
                index: 0,
                name: "NVIDIA GeForce RTX 4070 Laptop GPU".into(),
                luid: RTX,
                used_mb: rtx_mb,
            },
            AdapterMemory {
                index: 1,
                name: "Intel(R) Arc(TM) Graphics".into(),
                luid: ARC,
                used_mb: arc_mb,
            },
        ]
    }

    #[test]
    fn directml_counts_only_with_gpu_memory_in_use() {
        assert_eq!(
            verify(Backend::DirectMl, &[], &used(420, 0), Some(RTX)).unwrap(),
            "directml:NVIDIA GeForce RTX 4070 Laptop GPU"
        );
        assert!(
            verify(Backend::DirectMl, &[], &used(0, 0), Some(RTX)).is_err(),
            "no GPU memory means it ran on the CPU"
        );
        assert!(
            verify(Backend::DirectMl, &[], &[], Some(RTX)).is_err(),
            "no evidence is not success"
        );
    }

    #[test]
    fn directml_on_the_built_in_graphics_is_not_the_graphics_card() {
        // Hybrid laptops: DirectML's default device can be the built-in GPU, while
        // Aural's labels and memory checks are about the separate card.
        let err = verify(Backend::DirectMl, &[], &used(0, 380), Some(RTX)).unwrap_err();
        assert!(
            err.to_string().contains("Intel(R) Arc(TM) Graphics"),
            "{err}"
        );
    }

    #[test]
    fn without_a_separate_card_any_adapter_counts() {
        assert_eq!(
            verify(Backend::DirectMl, &[], &used(0, 380), None).unwrap(),
            "directml:Intel(R) Arc(TM) Graphics"
        );
    }

    #[test]
    fn a_logged_fallback_is_an_error_even_with_gpu_memory() {
        let log = vec![
            "Accelerator set to DirectML but ort-directml feature is not enabled; falling back to CPU"
                .to_owned(),
        ];
        let err = verify(Backend::DirectMl, &log, &used(500, 0), Some(RTX)).unwrap_err();
        assert!(err.to_string().contains("CPU"), "{err}");
    }

    #[test]
    fn the_cpu_needs_no_proof() {
        assert_eq!(verify(Backend::Cpu, &[], &[], None).unwrap(), "cpu");
    }

    #[test]
    fn onnx_models_only_run_on_the_processor_or_directml() {
        assert!(accelerator(Backend::Cpu).is_ok());
        assert!(accelerator(Backend::DirectMl).is_ok());
        assert!(accelerator(Backend::Vulkan).is_err());
        assert!(accelerator(Backend::Cuda).is_err());
    }
}
