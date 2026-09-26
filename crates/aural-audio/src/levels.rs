//! Real microphone level bands for the listening pill: a short FFT folded into 12
//! log-spaced bands between 90 Hz and 4 kHz (where speech lives), mapped to 0..1.

use realfft::{RealFftPlanner, RealToComplex};
use std::sync::Arc;

pub const BANDS: usize = 12;
pub const WINDOW: usize = 1024;
const LOW_HZ: f32 = 90.0;
const HIGH_HZ: f32 = 4_000.0;
/// dB range mapped onto 0..1: quiet speech sits around -45 dBFS, loud near -6.
const FLOOR_DB: f32 = -60.0;
const CEIL_DB: f32 = -6.0;

pub struct LevelAnalyzer {
    rate: u32,
    fft: Arc<dyn RealToComplex<f32>>,
    hann: Vec<f32>,
    input: Vec<f32>,
    spectrum: Vec<realfft::num_complex::Complex<f32>>,
}

impl LevelAnalyzer {
    pub fn new(rate: u32) -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(WINDOW);
        let hann = (0..WINDOW)
            .map(|n| 0.5 - 0.5 * (std::f32::consts::TAU * n as f32 / (WINDOW as f32 - 1.0)).cos())
            .collect();
        let input = fft.make_input_vec();
        let spectrum = fft.make_output_vec();
        Self {
            rate,
            fft,
            hann,
            input,
            spectrum,
        }
    }

    /// Frequency range of band `i` in Hz (geometric spacing).
    pub fn band_range_hz(&self, i: usize) -> (f32, f32) {
        let ratio = HIGH_HZ / LOW_HZ;
        let edge = |k: usize| LOW_HZ * ratio.powf(k as f32 / BANDS as f32);
        (edge(i), edge(i + 1))
    }

    /// Levels of the most recent `WINDOW` samples (zero-padded if shorter).
    pub fn analyze(&mut self, samples: &[f32]) -> [f32; BANDS] {
        let tail = &samples[samples.len().saturating_sub(WINDOW)..];
        self.input.fill(0.0);
        for (i, s) in tail.iter().enumerate() {
            self.input[i] = s * self.hann[i];
        }
        if self
            .fft
            .process(&mut self.input, &mut self.spectrum)
            .is_err()
        {
            return [0.0; BANDS];
        }
        // A full-scale sine windowed by Hann peaks at N/4 in its bin.
        let norm = WINDOW as f32 / 4.0;
        let bin_hz = self.rate as f32 / WINDOW as f32;
        let last_bin = self.spectrum.len() - 1;
        let mut out = [0.0; BANDS];
        for (i, level) in out.iter_mut().enumerate() {
            let (lo, hi) = self.band_range_hz(i);
            let first = ((lo / bin_hz).floor() as usize).min(last_bin);
            let end = ((hi / bin_hz).ceil() as usize).clamp(first + 1, last_bin + 1);
            let peak = self.spectrum[first..end]
                .iter()
                .map(|c| c.norm() / norm)
                .fold(0.0f32, f32::max);
            let db = 20.0 * peak.max(1e-9).log10();
            *level = ((db - FLOOR_DB) / (CEIL_DB - FLOOR_DB)).clamp(0.0, 1.0);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, amp: f32, rate: u32) -> Vec<f32> {
        (0..WINDOW)
            .map(|n| amp * (std::f32::consts::TAU * freq * n as f32 / rate as f32).sin())
            .collect()
    }

    #[test]
    fn silence_is_all_near_zero() {
        let mut a = LevelAnalyzer::new(16_000);
        let bands = a.analyze(&vec![0.0; WINDOW]);
        assert!(bands.iter().all(|b| *b < 0.02), "{bands:?}");
    }

    #[test]
    fn a_tone_lights_the_band_that_contains_it() {
        let mut a = LevelAnalyzer::new(48_000);
        let bands = a.analyze(&sine(1_000.0, 0.5, 48_000));
        let loudest = (0..BANDS)
            .max_by(|&i, &j| bands[i].total_cmp(&bands[j]))
            .unwrap();
        let (lo, hi) = a.band_range_hz(loudest);
        assert!(
            lo <= 1_000.0 && 1_000.0 < hi,
            "loudest band {loudest}: {lo}-{hi} Hz"
        );
        assert!(bands[loudest] > 0.5, "{bands:?}");
    }

    #[test]
    fn levels_stay_in_unit_range_even_for_clipped_input() {
        let mut a = LevelAnalyzer::new(16_000);
        let square: Vec<f32> = (0..WINDOW)
            .map(|n| if n % 16 < 8 { 1.0 } else { -1.0 })
            .collect();
        assert!(a.analyze(&square).iter().all(|b| (0.0..=1.0).contains(b)));
    }

    #[test]
    fn short_input_is_zero_padded_not_a_panic() {
        let mut a = LevelAnalyzer::new(16_000);
        assert_eq!(a.analyze(&[0.1; 100]).len(), BANDS);
    }

    #[test]
    fn bands_are_log_spaced_and_cover_speech() {
        let a = LevelAnalyzer::new(16_000);
        assert_eq!(a.band_range_hz(0).0, 90.0);
        assert!((a.band_range_hz(BANDS - 1).1 - 4_000.0).abs() < 1.0);
        let w0 = a.band_range_hz(0).1 - a.band_range_hz(0).0;
        let wl = a.band_range_hz(BANDS - 1).1 - a.band_range_hz(BANDS - 1).0;
        assert!(wl > w0 * 10.0);
    }
}
