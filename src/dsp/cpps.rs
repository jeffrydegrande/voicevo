use rustfft::{num_complex::Complex, FftPlanner};

use super::windowing;

/// Configuration for Cepstral Peak Prominence Smoothed (CPPS) computation.
pub struct CppsConfig {
    /// Analysis window duration in milliseconds.
    pub frame_size_ms: f32,
    /// Hop between frames in milliseconds.
    pub hop_size_ms: f32,
    /// Minimum quefrency in milliseconds (corresponds to max frequency).
    /// 2.5ms = 400 Hz upper bound.
    pub quefrency_min_ms: f32,
    /// Maximum quefrency in milliseconds (corresponds to min frequency).
    /// 16.7ms = 60 Hz lower bound.
    pub quefrency_max_ms: f32,
    /// Energy gate: frames below this RMS (dB) are excluded.
    pub energy_gate_db: f32,
}

impl Default for CppsConfig {
    fn default() -> Self {
        Self {
            frame_size_ms: 40.0,
            hop_size_ms: 10.0,
            quefrency_min_ms: 2.5,
            quefrency_max_ms: 16.7,
            energy_gate_db: -45.0,
        }
    }
}

/// Compute CPPS (Cepstral Peak Prominence Smoothed) for an audio signal.
///
/// CPPS measures the strength of the cepstral peak relative to the overall
/// cepstral shape. It's a robust indicator of voice quality that doesn't
/// require successful pitch tracking — making it ideal for damaged voices.
///
/// Algorithm per frame:
/// 1. Hanning window
/// 2. FFT -> power spectrum -> log power spectrum
/// 3. IFFT of log power spectrum -> cepstrum
/// 4. Find peak in quefrency range [quefrency_min, quefrency_max]
/// 5. Fit linear regression to cepstrum in that range
/// 6. CPPS = peak value - regression value at peak quefrency
///
/// Returns the mean CPPS across all energy-gated frames, or None if no
/// frames pass the energy gate.
///
/// Clinical reference: normal ~5-10 dB, < 3 dB = significant dysphonia.
pub fn compute_cpps(samples: &[f32], sample_rate: u32, config: &CppsConfig) -> Option<f32> {
    let sr = sample_rate as f32;
    let frame_size = (config.frame_size_ms / 1000.0 * sr) as usize;
    let hop_size = (config.hop_size_ms / 1000.0 * sr) as usize;

    if frame_size == 0 || samples.len() < frame_size {
        return None;
    }

    // FFT size: next power of 2 for efficiency
    let fft_size = frame_size.next_power_of_two();

    let mut planner = FftPlanner::new();
    let fft_forward = planner.plan_fft_forward(fft_size);
    let fft_inverse = planner.plan_fft_inverse(fft_size);

    // Quefrency range in samples
    let q_min = (config.quefrency_min_ms / 1000.0 * sr) as usize;
    let q_max = (config.quefrency_max_ms / 1000.0 * sr).ceil() as usize;
    let q_max = q_max.min(fft_size / 2);

    if q_min >= q_max || q_max >= fft_size / 2 {
        return None;
    }

    let mut cpp_values = Vec::new();
    let mut pos = 0;

    while pos + frame_size <= samples.len() {
        let frame = &samples[pos..pos + frame_size];

        // Energy gate
        let rms = frame_rms(frame);
        let rms_db = if rms > 0.0 {
            20.0 * rms.log10()
        } else {
            f32::NEG_INFINITY
        };

        if rms_db < config.energy_gate_db {
            pos += hop_size;
            continue;
        }

        // Step 1: Hanning window
        let windowed = windowing::hanning(frame);

        // Step 2: FFT -> power spectrum -> log power spectrum
        let mut fft_buf: Vec<Complex<f32>> = windowed
            .iter()
            .map(|&s| Complex::new(s, 0.0))
            .collect();
        // Zero-pad to fft_size
        fft_buf.resize(fft_size, Complex::new(0.0, 0.0));

        fft_forward.process(&mut fft_buf);

        // Power spectrum in dB: 10*log10(|X|^2)
        let mut log_power: Vec<Complex<f32>> = fft_buf
            .iter()
            .map(|c| {
                let power = c.norm_sqr();
                let log_p = if power > 1e-20 {
                    10.0 * power.log10()
                } else {
                    -200.0 // floor
                };
                Complex::new(log_p, 0.0)
            })
            .collect();

        // Step 3: IFFT of log power spectrum -> cepstrum
        fft_inverse.process(&mut log_power);

        // Normalize IFFT output
        let norm = 1.0 / fft_size as f32;
        let cepstrum: Vec<f32> = log_power.iter().map(|c| c.re * norm).collect();

        // Step 4: Find peak in quefrency range
        if let Some(cpp) = cepstral_peak_prominence(&cepstrum, q_min, q_max) {
            cpp_values.push(cpp);
        }

        pos += hop_size;
    }

    if cpp_values.is_empty() {
        return None;
    }

    // Mean CPPS across frames
    let mean = cpp_values.iter().sum::<f32>() / cpp_values.len() as f32;
    Some(mean)
}

/// Compute the cepstral peak prominence for one frame's cepstrum.
///
/// Finds the maximum cepstral value in the quefrency range, then subtracts
/// the linear regression value at that quefrency to get the prominence.
fn cepstral_peak_prominence(cepstrum: &[f32], q_min: usize, q_max: usize) -> Option<f32> {
    if q_min >= q_max || q_max >= cepstrum.len() {
        return None;
    }

    // Find peak in quefrency range
    let mut peak_val = f32::NEG_INFINITY;
    let mut peak_idx = q_min;

    for i in q_min..=q_max {
        if cepstrum[i] > peak_val {
            peak_val = cepstrum[i];
            peak_idx = i;
        }
    }

    // Linear regression of cepstrum in the quefrency range
    let n = (q_max - q_min + 1) as f32;
    let mut sum_x = 0.0f32;
    let mut sum_y = 0.0f32;
    let mut sum_xy = 0.0f32;
    let mut sum_xx = 0.0f32;

    for i in q_min..=q_max {
        let x = i as f32;
        let y = cepstrum[i];
        sum_x += x;
        sum_y += y;
        sum_xy += x * y;
        sum_xx += x * x;
    }

    let denom = n * sum_xx - sum_x * sum_x;
    if denom.abs() < 1e-10 {
        return Some(0.0);
    }

    let slope = (n * sum_xy - sum_x * sum_y) / denom;
    let intercept = (sum_y - slope * sum_x) / n;

    // Regression value at peak quefrency
    let regression_at_peak = slope * peak_idx as f32 + intercept;

    // CPP = peak - regression
    Some(peak_val - regression_at_peak)
}

fn frame_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn sine_wave(freq_hz: f32, sample_rate: u32, duration_secs: f32) -> Vec<f32> {
        let num_samples = (sample_rate as f32 * duration_secs) as usize;
        (0..num_samples)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                0.5 * (2.0 * PI * freq_hz * t).sin()
            })
            .collect()
    }

    #[test]
    fn pure_tone_high_cpps() {
        // A pure sine wave has a strong cepstral peak -> high CPPS
        let samples = sine_wave(100.0, 44100, 1.0);
        let cpps = compute_cpps(&samples, 44100, &CppsConfig::default());
        assert!(cpps.is_some(), "Should compute CPPS for a sine wave");
        let val = cpps.unwrap();
        assert!(
            val > 0.5,
            "Pure tone should have positive CPPS, got {val:.2}"
        );
    }

    #[test]
    fn noise_low_cpps() {
        // White noise has no cepstral peak -> low CPPS
        let sr = 44100u32;
        let n = (sr as f32 * 1.0) as usize;
        let mut rng_state: u32 = 42;
        let samples: Vec<f32> = (0..n)
            .map(|_| {
                rng_state = rng_state.wrapping_mul(1103515245).wrapping_add(12345);
                0.5 * ((rng_state as f32 / u32::MAX as f32) * 2.0 - 1.0)
            })
            .collect();

        let cpps = compute_cpps(&samples, sr, &CppsConfig::default());
        assert!(cpps.is_some(), "Should compute CPPS for noise");
        let val = cpps.unwrap();
        // Noise CPPS should be much lower than periodic signal
        assert!(
            val < 1.0,
            "White noise should have low CPPS, got {val:.2}"
        );
    }

    #[test]
    fn silence_returns_none() {
        // All-zero signal should be gated out
        let samples = vec![0.0; 44100];
        let cpps = compute_cpps(&samples, 44100, &CppsConfig::default());
        assert!(cpps.is_none(), "Silence should return None (all frames gated)");
    }

    #[test]
    fn tone_higher_than_noise() {
        // Verify that a periodic signal has higher CPPS than noise
        let tone = sine_wave(100.0, 44100, 1.0);
        let tone_cpps = compute_cpps(&tone, 44100, &CppsConfig::default()).unwrap();

        let mut rng_state: u32 = 42;
        let noise: Vec<f32> = (0..44100)
            .map(|_| {
                rng_state = rng_state.wrapping_mul(1103515245).wrapping_add(12345);
                0.5 * ((rng_state as f32 / u32::MAX as f32) * 2.0 - 1.0)
            })
            .collect();
        let noise_cpps = compute_cpps(&noise, 44100, &CppsConfig::default()).unwrap();

        assert!(
            tone_cpps > noise_cpps,
            "Tone CPPS ({tone_cpps:.2}) should exceed noise CPPS ({noise_cpps:.2})"
        );
    }
}
