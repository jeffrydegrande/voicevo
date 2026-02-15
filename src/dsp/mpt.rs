use super::contour;
use super::pitch::PitchFrame;

/// Compute Maximum Phonation Time from a pitch contour.
///
/// MPT is the duration of the longest continuous stretch of voiced frames.
/// This reflects how efficiently the vocal cords use air: healthy cords
/// close fully, use air sparingly, and can sustain a vowel for 15-25 seconds.
/// With paralysis, air leaks through the gap, depleting the supply faster.
///
/// Returns the MPT in seconds. Returns 0.0 if there are no voiced frames.
pub fn max_phonation_time_secs(contour: &[PitchFrame], hop_size_ms: f32) -> f32 {
    let runs = contour::voiced_runs(contour);

    runs.iter()
        .map(|&(start, end)| contour::run_duration_secs(start, end, hop_size_ms))
        .fold(0.0_f32, f32::max)
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
    fn continuous_voicing() {
        // 100 voiced frames at 10ms hop = 1.0 seconds
        let contour: Vec<_> = (0..100)
            .map(|i| frame(i as f32 * 0.01, Some(100.0)))
            .collect();

        let mpt = max_phonation_time_secs(&contour, 10.0);
        assert!(
            (mpt - 1.0).abs() < 0.02,
            "Expected ~1.0s MPT, got {mpt:.3}s"
        );
    }

    #[test]
    fn longest_run_wins() {
        // Two voiced segments: 20 frames and 50 frames. MPT should reflect the longer one.
        let mut contour: Vec<PitchFrame> = Vec::new();

        // First segment: 20 frames
        for i in 0..20 {
            contour.push(frame(i as f32 * 0.01, Some(100.0)));
        }
        // Gap: 5 frames
        for i in 20..25 {
            contour.push(frame(i as f32 * 0.01, None));
        }
        // Second segment: 50 frames
        for i in 25..75 {
            contour.push(frame(i as f32 * 0.01, Some(100.0)));
        }

        let mpt = max_phonation_time_secs(&contour, 10.0);
        assert!(
            (mpt - 0.5).abs() < 0.02,
            "Expected ~0.5s (50 frames × 10ms), got {mpt:.3}s"
        );
    }

    #[test]
    fn all_unvoiced() {
        let contour = vec![frame(0.0, None), frame(0.01, None)];
        assert_eq!(max_phonation_time_secs(&contour, 10.0), 0.0);
    }

    #[test]
    fn empty_contour() {
        assert_eq!(max_phonation_time_secs(&[], 10.0), 0.0);
    }
}
