use super::hnr::normalized_autocorrelation;
use super::pitch::PitchFrame;

/// Compute mean periodicity score across voiced, active frames.
///
/// For each frame that is both voiced (has pitch) and active (has energy),
/// computes the normalized autocorrelation at the pitch period lag.
/// Returns the mean across all such frames, in the range 0.0-1.0.
///
/// - 1.0 = perfectly periodic (pure tone)
/// - 0.0 = completely aperiodic (noise)
///
/// This is the same autocorrelation used in HNR computation, but returned
/// as a raw correlation value rather than converted to dB. It provides
/// a more intuitive 0-1 scale for periodicity.
///
/// Returns None if no frames have valid periodicity measurements.
pub fn compute_periodicity(
    samples: &[f32],
    sample_rate: u32,
    contour: &[PitchFrame],
    active_frames: &[bool],
    hop_size_ms: f32,
) -> Option<f32> {
    let sr = sample_rate as f32;
    let hop_samples = (hop_size_ms / 1000.0 * sr) as usize;

    let mut values: Vec<f32> = Vec::new();

    for (i, frame) in contour.iter().enumerate() {
        let Some(f0) = frame.frequency else {
            continue;
        };

        // Only use frames that are also active (have energy)
        if i < active_frames.len() && !active_frames[i] {
            continue;
        }

        let period_samples = (sr / f0).round() as usize;
        if period_samples == 0 {
            continue;
        }

        // Need at least 2 periods of audio for autocorrelation
        let center = i * hop_samples;
        let window_size = period_samples * 3;
        let start = center.saturating_sub(window_size / 2);
        let end = (start + window_size).min(samples.len());

        if end - start < period_samples * 2 {
            continue;
        }

        let chunk = &samples[start..end];
        let r = normalized_autocorrelation(chunk, period_samples);
        let r = r.clamp(0.0, 1.0);
        values.push(r);
    }

    if values.is_empty() {
        return None;
    }

    Some(values.iter().sum::<f32>() / values.len() as f32)
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
    fn sine_high_periodicity() {
        let sr = 44100;
        let samples = sine_wave(100.0, sr, 1.0);
        let hop_ms = 10.0;
        let num_frames = 80;
        let contour: Vec<_> = (0..num_frames)
            .map(|i| frame(i as f32 * hop_ms / 1000.0, Some(100.0)))
            .collect();
        let active = vec![true; num_frames];

        let p = compute_periodicity(&samples, sr, &contour, &active, hop_ms).unwrap();
        assert!(
            p > 0.95,
            "Pure sine should have periodicity near 1.0, got {p:.3}"
        );
    }

    #[test]
    fn noise_low_periodicity() {
        let sr = 44100;
        let n = (sr as f32 * 1.0) as usize;
        let mut rng_state: u32 = 42;
        let samples: Vec<f32> = (0..n)
            .map(|_| {
                rng_state = rng_state.wrapping_mul(1103515245).wrapping_add(12345);
                (rng_state as f32 / u32::MAX as f32) * 2.0 - 1.0
            })
            .collect();

        let hop_ms = 10.0;
        let num_frames = 80;
        let contour: Vec<_> = (0..num_frames)
            .map(|i| frame(i as f32 * hop_ms / 1000.0, Some(100.0)))
            .collect();
        let active = vec![true; num_frames];

        let p = compute_periodicity(&samples, sr, &contour, &active, hop_ms).unwrap();
        assert!(
            p < 0.3,
            "Pure noise should have low periodicity, got {p:.3}"
        );
    }

    #[test]
    fn no_voiced_frames() {
        let contour = vec![frame(0.0, None), frame(0.01, None)];
        let active = vec![true; 2];
        assert!(compute_periodicity(&[0.0; 44100], 44100, &contour, &active, 10.0).is_none());
    }

    #[test]
    fn inactive_frames_skipped() {
        let sr = 44100;
        let samples = sine_wave(100.0, sr, 1.0);
        let hop_ms = 10.0;
        let num_frames = 80;
        let contour: Vec<_> = (0..num_frames)
            .map(|i| frame(i as f32 * hop_ms / 1000.0, Some(100.0)))
            .collect();
        // All frames inactive
        let active = vec![false; num_frames];

        assert!(compute_periodicity(&samples, sr, &contour, &active, hop_ms).is_none());
    }
}
