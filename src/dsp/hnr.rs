use super::pitch::PitchFrame;

/// Compute Harmonic-to-Noise Ratio using the autocorrelation method.
///
/// HNR quantifies breathiness — the ratio of harmonic (periodic) energy
/// to noise (aperiodic) energy. This is the method Praat uses (Boersma, 1993).
///
/// For each voiced frame:
///   1. We know the pitch period T0 = 1/F0 from the pitch contour
///   2. Extract a chunk of audio around this frame
///   3. Compute the normalized autocorrelation at lag T0
///   4. The autocorrelation value r(T0) tells us how periodic the signal is:
///      - r = 1.0 means perfectly periodic (pure tone, no noise)
///      - r = 0.0 means completely random (pure noise)
///   5. HNR = 10 * log10(r / (1 - r)) dB
///
/// We average HNR over all voiced frames.
///
/// Returns None if there are no voiced frames with valid HNR.
///
/// Clinical thresholds:
///   Normal voice: > 20 dB
///   Concerning:   < 7 dB
pub fn compute_hnr_db(
    samples: &[f32],
    sample_rate: u32,
    contour: &[PitchFrame],
    hop_size_ms: f32,
) -> Option<f32> {
    let sr = sample_rate as f32;
    let hop_samples = (hop_size_ms / 1000.0 * sr) as usize;

    let mut hnr_values: Vec<f32> = Vec::new();

    for (i, frame) in contour.iter().enumerate() {
        let Some(f0) = frame.frequency else {
            continue;
        };

        // Pitch period in samples — the lag at which we compute autocorrelation
        let period_samples = (sr / f0).round() as usize;
        if period_samples == 0 {
            continue;
        }

        // We need at least 2 periods of audio to compute autocorrelation at this lag.
        // Center the window around the frame position.
        let center = i * hop_samples;
        let window_size = period_samples * 3; // 3 periods gives a stable estimate
        let start = center.saturating_sub(window_size / 2);
        let end = (start + window_size).min(samples.len());

        if end - start < period_samples * 2 {
            continue; // not enough audio
        }

        let chunk = &samples[start..end];

        // Compute normalized autocorrelation at the pitch period lag.
        //
        // Autocorrelation at lag L:
        //   r(L) = Σ x(n) * x(n + L)  /  sqrt(Σ x(n)^2 * Σ x(n+L)^2)
        //
        // This is like a dot product between the signal and a shifted copy,
        // normalized so the result is between -1 and 1.
        let r = normalized_autocorrelation(chunk, period_samples);

        // Clamp r to a valid range — numerical imprecision can push it slightly
        // above 1.0 or below 0.0
        let r = r.clamp(0.001, 0.999);

        // Convert to dB: HNR = 10 * log10(r / (1 - r))
        // When r = 0.5 → HNR = 0 dB (equal harmonic and noise energy)
        // When r = 0.99 → HNR = 20 dB (harmonics 100x stronger than noise)
        // When r = 0.01 → HNR = -20 dB (almost pure noise)
        let hnr = 10.0 * (r / (1.0 - r)).log10();
        hnr_values.push(hnr);
    }

    if hnr_values.is_empty() {
        return None;
    }

    // Average HNR across all voiced frames
    let mean_hnr = hnr_values.iter().sum::<f32>() / hnr_values.len() as f32;
    Some(mean_hnr)
}

/// Compute the normalized autocorrelation of a signal at a given lag.
///
/// This compares the signal with a shifted copy of itself.
/// A value of 1.0 means the signal is perfectly periodic at this lag.
/// A value of 0.0 means no correlation (random noise).
fn normalized_autocorrelation(signal: &[f32], lag: usize) -> f32 {
    if lag >= signal.len() {
        return 0.0;
    }

    let n = signal.len() - lag;
    if n == 0 {
        return 0.0;
    }

    // Numerator: dot product of signal with shifted copy
    let mut cross_sum = 0.0_f64;
    // Denominators: energy of each segment
    let mut energy_a = 0.0_f64;
    let mut energy_b = 0.0_f64;

    for i in 0..n {
        let a = signal[i] as f64;
        let b = signal[i + lag] as f64;
        cross_sum += a * b;
        energy_a += a * a;
        energy_b += b * b;
    }

    let denom = (energy_a * energy_b).sqrt();
    if denom == 0.0 {
        return 0.0;
    }

    (cross_sum / denom) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn frame(time: f32, freq: Option<f32>) -> PitchFrame {
        PitchFrame {
            time,
            frequency: freq,
        }
    }

    fn sine_wave(freq: f32, sample_rate: u32, duration: f32) -> Vec<f32> {
        let n = (sample_rate as f32 * duration) as usize;
        (0..n)
            .map(|i| (2.0 * PI * freq * i as f32 / sample_rate as f32).sin())
            .collect()
    }

    #[test]
    fn pure_tone_high_hnr() {
        // A pure sine wave has no noise → HNR should be very high
        let sr = 44100;
        let samples = sine_wave(100.0, sr, 1.0);

        let hop_ms = 10.0;
        let num_frames = 80; // ~0.8s worth
        let contour: Vec<_> = (0..num_frames)
            .map(|i| frame(i as f32 * hop_ms / 1000.0, Some(100.0)))
            .collect();

        let hnr = compute_hnr_db(&samples, sr, &contour, hop_ms).unwrap();
        assert!(
            hnr > 20.0,
            "Pure tone should have HNR > 20 dB, got {hnr:.1} dB"
        );
    }

    #[test]
    fn noisy_signal_low_hnr() {
        // Mix a sine wave with white noise at ~1:1 ratio → low HNR
        let sr = 44100;
        let n = (sr as f32 * 1.0) as usize;

        // Simple deterministic "noise" using a linear congruential generator.
        // Real white noise would be random, but we want reproducible tests.
        let mut rng_state: u32 = 42;
        let samples: Vec<f32> = (0..n)
            .map(|i| {
                let signal = (2.0 * PI * 100.0 * i as f32 / sr as f32).sin();
                // LCG pseudo-random noise in [-1, 1]
                rng_state = rng_state.wrapping_mul(1103515245).wrapping_add(12345);
                let noise = (rng_state as f32 / u32::MAX as f32) * 2.0 - 1.0;
                // Equal parts signal and noise
                0.5 * signal + 0.5 * noise
            })
            .collect();

        let hop_ms = 10.0;
        let num_frames = 80;
        let contour: Vec<_> = (0..num_frames)
            .map(|i| frame(i as f32 * hop_ms / 1000.0, Some(100.0)))
            .collect();

        let hnr = compute_hnr_db(&samples, sr, &contour, hop_ms).unwrap();
        assert!(
            hnr < 15.0,
            "Noisy signal should have low HNR, got {hnr:.1} dB"
        );
    }

    #[test]
    fn autocorrelation_self() {
        // Autocorrelation at lag 0 should be 1.0 (signal correlates perfectly with itself)
        let signal: Vec<f32> = (0..100).map(|i| (i as f32 * 0.1).sin()).collect();
        let r = normalized_autocorrelation(&signal, 0);
        assert!(
            (r - 1.0).abs() < 0.001,
            "Self-correlation should be 1.0, got {r}"
        );
    }

    #[test]
    fn no_voiced_frames() {
        let contour = vec![frame(0.0, None), frame(0.01, None)];
        assert!(compute_hnr_db(&[0.0; 44100], 44100, &contour, 10.0).is_none());
    }
}
