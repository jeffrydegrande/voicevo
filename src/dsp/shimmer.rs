use super::pitch::PitchFrame;

/// Compute local shimmer from audio samples and a pitch contour.
///
/// Shimmer measures how much the amplitude varies from one pitch cycle
/// to the next. It detects air leak and inconsistent vocal cord closure.
///
/// Algorithm:
/// 1. For each voiced frame, we know the pitch period T = 1/F0
/// 2. Extract one pitch period's worth of audio from that frame position
/// 3. Compute the peak amplitude of that period
/// 4. Compare consecutive amplitudes: shimmer = mean(|A(i) - A(i+1)|) / mean(A)
///
/// Returns shimmer as a percentage. None if insufficient data.
///
/// Clinical thresholds (Praat):
///   Normal voice: < 3.81%
///   Pathological: > 3.81%
pub fn local_shimmer_percent(
    samples: &[f32],
    sample_rate: u32,
    contour: &[PitchFrame],
    hop_size_ms: f32,
) -> Option<f32> {
    let sr = sample_rate as f32;
    let hop_samples = (hop_size_ms / 1000.0 * sr) as usize;

    // For each voiced frame, compute the peak amplitude over one pitch period.
    // We pair up the amplitude with whether this frame is consecutive to the
    // previous voiced frame (no unvoiced gap in between).
    let mut amplitudes: Vec<f32> = Vec::new();
    let mut perturbations: Vec<f32> = Vec::new();
    let mut prev_amp: Option<f32> = None;

    for (i, frame) in contour.iter().enumerate() {
        match frame.frequency {
            Some(f0) => {
                // One pitch period in samples: at 100 Hz and 44100 Hz SR, that's 441 samples
                let period_samples = (sr / f0).round() as usize;

                // Position of this frame in the audio buffer
                let start = i * hop_samples;
                let end = (start + period_samples).min(samples.len());

                if start >= samples.len() || start >= end {
                    prev_amp = None;
                    continue;
                }

                // Peak amplitude over this one pitch period
                let amp = samples[start..end]
                    .iter()
                    .fold(0.0_f32, |max, &s| max.max(s.abs()));

                amplitudes.push(amp);

                if let Some(prev) = prev_amp {
                    perturbations.push((amp - prev).abs());
                }

                prev_amp = Some(amp);
            }
            None => {
                prev_amp = None;
            }
        }
    }

    if perturbations.is_empty() || amplitudes.is_empty() {
        return None;
    }

    let mean_perturbation: f32 = perturbations.iter().sum::<f32>() / perturbations.len() as f32;
    let mean_amplitude: f32 = amplitudes.iter().sum::<f32>() / amplitudes.len() as f32;

    if mean_amplitude == 0.0 {
        return None;
    }

    Some((mean_perturbation / mean_amplitude) * 100.0)
}

/// Minimum consecutive tier-1/2 frames needed for a valid gated measurement.
const MIN_GATED_FRAMES: usize = 15;
/// Minimum total duration of gated frames (seconds) for a valid measurement.
const MIN_GATED_DURATION_S: f32 = 1.5;

/// Compute gated local shimmer using only tier 1/2 frames.
///
/// Only frames detected by standard or relaxed pitch detection (tiers 1-2)
/// contribute. Tier 3 frames have estimated pitch and would produce
/// unreliable amplitude windows.
pub fn local_shimmer_percent_gated(
    samples: &[f32],
    sample_rate: u32,
    contour: &[PitchFrame],
    frame_tiers: &[u8],
    hop_size_ms: f32,
) -> Option<f32> {
    if contour.len() != frame_tiers.len() {
        return None;
    }

    let sr = sample_rate as f32;
    let hop_samples = (hop_size_ms / 1000.0 * sr) as usize;

    let mut amplitudes: Vec<f32> = Vec::new();
    let mut perturbations: Vec<f32> = Vec::new();
    let mut prev_amp: Option<f32> = None;
    let mut consecutive = 0_usize;
    let mut max_consecutive = 0_usize;
    let mut total_gated_frames = 0_usize;

    for (i, (frame, &tier)) in contour.iter().zip(frame_tiers.iter()).enumerate() {
        match frame.frequency {
            Some(f0) if tier <= 2 => {
                let period_samples = (sr / f0).round() as usize;
                let start = i * hop_samples;
                let end = (start + period_samples).min(samples.len());

                if start >= samples.len() || start >= end {
                    prev_amp = None;
                    consecutive = 0;
                    continue;
                }

                let amp = samples[start..end]
                    .iter()
                    .fold(0.0_f32, |max, &s| max.max(s.abs()));

                amplitudes.push(amp);
                total_gated_frames += 1;
                consecutive += 1;
                max_consecutive = max_consecutive.max(consecutive);

                if let Some(prev) = prev_amp {
                    perturbations.push((amp - prev).abs());
                }
                prev_amp = Some(amp);
            }
            _ => {
                prev_amp = None;
                consecutive = 0;
            }
        }
    }

    if max_consecutive < MIN_GATED_FRAMES {
        return None;
    }
    let total_duration_s = total_gated_frames as f32 * hop_size_ms / 1000.0;
    if total_duration_s < MIN_GATED_DURATION_S {
        return None;
    }

    if perturbations.is_empty() || amplitudes.is_empty() {
        return None;
    }

    let mean_perturbation: f32 = perturbations.iter().sum::<f32>() / perturbations.len() as f32;
    let mean_amplitude: f32 = amplitudes.iter().sum::<f32>() / amplitudes.len() as f32;

    if mean_amplitude == 0.0 {
        return None;
    }

    Some((mean_perturbation / mean_amplitude) * 100.0)
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

    /// Generate a pure sine wave — constant amplitude, so shimmer should be ~0%.
    fn sine_wave(freq: f32, sample_rate: u32, duration: f32) -> Vec<f32> {
        let n = (sample_rate as f32 * duration) as usize;
        (0..n)
            .map(|i| (2.0 * PI * freq * i as f32 / sample_rate as f32).sin())
            .collect()
    }

    #[test]
    fn constant_amplitude_low_shimmer() {
        let sr = 44100;
        let samples = sine_wave(100.0, sr, 0.5);

        // Build contour: all frames at 100 Hz, 10ms hop
        let hop_ms = 10.0;
        let num_frames = (500.0 / hop_ms) as usize;
        let contour: Vec<_> = (0..num_frames)
            .map(|i| frame(i as f32 * hop_ms / 1000.0, Some(100.0)))
            .collect();

        let shimmer = local_shimmer_percent(&samples, sr, &contour, hop_ms).unwrap();
        assert!(
            shimmer < 5.0,
            "Constant-amplitude sine should have low shimmer, got {shimmer:.2}%"
        );
    }

    #[test]
    fn amplitude_modulated_high_shimmer() {
        let sr = 44100;
        // Create a signal where amplitude alternates between 0.5 and 1.0
        let n = (sr as f32 * 0.5) as usize;
        let hop_ms = 10.0;
        let hop_samples = (hop_ms / 1000.0 * sr as f32) as usize;

        let samples: Vec<f32> = (0..n)
            .map(|i| {
                let frame_idx = i / hop_samples;
                let amp = if frame_idx % 2 == 0 { 1.0 } else { 0.5 };
                amp * (2.0 * PI * 100.0 * i as f32 / sr as f32).sin()
            })
            .collect();

        let num_frames = n / hop_samples;
        let contour: Vec<_> = (0..num_frames)
            .map(|i| frame(i as f32 * hop_ms / 1000.0, Some(100.0)))
            .collect();

        let shimmer = local_shimmer_percent(&samples, sr, &contour, hop_ms).unwrap();
        assert!(
            shimmer > 20.0,
            "Alternating amplitude should show high shimmer, got {shimmer:.2}%"
        );
    }

    #[test]
    fn insufficient_data() {
        let contour = vec![frame(0.0, Some(100.0))];
        assert!(local_shimmer_percent(&[0.0; 1000], 44100, &contour, 10.0).is_none());
    }
}
