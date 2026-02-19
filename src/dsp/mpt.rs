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
///
/// `max_bridge_ms` controls the maximum gap that gets bridged as a detector
/// dropout. Default: 250ms. Gaps longer than this are treated as the patient
/// stopping (breath or pause), ending the phonation measurement.
pub fn max_phonation_time_secs(contour: &[PitchFrame], hop_size_ms: f32, max_bridge_ms: f32) -> f32 {
    let runs = contour::voiced_runs(contour);
    // Bridge gaps up to max_bridge_ms caused by pitch detector dropouts.
    //
    // MPT measures sustained phonation — the patient holds a vowel as long as
    // possible. Short gaps are detector artifacts (breathy signal loses
    // periodicity momentarily). Longer gaps indicate the patient actually
    // stopped (e.g., took a breath), which should end the measurement.
    let max_gap_frames = (max_bridge_ms / hop_size_ms) as usize;
    let runs = contour::merge_close_runs(&runs, max_gap_frames);

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

        let mpt = max_phonation_time_secs(&contour, 10.0, 250.0);
        assert!(
            (mpt - 1.0).abs() < 0.02,
            "Expected ~1.0s MPT, got {mpt:.3}s"
        );
    }

    #[test]
    fn longest_run_wins() {
        // Two phonation attempts separated by a breathing pause (>500ms).
        // MPT should reflect the longer one.
        let mut contour: Vec<PitchFrame> = Vec::new();

        // First attempt: 200 frames = 2.0s
        for i in 0..200 {
            contour.push(frame(i as f32 * 0.01, Some(100.0)));
        }
        // Breathing pause: 60 frames = 600ms (> 500ms, not bridged)
        for i in 200..260 {
            contour.push(frame(i as f32 * 0.01, None));
        }
        // Second attempt: 500 frames = 5.0s
        for i in 260..760 {
            contour.push(frame(i as f32 * 0.01, Some(100.0)));
        }

        let mpt = max_phonation_time_secs(&contour, 10.0, 250.0);
        assert!(
            (mpt - 5.0).abs() < 0.1,
            "Expected ~5.0s (500 frames × 10ms), got {mpt:.3}s"
        );
    }

    #[test]
    fn bridges_detector_dropouts() {
        // Simulate pitch detector dropping frames in a 20s sustained vowel.
        // Gaps of 100ms and 200ms should be bridged (< 500ms threshold).
        let mut contour: Vec<PitchFrame> = Vec::new();
        // 10s voiced
        for i in 0..1000 {
            contour.push(frame(i as f32 * 0.01, Some(100.0)));
        }
        // 100ms gap (10 frames) — detector dropout
        for i in 1000..1010 {
            contour.push(frame(i as f32 * 0.01, None));
        }
        // 5s voiced
        for i in 1010..1510 {
            contour.push(frame(i as f32 * 0.01, Some(100.0)));
        }
        // 200ms gap (20 frames) — still a detector dropout
        for i in 1510..1530 {
            contour.push(frame(i as f32 * 0.01, None));
        }
        // 5s voiced
        for i in 1530..2030 {
            contour.push(frame(i as f32 * 0.01, Some(100.0)));
        }

        let mpt = max_phonation_time_secs(&contour, 10.0, 250.0);
        assert!(
            mpt > 20.0,
            "Should bridge gaps < 500ms in sustained phonation, got {mpt:.3}s"
        );
    }

    #[test]
    fn does_not_bridge_breathing_pause() {
        // A 600ms gap (> 500ms) indicates the patient stopped to breathe.
        // This should NOT be bridged — it's two separate phonation attempts.
        let mut contour: Vec<PitchFrame> = Vec::new();
        // 5s voiced
        for i in 0..500 {
            contour.push(frame(i as f32 * 0.01, Some(100.0)));
        }
        // 600ms gap (60 frames) — breathing pause
        for i in 500..560 {
            contour.push(frame(i as f32 * 0.01, None));
        }
        // 3s voiced
        for i in 560..860 {
            contour.push(frame(i as f32 * 0.01, Some(100.0)));
        }

        let mpt = max_phonation_time_secs(&contour, 10.0, 250.0);
        assert!(
            (mpt - 5.0).abs() < 0.1,
            "Breathing pause (>500ms) should not be bridged, expected ~5.0s, got {mpt:.3}s"
        );
    }

    #[test]
    fn does_not_bridge_300ms_gap() {
        // 300ms > 250ms threshold — should NOT bridge
        let mut contour: Vec<PitchFrame> = Vec::new();
        for i in 0..500 {
            contour.push(frame(i as f32 * 0.01, Some(100.0)));
        }
        // 300ms gap (30 frames)
        for i in 500..530 {
            contour.push(frame(i as f32 * 0.01, None));
        }
        for i in 530..830 {
            contour.push(frame(i as f32 * 0.01, Some(100.0)));
        }

        let mpt = max_phonation_time_secs(&contour, 10.0, 250.0);
        assert!(
            (mpt - 5.0).abs() < 0.1,
            "300ms gap should NOT be bridged at 250ms threshold, expected ~5.0s, got {mpt:.3}s"
        );
    }

    #[test]
    fn bridges_200ms_gap() {
        // 200ms < 250ms threshold — should bridge
        let mut contour: Vec<PitchFrame> = Vec::new();
        for i in 0..500 {
            contour.push(frame(i as f32 * 0.01, Some(100.0)));
        }
        // 200ms gap (20 frames)
        for i in 500..520 {
            contour.push(frame(i as f32 * 0.01, None));
        }
        for i in 520..820 {
            contour.push(frame(i as f32 * 0.01, Some(100.0)));
        }

        let mpt = max_phonation_time_secs(&contour, 10.0, 250.0);
        assert!(
            mpt > 8.0,
            "200ms gap should be bridged at 250ms threshold, got {mpt:.3}s"
        );
    }

    #[test]
    fn all_unvoiced() {
        let contour = vec![frame(0.0, None), frame(0.01, None)];
        assert_eq!(max_phonation_time_secs(&contour, 10.0, 250.0), 0.0);
    }

    #[test]
    fn empty_contour() {
        assert_eq!(max_phonation_time_secs(&[], 10.0, 250.0), 0.0);
    }
}
