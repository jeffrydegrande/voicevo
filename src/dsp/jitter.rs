use super::pitch::PitchFrame;

/// Compute local jitter from a pitch contour.
///
/// Jitter measures how much the pitch period (1/F0) varies from one cycle
/// to the next. It's a key indicator of vocal cord stability.
///
/// Formula:
///   jitter_local = mean(|T(i) - T(i+1)|) / mean(T)
///
/// where T(i) = 1/F0(i) is the pitch period in seconds.
///
/// We only use consecutive voiced frames — unvoiced gaps are skipped.
///
/// Returns the jitter as a percentage (e.g., 1.5 means 1.5%).
/// Returns None if there aren't enough consecutive voiced frames (need >= 2).
///
/// Clinical thresholds (Praat):
///   Normal voice: < 1.04%
///   Pathological: > 1.04%
pub fn local_jitter_percent(contour: &[PitchFrame]) -> Option<f32> {
    // Collect consecutive voiced frequencies.
    // We need pairs of adjacent voiced frames — if there's an unvoiced frame
    // in between, the pair is broken (we can't measure cycle-to-cycle variation
    // across a gap).
    let mut periods: Vec<f32> = Vec::new();
    let mut perturbations: Vec<f32> = Vec::new();

    // prev_period tracks the period of the previous voiced frame.
    // It's reset to None whenever we hit an unvoiced frame.
    let mut prev_period: Option<f32> = None;

    for frame in contour {
        match frame.frequency {
            Some(f0) => {
                let period = 1.0 / f0;
                periods.push(period);

                if let Some(prev) = prev_period {
                    perturbations.push((period - prev).abs());
                }

                prev_period = Some(period);
            }
            None => {
                // Gap in voicing — reset the chain
                prev_period = None;
            }
        }
    }

    if perturbations.is_empty() || periods.is_empty() {
        return None;
    }

    let mean_perturbation: f32 = perturbations.iter().sum::<f32>() / perturbations.len() as f32;
    let mean_period: f32 = periods.iter().sum::<f32>() / periods.len() as f32;

    if mean_period == 0.0 {
        return None;
    }

    Some((mean_perturbation / mean_period) * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(time: f32, freq: Option<f32>) -> PitchFrame {
        PitchFrame {
            time,
            frequency: freq,
        }
    }

    #[test]
    fn perfect_signal_zero_jitter() {
        // All frames at exactly 100 Hz — zero perturbation
        let contour: Vec<_> = (0..50)
            .map(|i| frame(i as f32 * 0.01, Some(100.0)))
            .collect();

        let jitter = local_jitter_percent(&contour).unwrap();
        assert!(
            jitter < 0.001,
            "Perfect signal should have ~0% jitter, got {jitter:.4}%"
        );
    }

    #[test]
    fn known_jitter() {
        // Alternating between 100 Hz and 110 Hz.
        // Periods: 0.01s and 0.00909s
        // |diff| = 0.000909... each time
        // Mean period = (0.01 + 0.00909) / 2 ≈ 0.009545
        // Jitter = 0.000909 / 0.009545 ≈ 9.5%
        let contour: Vec<_> = (0..20)
            .map(|i| {
                let f0 = if i % 2 == 0 { 100.0 } else { 110.0 };
                frame(i as f32 * 0.01, Some(f0))
            })
            .collect();

        let jitter = local_jitter_percent(&contour).unwrap();
        assert!(
            (jitter - 9.5).abs() < 1.0,
            "Expected ~9.5% jitter, got {jitter:.2}%"
        );
    }

    #[test]
    fn gap_breaks_chain() {
        // Two voiced segments separated by a gap — the gap should NOT
        // contribute a perturbation measurement
        let contour = vec![
            frame(0.0, Some(100.0)),
            frame(0.01, Some(100.0)),
            frame(0.02, None), // gap
            frame(0.03, Some(200.0)), // different pitch, but gap breaks chain
            frame(0.04, Some(200.0)),
        ];

        let jitter = local_jitter_percent(&contour).unwrap();
        // Within each segment, pitch is constant → 0% jitter
        assert!(
            jitter < 0.001,
            "Should be ~0% jitter (gap breaks the chain), got {jitter:.4}%"
        );
    }

    #[test]
    fn too_few_frames() {
        let contour = vec![frame(0.0, Some(100.0))];
        assert!(local_jitter_percent(&contour).is_none());
    }

    #[test]
    fn all_unvoiced() {
        let contour = vec![frame(0.0, None), frame(0.01, None)];
        assert!(local_jitter_percent(&contour).is_none());
    }
}
