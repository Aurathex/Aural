//! Which device the ONNX engines run on, and proof that they really run there.
//!
//! transcribe-rs takes the device as a process-wide preference and, if the GPU provider
//! can't be used, logs a warning and quietly runs on the CPU. Aural never accepts that:
//! a model asked to run on the graphics card counts as running there only if nothing
//! logged a fall-back *and* the worker process actually holds graphics-card memory.

use crate::Backend;
use anyhow::{bail, Result};
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
/// graphics-card memory after loading. Returns the backend actually in use.
pub fn verify(requested: Backend, log: &[String], gpu_memory_mb: Option<u64>) -> Result<String> {
    match requested {
        Backend::Cpu => Ok("cpu".into()),
        Backend::DirectMl => {
            if let Some(line) = log.iter().find(|l| l.contains("falling back to CPU")) {
                bail!("DirectML was requested but the model fell back to the CPU: {line}");
            }
            match gpu_memory_mb {
                Some(mb) if mb > 0 => Ok("directml".into()),
                Some(_) => bail!(
                    "DirectML was requested but the model uses no graphics-card memory, so it ran on the CPU"
                ),
                None => bail!("DirectML was requested but its graphics-card use couldn't be confirmed"),
            }
        }
        other => bail!("ONNX models can't run on {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directml_counts_only_with_gpu_memory_in_use() {
        assert_eq!(
            verify(Backend::DirectMl, &[], Some(420)).unwrap(),
            "directml"
        );
        assert!(
            verify(Backend::DirectMl, &[], Some(0)).is_err(),
            "no GPU memory means it ran on the CPU"
        );
        assert!(
            verify(Backend::DirectMl, &[], None).is_err(),
            "no evidence is not success"
        );
    }

    #[test]
    fn a_logged_fallback_is_an_error_even_with_gpu_memory() {
        let log = vec![
            "Accelerator set to DirectML but ort-directml feature is not enabled; falling back to CPU"
                .to_owned(),
        ];
        let err = verify(Backend::DirectMl, &log, Some(500)).unwrap_err();
        assert!(err.to_string().contains("CPU"), "{err}");
    }

    #[test]
    fn the_cpu_needs_no_proof() {
        assert_eq!(verify(Backend::Cpu, &[], None).unwrap(), "cpu");
    }

    #[test]
    fn onnx_models_only_run_on_the_processor_or_directml() {
        assert!(accelerator(Backend::Cpu).is_ok());
        assert!(accelerator(Backend::DirectMl).is_ok());
        assert!(accelerator(Backend::Vulkan).is_err());
        assert!(accelerator(Backend::Cuda).is_err());
    }
}
